// Implements [PROF-UI-HOST]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Logging abstraction for the lsp-profiler UI library.
 *
 * Provides a backend-agnostic `Logger` interface so callers never depend on
 * VS Code APIs or any concrete sink; only the `FileLogSink` in this module's
 * sibling touches the filesystem. The active sink is stored in the centralized
 * Store — no global mutable state lives in this module.
 */

export { FileLogSink } from "./file-log-sink";

// ── Public interface ─────────────────────────────────────────────────────

export enum LogLevel {
  Trace = 0,
  Debug = 1,
  Info = 2,
  Warn = 3,
  Error = 4,
}

/** Backend-agnostic logging interface. */
export interface LogSink {
  trace: (message: string) => void;
  debug: (message: string) => void;
  info: (message: string) => void;
  warn: (message: string) => void;
  error: (message: string) => void;
}

// ── Built-in sinks ──────────────────────────────────────────────────────

/** Sink that silently discards all messages. */
export const nullSink: LogSink = {
  trace(): void {
    // Noop.
  },
  debug(): void {
    // Noop.
  },
  info(): void {
    // Noop.
  },
  warn(): void {
    // Noop.
  },
  error(): void {
    // Noop.
  },
};

/** Sink that fans out to multiple backends. */
export class CompositeSink implements LogSink {
  private readonly sinks: LogSink[];
  public constructor(sinks: LogSink[]) {
    this.sinks = sinks;
  }
  public trace(message: string): void { for (const sink of this.sinks) {sink.trace(message);} }
  public debug(message: string): void { for (const sink of this.sinks) {sink.debug(message);} }
  public info(message: string): void { for (const sink of this.sinks) {sink.info(message);} }
  public warn(message: string): void { for (const sink of this.sinks) {sink.warn(message);} }
  public error(message: string): void { for (const sink of this.sinks) {sink.error(message);} }
}

// ── Logger with injectable sink ─────────────────────────────────────────

/** Sink accessor — set once at startup by extension.ts via bindLogger(). */
 
let sinkAccessor: () => LogSink = () => nullSink;

/**
 * Bind the Logger to a sink accessor function. Called once during
 * activate() so the Logger reads from the store without importing it
 * (avoiding circular deps).
 */
export function bindLogger(accessor: () => LogSink): void {
  sinkAccessor = accessor;
}

/** The global logger. Always safe to call — defaults to a no-op sink. */
export const Logger = {
  trace(message: string): void { sinkAccessor().trace(message); },
  debug(message: string): void { sinkAccessor().debug(message); },
  info(message: string): void { sinkAccessor().info(message); },
  warn(message: string): void { sinkAccessor().warn(message); },
  error(message: string): void { sinkAccessor().error(message); },
} as const;
