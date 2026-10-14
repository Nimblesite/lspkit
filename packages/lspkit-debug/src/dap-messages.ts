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

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : undefined;
}

/** Compact one-line summary of a DAP request's arguments for tracing. */
export function summarizeArgs(args: unknown): string {
  const record = asRecord(args);
  if (!record) {
    return "";
  }
  const parts: string[] = [];
  for (const key of ["threadId", "frameId", "variablesReference", "expression"]) {
    if (record[key] !== undefined) {
      parts.push(`${key}=${String(record[key])}`);
    }
  }
  if (Array.isArray(record.breakpoints)) {
    parts.push(`breakpoints=${record.breakpoints.length}`);
  }
  return parts.join(" ");
}

/** Compact one-line summary of a DAP response/event body for tracing. */
export function summarizeBody(body: unknown): string {
  const record = asRecord(body);
  if (!record) {
    return "";
  }
  const parts: string[] = [];
  if (typeof record.reason === "string") {
    parts.push(`reason=${record.reason}`);
  }
  if (record.threadId !== undefined) {
    parts.push(`threadId=${String(record.threadId)}`);
  }
  for (const key of ["stackFrames", "scopes", "variables", "threads"]) {
    if (Array.isArray(record[key])) {
      parts.push(`${key}=${(record[key] as unknown[]).length}`);
    }
  }
  if (typeof record.result === "string") {
    parts.push(`result=${record.result}`);
  }
  return parts.join(" ");
}
