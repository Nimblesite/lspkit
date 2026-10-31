// Implements [PROF-UI-HOST]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The filesystem-backed log sink.
 *
 * A long-lived append-only sink, so it writes through a `WriteStream` rather
 * than reopening the file per line. The stream is created with owner-only
 * permissions because the log can contain workspace paths (defense-in-depth
 * for js/insecure-temporary-file).
 */

import * as fs from "node:fs";
import type { LogSink } from "./logger";

/**
 * Log file permission bits: owner read+write only. The log can contain
 * workspace paths, so it must never be group/world-readable
 * (defense-in-depth for js/insecure-temporary-file).
 */
const LOG_FILE_MODE = 0o600;

/** Sink that appends to a file on disk. */
export class FileLogSink implements LogSink {
  private readonly stream: fs.WriteStream;
  public constructor(filePath: string) {
    // Create+truncate with OWNER-ONLY (0o600) permissions so the log — which
    // can contain workspace paths — is never world-readable, even if a caller
    // ever points this at a shared directory (defense-in-depth for
    // js/insecure-temporary-file; callers pass the extension's private logUri).
    this.stream = fs.createWriteStream(filePath, { flags: "w", mode: LOG_FILE_MODE });
  }
  public trace(message: string): void { this.write("TRACE", message); }
  public debug(message: string): void { this.write("DEBUG", message); }
  public info(message: string): void { this.write("INFO ", message); }
  public warn(message: string): void { this.write("WARN ", message); }
  public error(message: string): void { this.write("ERROR", message); }
  private write(level: string, message: string): void {
    const timestamp = new Date().toISOString();
    this.stream.write(`${timestamp} [${level}] ${message}\n`);
  }
}
