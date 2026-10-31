// VS Code-bound test helpers: source/conditional breakpoint management and
// debug-session lifecycle waiters. The VS Code `debug` surface and the
// breakpoint constructor are INJECTED, so these unit-test against fakes and run
// against the real `vscode` namespace in a test host with no code change.

import { pollUntilResult } from "../poll";

/** Default budget for the session-lifecycle waiters. */
const DEFAULT_SESSION_TIMEOUT_MS = 30_000;

/** Delay between checks that the active debug session has ended. */
const SESSION_POLL_INTERVAL_MS = 100;

/** The breakpoint subset of `vscode.debug` the tools mutate. */
export interface VscodeBreakpointApi {
  readonly breakpoints: readonly unknown[];
  addBreakpoints: (breakpoints: unknown[]) => void;
  removeBreakpoints: (breakpoints: readonly unknown[]) => void;
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
  setSourceBreakpoints: (filePath: string, specs: BreakpointSpec[]) => void;
  /** Remove every breakpoint. */
  clearBreakpoints: () => void;
}

/** The breakpoint for one spec: a bare line, or a line plus condition. */
function breakpointFor(
  makeBreakpoint: BreakpointFactory,
  filePath: string,
  spec: BreakpointSpec,
): unknown {
  return typeof spec === "number"
    ? makeBreakpoint(filePath, spec)
    : makeBreakpoint(filePath, spec.line, spec.condition);
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
        specs.map((spec) => breakpointFor(makeBreakpoint, filePath, spec)),
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

/** What an `onDidStartDebugSession` registration hands back. */
export interface SessionSubscription {
  dispose: () => void;
}

/** The session-lifecycle subset of `vscode.debug`. */
export interface VscodeSessionApi<TSession extends DebugSessionHandle = DebugSessionHandle> {
  onDidStartDebugSession: (listener: (session: TSession) => void) => SessionSubscription;
  readonly activeDebugSession: TSession | undefined;
}

/** Session-lifecycle waiters bound to a VS Code debug API. */
export interface SessionWaiters<TSession extends DebugSessionHandle> {
  /** Resolve with the next started debug session, or reject on timeout. */
  waitForDebugSessionStart: (timeoutMs?: number) => Promise<TSession>;
  /** Resolve once no (matching) session is active, or reject on timeout. */
  waitForDebugSessionEnd: (timeoutMs?: number, sessionId?: string) => Promise<void>;
}

/** Resolve with the next started session, rejecting on timeout. */
async function startSession<TSession extends DebugSessionHandle>(
  api: VscodeSessionApi<TSession>,
  timeoutMs: number,
): Promise<TSession> {
  const started: TSession[] = [];
  const subscription = api.onDidStartDebugSession((session) => { started.push(session); });
  try {
    const first = await pollUntilResult({
      fn: (): PromiseLike<TSession | undefined> => Promise.resolve(started.at(0)),
      predicate: (session) => session !== undefined,
      timeoutMs,
      intervalMs: SESSION_POLL_INTERVAL_MS,
    });
    if (first === undefined) {
      throw new Error(`Debug session did not start within ${timeoutMs}ms`);
    }
    return first;
  } finally {
    subscription.dispose();
  }
}

/** True when the (matching) session is no longer the active one. */
function hasEnded(
  active: DebugSessionHandle | undefined,
  sessionId: string | undefined,
): boolean {
  return active === undefined || (sessionId !== undefined && active.id !== sessionId);
}

/** Resolve once no (matching) session is active, or reject on timeout. */
async function endSession<TSession extends DebugSessionHandle>(
  api: VscodeSessionApi<TSession>,
  timeoutMs: number,
  sessionId?: string,
): Promise<void> {
  const readActive = (): PromiseLike<TSession | undefined> =>
    Promise.resolve(api.activeDebugSession);
  const ended = await pollUntilResult({
    fn: readActive,
    predicate: (active) => hasEnded(active, sessionId),
    timeoutMs,
    intervalMs: SESSION_POLL_INTERVAL_MS,
  });
  if (!hasEnded(ended, sessionId)) {
    throw new Error(`Debug session did not end within ${timeoutMs}ms`);
  }
}

/** Bind session-lifecycle waiters to a debug API. */
export function makeSessionWaiters<TSession extends DebugSessionHandle>(
  api: VscodeSessionApi<TSession>,
): SessionWaiters<TSession> {
  return {
    waitForDebugSessionStart: async (timeoutMs) => {
      const started = await startSession(api, timeoutMs ?? DEFAULT_SESSION_TIMEOUT_MS);
      return started;
    },
    waitForDebugSessionEnd: async (timeoutMs, sessionId) => {
      await endSession(api, timeoutMs ?? DEFAULT_SESSION_TIMEOUT_MS, sessionId);
    },
  };
}
