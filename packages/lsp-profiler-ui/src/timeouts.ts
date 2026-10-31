// Implements [PROF-UI-HOST]. See docs/specs/LSP-PROFILER-SPEC.md
import { setTimeout as delayTimer } from "node:timers/promises";

/**
 * Canonical timeout constants for the lsp-profiler UI library.
 *
 * Three knobs. No others. Anything that feels like it needs a fourth is a
 * design smell — fix the underlying slowness, do not invent a new bucket.
 */

/** Interval between polls. */
export const POLL_INTERVAL_MS = 10;

/** Max time to wait for a single command/event to settle at runtime.
 *  If a wait exceeds this, the operation is broken. */
export const WAIT_MS = 1000;

/** Startup / cold-init timeout. Anything slower than this is a bug. */
export const STARTUP_TIMEOUT_MS = 10_000;

/**
 * Resolve after `ms` milliseconds.
 *
 * The one place a delay is scheduled, so a timing change happens once rather
 * than in fifty copies. `node:timers/promises` wraps the timer without a
 * hand-rolled `new Promise` whose executor's return value would be discarded.
 */
export async function delay(ms: number): Promise<void> {
  await delayTimer(ms);
}
