// VS Code-bound test helpers: source/conditional breakpoint management and
// debug-session lifecycle waiters. The VS Code `debug` surface and the
// breakpoint constructor are INJECTED, so these unit-test against fakes and run
// against the real `vscode` namespace in a test host with no code change.

import { pollUntilResult } from "../poll";

/** The breakpoint subset of `vscode.debug` the tools mutate. */
export interface VscodeBreakpointApi {
  readonly breakpoints: readonly unknown[];
  addBreakpoints(breakpoints: unknown[]): void;
  removeBreakpoints(breakpoints: readonly unknown[]): void;
}

/** Builds a host SourceBreakpoint (the consumer wires vscode's classes). */
export type BreakpointFactory = (
  filePath: string,
  line: number,
  condition?: string,
) => unknown;

/** A breakpoint spec: a bare 1-based line, or a line with a condition. */
export type BreakpointSpec = number | { line: number; condition?: string };

/** Breakpoint tools bound to a VS Code debug API + breakpoint factory. */
export interface BreakpointTools {
  /** Replace all breakpoints with one per spec on `filePath`. */
  setSourceBreakpoints(filePath: string, specs: BreakpointSpec[]): void;
  /** Remove every breakpoint. */
  clearBreakpoints(): void;
}

/** Bind breakpoint tools to a debug API and a host breakpoint factory. */
export function makeBreakpointTools(
  api: VscodeBreakpointApi,
  makeBreakpoint: BreakpointFactory,
): BreakpointTools {
  return {
    setSourceBreakpoints(filePath, specs) {
      api.removeBreakpoints(api.breakpoints);
      api.addBreakpoints(
        specs.map((spec) =>
          typeof spec === "number"
            ? makeBreakpoint(filePath, spec)
            : makeBreakpoint(filePath, spec.line, spec.condition),
        ),
      );
    },
    clearBreakpoints() {
      api.removeBreakpoints(api.breakpoints);
    },
  };
}

/** A started debug session (only the id is read by the waiters). */
export interface DebugSessionHandle {
  readonly id: string;
}

/** The session-lifecycle subset of `vscode.debug`. */
export interface VscodeSessionApi<S extends DebugSessionHandle = DebugSessionHandle> {
  onDidStartDebugSession(listener: (session: S) => void): { dispose(): void };
  readonly activeDebugSession: S | undefined;
}

/** Session-lifecycle waiters bound to a VS Code debug API. */
export interface SessionWaiters<S extends DebugSessionHandle> {
  /** Resolve with the next started debug session, or reject on timeout. */
  waitForDebugSessionStart(timeoutMs?: number): Promise<S>;
  /** Resolve once no (matching) session is active, or reject on timeout. */
  waitForDebugSessionEnd(timeoutMs?: number, sessionId?: string): Promise<void>;
}

/** Bind session-lifecycle waiters to a debug API. */
export function makeSessionWaiters<S extends DebugSessionHandle>(
  api: VscodeSessionApi<S>,
): SessionWaiters<S> {
  return {
    waitForDebugSessionStart(timeoutMs = 30_000) {
      return new Promise<S>((resolve, reject) => {
        const timer = setTimeout(() => {
          subscription.dispose();
          reject(new Error(`Debug session did not start within ${timeoutMs}ms`));
        }, timeoutMs);
        const subscription = api.onDidStartDebugSession((session) => {
          clearTimeout(timer);
          subscription.dispose();
          resolve(session);
        });
      });
    },
    async waitForDebugSessionEnd(timeoutMs = 30_000, sessionId?: string) {
      const ended = await pollUntilResult(
        () => Promise.resolve(api.activeDebugSession),
        (active) => !active || (sessionId !== undefined && active.id !== sessionId),
        timeoutMs,
        100,
      );
      if (ended && !(sessionId !== undefined && ended.id !== sessionId)) {
        throw new Error(`Debug session did not end within ${timeoutMs}ms`);
      }
    },
  };
}
