// Implements [PROF-UI-MEMORY]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand } from "./ids";
import type { ProfilerStore as Store } from "./store";

/** The running LSP client handle. */
export type LspClient = NonNullable<Store["client"]["value"]>;

/** Everything one staged memory round-trip needs. */
export interface MemoryOperation {
  readonly store: Store;
  readonly command: string;
  readonly extraArgs: Record<string, unknown>;
  readonly report: (message: string) => void;
  /** Suppress user-facing warnings (the autopilot captures silently). */
  readonly quiet: boolean;
}

/** The running client and the memory session it tracks. */
export interface ActiveMemorySession {
  readonly client: LspClient;
  readonly memorySessionId: string;
}

/** The acquired-frame context one round-trip evaluates against. */
export interface OperationContext extends ActiveMemorySession {
  readonly frameId: number;
}

/** Show `message` unless the operation captures silently. */
export function warnUnlessQuiet(op: MemoryOperation, message: string): void {
  if (!op.quiet) { void vscode.window.showWarningMessage(message); }
}

/**
 * Resolve the running client + active memory session, or null (warning unless
 * `quiet`) when the LSP is down or tracking is not active.
 */
export function resolveActiveSession(op: MemoryOperation): ActiveMemorySession | null {
  const client = op.store.client.value;
  if (client === undefined || !client.isRunning()) {
    return notConnected(op);
  }
  const { memorySessionId } = op.store.profiler.value;
  if (op.store.profiler.value.memory !== "active" || memorySessionId === undefined) {
    warnUnlessQuiet(op, `${brand()}: Start memory tracking first.`);
    return null;
  }
  return { client, memorySessionId };
}

/** The "LSP not connected" refusal, with a warning unless the op is quiet. */
function notConnected(op: MemoryOperation): null {
  if (!op.quiet) { void vscode.window.showErrorMessage(`${brand()} LSP not connected`); }
  return null;
}
