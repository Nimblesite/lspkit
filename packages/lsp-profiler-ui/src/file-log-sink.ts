// Implements [PROF-UI-HOST]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The filesystem-backed log sink.
 *
 * A long-lived append-only sink, so it holds one open file rather than
 * reopening the file per line. Files are created with owner-only permissions
 * because the log can contain workspace paths (defense-in-depth for
 * js/insecure-temporary-file).
 *
 * [MEM-060-LOG-SINK] A language server runs for days, so the log is capped:
 * past `maxBytes` it continues in the other of two files (`<path>`,
 * `<path>.1`), truncating it, so the newest output is always on disk and the
 * total stays under twice the cap. Writes run in order through one queue, so a
 * rotation closes the old file before it truncates the other. `dispose`
 * closes the file.
 */

import type { FileHandle } from "node:fs/promises";
import { open } from "node:fs/promises";
import type { LogSink } from "./logger";

/**
 * Log file permission bits: owner read+write only. The log can contain
 * workspace paths, so it must never be group/world-readable
 * (defense-in-depth for js/insecure-temporary-file).
 */
const LOG_FILE_MODE = 0o600;
/** Bytes in a mebibyte. */
const BYTES_PER_MIB = 1_048_576;
/** Mebibytes one log file holds before the sink continues in the other. */
const MAX_LOG_MIB = 8;
/** Suffix of the second log file. */
const ROTATED_SUFFIX = ".1";

/** Create+truncate `filePath` with owner-only permissions; nothing when it cannot. */
async function openLog(filePath: string): Promise<FileHandle | undefined> {
  try {
    return await open(filePath, "w", LOG_FILE_MODE);
  } catch {
    return undefined;
  }
}

/** Sink that appends to a size-capped pair of files on disk. */
export class FileLogSink implements LogSink {
  private readonly paths: readonly [string, string];
  private readonly maxBytes: number;
  /** The file after every queued write, in order. */
  private queue: Promise<FileHandle | undefined>;
  private active = 0;
  private written = 0;
  private disposed = false;
  public constructor(filePath: string, maxBytes = MAX_LOG_MIB * BYTES_PER_MIB) {
    // Callers pass the extension's private logUri; the owner-only mode keeps
    // the log private even if one ever points this at a shared directory.
    this.paths = [filePath, `${filePath}${ROTATED_SUFFIX}`];
    this.maxBytes = maxBytes;
    this.queue = openLog(filePath);
  }
  public trace(message: string): void { this.write("TRACE", message); }
  public debug(message: string): void { this.write("DEBUG", message); }
  public info(message: string): void { this.write("INFO ", message); }
  public warn(message: string): void { this.write("WARN ", message); }
  public error(message: string): void { this.write("ERROR", message); }
  /** Close the log file once every queued line is written; later lines are dropped. */
  public async dispose(): Promise<void> {
    this.disposed = true;
    this.queue = closeAfter(this.queue);
    await this.queue;
  }
  private write(level: string, message: string): void {
    if (this.disposed) { return; }
    const line = `${new Date().toISOString()} [${level}] ${message}\n`;
    const bytes = Buffer.byteLength(line);
    const rotate = this.written > 0 && this.written + bytes > this.maxBytes;
    if (rotate) {
      this.active = 1 - this.active;
      this.written = 0;
    }
    this.written += bytes;
    this.queue = append(this.queue, line, rotate ? this.paths[this.active] : undefined);
  }
}

/** After `previous`, optionally move to `reopenAt`, then write `line`. */
async function append(
  previous: Promise<FileHandle | undefined>,
  line: string,
  reopenAt: string | undefined,
): Promise<FileHandle | undefined> {
  const handle = await previous;
  const target = reopenAt === undefined ? handle : await reopen(handle, reopenAt);
  try {
    await target?.write(line);
  } catch {
    // A full or vanished disk drops the line; the sink has nowhere to report it.
  }
  return target;
}

/** Close `handle`, then create+truncate `filePath`. */
async function reopen(handle: FileHandle | undefined, filePath: string): Promise<FileHandle | undefined> {
  await closeQuietly(handle);
  return openLog(filePath);
}

/** After `previous`, close the file. */
async function closeAfter(previous: Promise<FileHandle | undefined>): Promise<undefined> {
  await closeQuietly(await previous);
  return undefined;
}

/** Close `handle`, ignoring an already closed one. */
async function closeQuietly(handle: FileHandle | undefined): Promise<void> {
  try {
    await handle?.close();
  } catch {
    // Already closed: nothing left to release.
  }
}
