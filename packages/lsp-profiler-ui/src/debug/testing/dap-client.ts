// A DAP test client over a session that exposes `customRequest` (the shape VS
// Code's DebugSession provides). Every wrapper is language-neutral and depends
// only on `customRequest`, so it unit-tests against a fake session and drives a
// real session in a VS Code test host unchanged. This is the harness Osprey,
// Basilisk and SharpLsp e2e suites share instead of each re-deriving it.

import { pollUntilResult } from "../poll";

/** The single capability the DAP client needs from a debug session. */
export interface DapSessionLike {
  customRequest: (command: string, args?: unknown) => Promise<unknown>;
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

/** A thread as reported by the adapter. */
interface DapThread {
  id: number;
  name: string;
}

/** A stack frame's source location. */
type DapFrameSource = NonNullable<DapStackFrame["source"]>;

const DEFAULT_STACK_LEVELS = 20;
const DEFAULT_STOP_TIMEOUT_MS = 30_000;
const STOP_POLL_INTERVAL_MS = 100;

/** True when `value` is a non-null object whose fields can be inspected. */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** The elements of `value` when it is an array, and no elements otherwise. */
function arrayItems(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

/** True when an optional field is absent or a string. */
function isOptionalString(value: unknown): boolean {
  return value === undefined || typeof value === "string";
}

function isThread(value: unknown): value is DapThread {
  return isRecord(value) && typeof value.id === "number" && typeof value.name === "string";
}

function isFrameSource(value: unknown): value is DapFrameSource {
  return isRecord(value) && isOptionalString(value.path) && isOptionalString(value.name);
}

function isStackFrame(value: unknown): value is DapStackFrame {
  return (
    isRecord(value) &&
    typeof value.id === "number" &&
    typeof value.name === "string" &&
    typeof value.line === "number" &&
    typeof value.column === "number" &&
    (value.source === undefined || isFrameSource(value.source))
  );
}

function isScope(value: unknown): value is DapScope {
  return (
    isRecord(value) &&
    typeof value.name === "string" &&
    typeof value.variablesReference === "number" &&
    (value.expensive === undefined || typeof value.expensive === "boolean")
  );
}

function isVariable(value: unknown): value is DapVariable {
  return (
    isRecord(value) &&
    typeof value.name === "string" &&
    typeof value.value === "string" &&
    typeof value.variablesReference === "number" &&
    isOptionalString(value.type)
  );
}

function isEvaluateResult(value: unknown): value is DapEvaluateResult {
  return (
    isRecord(value) &&
    typeof value.result === "string" &&
    typeof value.variablesReference === "number" &&
    isOptionalString(value.type)
  );
}

/** The `key` array of a response body, keeping only well-formed entries. */
function arrayField<TItem>(
  body: unknown,
  key: string,
  isItem: (item: unknown) => item is TItem,
): TItem[] {
  return isRecord(body)
    ? arrayItems(body[key]).filter((item): item is TItem => isItem(item))
    : [];
}

/** All threads currently known to the adapter. */
export async function threads(
  session: DapSessionLike,
): Promise<{ id: number; name: string }[]> {
  const response = await session.customRequest("threads");
  return arrayField(response, "threads", isThread);
}

/** The stack trace for a thread. */
export async function stackTrace(
  session: DapSessionLike,
  threadId: number,
  levels = DEFAULT_STACK_LEVELS,
): Promise<{ stackFrames: DapStackFrame[] }> {
  const response = await session.customRequest("stackTrace", {
    threadId,
    startFrame: 0,
    levels,
  });
  return { stackFrames: arrayField(response, "stackFrames", isStackFrame) };
}

/** The scopes of a stack frame. */
export async function scopes(
  session: DapSessionLike,
  frameId: number,
): Promise<{ scopes: DapScope[] }> {
  const response = await session.customRequest("scopes", { frameId });
  return { scopes: arrayField(response, "scopes", isScope) };
}

/** The variables under a variablesReference. */
export async function variables(
  session: DapSessionLike,
  variablesReference: number,
): Promise<{ variables: DapVariable[] }> {
  const response = await session.customRequest("variables", { variablesReference });
  return { variables: arrayField(response, "variables", isVariable) };
}

/** The result used when the adapter answers `evaluate` malformed. */
function emptyEvaluateResult(): DapEvaluateResult {
  return { result: "", variablesReference: 0 };
}

/** The DAP contexts an `evaluate` request may run in. */
export type EvaluateContext = "watch" | "repl" | "hover";

/** The fields of a DAP `evaluate` request. */
export interface EvaluateRequest {
  expression: string;
  frameId: number;
  context?: EvaluateContext;
}

/** Evaluate an expression in a frame (watch/repl/hover context). */
export async function evaluate(
  session: DapSessionLike,
  request: EvaluateRequest,
): Promise<DapEvaluateResult> {
  const { expression, frameId, context = "watch" } = request;
  const response = await session.customRequest("evaluate", {
    expression,
    frameId,
    context,
  });
  return isEvaluateResult(response) ? response : emptyEvaluateResult();
}

/** Issue a thread-scoped DAP request (step/resume), awaiting its response. */
async function threadRequest(
  session: DapSessionLike,
  command: string,
  threadId: number,
): Promise<unknown> {
  const response = await session.customRequest(command, { threadId });
  return response;
}

/** Step over (DAP `next`). */
export async function stepOver(
  session: DapSessionLike,
  threadId: number,
): Promise<unknown> {
  const response = await threadRequest(session, "next", threadId);
  return response;
}

/** Step into (DAP `stepIn`). */
export async function stepIn(
  session: DapSessionLike,
  threadId: number,
): Promise<unknown> {
  const response = await threadRequest(session, "stepIn", threadId);
  return response;
}

/** Step out (DAP `stepOut`). */
export async function stepOut(
  session: DapSessionLike,
  threadId: number,
): Promise<unknown> {
  const response = await threadRequest(session, "stepOut", threadId);
  return response;
}

/** Resume (DAP `continue`). */
export async function continueExecution(
  session: DapSessionLike,
  threadId: number,
): Promise<unknown> {
  const response = await threadRequest(session, "continue", threadId);
  return response;
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
      .map(async (scope) => {
        const group = await variables(session, scope.variablesReference);
        return group.variables;
      }),
  );
  return groups.flat();
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

/** The first thread, in order, whose stack has at least one frame. */
async function firstStopped(
  session: DapSessionLike,
  candidates: readonly DapThread[],
): Promise<DapStop | undefined> {
  const [thread, ...rest] = candidates;
  if (thread === undefined) {
    return undefined;
  }
  const stack = await stackTraceQuietly(session, thread.id);
  if (stack !== undefined && stack.stackFrames.length > 0) {
    return { threadId: thread.id, stack };
  }
  return firstStopped(session, rest);
}

/** One poll: look for a stopped thread among every thread the adapter knows. */
async function probeForStop(session: DapSessionLike): Promise<DapStop | undefined> {
  return firstStopped(session, await threadsQuietly(session));
}

/**
 * Poll until the debuggee is stopped with at least one stack frame, returning
 * the stopped thread and its stack. Resolves the first thread that reports a
 * frame. Throws a labelled error on timeout.
 */
export async function waitForStop(
  session: DapSessionLike,
  timeoutMs = DEFAULT_STOP_TIMEOUT_MS,
): Promise<DapStop> {
  const probe = (): PromiseLike<DapStop | undefined> => probeForStop(session);
  const stop = await pollUntilResult({
    fn: probe,
    predicate: (value) => value !== undefined,
    timeoutMs,
    intervalMs: STOP_POLL_INTERVAL_MS,
  });
  if (stop === undefined) {
    throw new Error(`Timed out after ${timeoutMs}ms waiting for the debuggee to stop`);
  }
  return stop;
}

async function threadsQuietly(session: DapSessionLike): Promise<DapThread[]> {
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
