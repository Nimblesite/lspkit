// Debug Adapter Protocol wire-message shape plus compact summarizers used for
// human-readable DAP tracing. Language-neutral: every DAP adapter speaks these.

/** A single Debug Adapter Protocol message (request, response, or event). */
export interface DapMessage {
  /** "request" | "response" | "event". */
  type: string;
  seq?: number;
  request_seq?: number;
  /** Request/response command, e.g. "setBreakpoints", "stackTrace". */
  command?: string;
  /** Event name, e.g. "stopped", "continued", "exited". */
  event?: string;
  success?: boolean;
  body?: unknown;
  arguments?: Record<string, unknown>;
}

/** Request-argument fields rendered in order by {@link summarizeArgs}. */
const ARG_KEYS = ["threadId", "frameId", "variablesReference", "expression"] as const;

/** Body fields whose array length is rendered by {@link summarizeBody}. */
const BODY_COUNT_KEYS = ["stackFrames", "scopes", "variables", "threads"] as const;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Render a traced field without relying on Object's default stringification. */
function traceValue(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (
    typeof value === "number" ||
    typeof value === "boolean" ||
    typeof value === "bigint"
  ) {
    return String(value);
  }
  return JSON.stringify(value);
}

function isPresent(part: string | undefined): part is string {
  return part !== undefined;
}

/** `key=value` for one argument field, or undefined when it is absent. */
function argPart(record: Record<string, unknown>, key: string): string | undefined {
  const value = record[key];
  return value === undefined ? undefined : `${key}=${traceValue(value)}`;
}

/** `key=<count>` for one array field, or undefined when it is absent. */
function countPart(record: Record<string, unknown>, key: string): string | undefined {
  const value = record[key];
  return Array.isArray(value) ? `${key}=${value.length}` : undefined;
}

function reasonPart(record: Record<string, unknown>): string | undefined {
  return typeof record.reason === "string" ? `reason=${record.reason}` : undefined;
}

function resultPart(record: Record<string, unknown>): string | undefined {
  return typeof record.result === "string" ? `result=${record.result}` : undefined;
}

/** Compact one-line summary of a DAP request's arguments for tracing. */
export function summarizeArgs(args: unknown): string {
  if (!isRecord(args)) {
    return "";
  }
  return [
    ...ARG_KEYS.map((key) => argPart(args, key)),
    countPart(args, "breakpoints"),
  ]
    .filter((part) => isPresent(part))
    .join(" ");
}

/** Compact one-line summary of a DAP response/event body for tracing. */
export function summarizeBody(body: unknown): string {
  if (!isRecord(body)) {
    return "";
  }
  return [
    reasonPart(body),
    argPart(body, "threadId"),
    ...BODY_COUNT_KEYS.map((key) => countPart(body, key)),
    resultPart(body),
  ]
    .filter((part) => isPresent(part))
    .join(" ");
}
