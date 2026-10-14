// A DAP test client over a session that exposes `customRequest` (the shape VS
// Code's DebugSession provides). Every wrapper is language-neutral and depends
// only on `customRequest`, so it unit-tests against a fake session and drives a
// real session in a VS Code test host unchanged. This is the harness Osprey,
// Basilisk and SharpLsp e2e suites share instead of each re-deriving it.

import { pollUntilResult } from "../poll";

/** The single capability the DAP client needs from a debug session. */
export interface DapSessionLike {
  customRequest(command: string, args?: unknown): Promise<unknown>;
}

/** A DAP stack frame (the fields tests assert on). */
export interface DapStackFrame {
  id: number;
  name: string;
  source?: { path?: string; name?: string };
  line: number;
  column: number;
}

/** A DAP variable/scope entry. */
export interface DapVariable {
  name: string;
  value: string;
  type?: string;
  variablesReference: number;
}

/** A DAP scope. */
export interface DapScope {
  name: string;
  variablesReference: number;
  expensive?: boolean;
}

/** Result of an `evaluate` request (watch/repl/hover). */
export interface DapEvaluateResult {
  result: string;
  type?: string;
  variablesReference: number;
}

/** A stopped debuggee: the thread and its current stack. */
export interface DapStop {
  threadId: number;
  stack: { stackFrames: DapStackFrame[] };
}

const DEFAULT_STACK_LEVELS = 20;

/** All threads currently known to the adapter. */
export async function threads(
  session: DapSessionLike,
): Promise<{ id: number; name: string }[]> {
  const response = (await session.customRequest("threads")) as {
    threads?: { id: number; name: string }[];
  };
  return response.threads ?? [];
}

/** The stack trace for a thread. */
export async function stackTrace(
  session: DapSessionLike,
  threadId: number,
  levels = DEFAULT_STACK_LEVELS,
): Promise<{ stackFrames: DapStackFrame[] }> {
  return session.customRequest("stackTrace", {
    threadId,
    startFrame: 0,
    levels,
  }) as Promise<{ stackFrames: DapStackFrame[] }>;
}

/** The scopes of a stack frame. */
export async function scopes(
  session: DapSessionLike,
  frameId: number,
): Promise<{ scopes: DapScope[] }> {
  return session.customRequest("scopes", { frameId }) as Promise<{
    scopes: DapScope[];
  }>;
}

/** The variables under a variablesReference. */
export async function variables(
  session: DapSessionLike,
  variablesReference: number,
): Promise<{ variables: DapVariable[] }> {
  return session.customRequest("variables", {
    variablesReference,
  }) as Promise<{ variables: DapVariable[] }>;
}

/** Evaluate an expression in a frame (watch/repl/hover context). */
export async function evaluate(
  session: DapSessionLike,
  expression: string,
  frameId: number,
  context: "watch" | "repl" | "hover" = "watch",
): Promise<DapEvaluateResult> {
  return session.customRequest("evaluate", {
    expression,
    frameId,
    context,
  }) as Promise<DapEvaluateResult>;
}

/** Step over (DAP `next`). */
export function stepOver(session: DapSessionLike, threadId: number): Promise<unknown> {
  return session.customRequest("next", { threadId });
}

/** Step into (DAP `stepIn`). */
export function stepIn(session: DapSessionLike, threadId: number): Promise<unknown> {
  return session.customRequest("stepIn", { threadId });
}

/** Step out (DAP `stepOut`). */
export function stepOut(session: DapSessionLike, threadId: number): Promise<unknown> {
  return session.customRequest("stepOut", { threadId });
}

/** Resume (DAP `continue`). */
export function continueExecution(
  session: DapSessionLike,
  threadId: number,
): Promise<unknown> {
  return session.customRequest("continue", { threadId });
}

/** Flatten every concrete (referenceable) scope's variables for a frame. */
export async function readFrameVariables(
  session: DapSessionLike,
  frameId: number,
): Promise<DapVariable[]> {
  const frameScopes = await scopes(session, frameId);
  const groups = await Promise.all(
    frameScopes.scopes
      .filter((scope) => scope.variablesReference > 0)
      .map((scope) => variables(session, scope.variablesReference)),
  );
  return groups.flatMap((group) => group.variables);
}

/** Find a named local/argument in a frame, or undefined. */
export async function findVariable(
  session: DapSessionLike,
  frameId: number,
  name: string,
): Promise<DapVariable | undefined> {
  const all = await readFrameVariables(session, frameId);
  return all.find((variable) => variable.name === name);
}

/**
 * Poll until the debuggee is stopped with at least one stack frame, returning
 * the stopped thread and its stack. Resolves the first thread that reports a
 * frame. Throws a labelled error on timeout.
 */
export async function waitForStop(
  session: DapSessionLike,
  timeoutMs = 30_000,
): Promise<DapStop> {
  const probe = async (): Promise<DapStop | undefined> => {
    for (const thread of await threadsQuietly(session)) {
      const stack = await stackTraceQuietly(session, thread.id);
      if (stack && stack.stackFrames.length > 0) {
        return { threadId: thread.id, stack };
      }
    }
    return undefined;
  };
  const stop = await pollUntilResult(
    probe,
    (value): value is DapStop => value !== undefined,
    timeoutMs,
    100,
  );
  if (!stop) {
    throw new Error(`Timed out after ${timeoutMs}ms waiting for the debuggee to stop`);
  }
  return stop;
}

async function threadsQuietly(
  session: DapSessionLike,
): Promise<{ id: number; name: string }[]> {
  try {
    return await threads(session);
  } catch {
    return [];
  }
}

async function stackTraceQuietly(
  session: DapSessionLike,
  threadId: number,
): Promise<{ stackFrames: DapStackFrame[] } | undefined> {
  try {
    return await stackTrace(session, threadId);
  } catch {
    return undefined;
  }
}
