// Implements [PROF-TESTING]. One fixture and assertion contract for every real host.
import * as assert from 'assert';
import * as fs from 'node:fs';
import * as vscode from 'vscode';
import { delay } from '../timeouts';
import type { ProfileResult } from '../profiler-decorations';
import type { MemoryDashboardSnapshot } from '../memory-dashboard';
const DEFAULT_POLL_INTERVAL_MS = 100;
/** The budget of the two-argument `pollUntilResult`. */
const DEFAULT_POLL_TIMEOUT_MS = 10_000;

/** A payload that closes the surrounding <script> if embedding is unescaped. */
export const HOSTILE = '</script><img src=x onerror=alert(1)>';

export interface PollOptions<T> {
  readonly fn: () => Promise<T> | T;
  readonly until: (value: T) => boolean;
  readonly timeoutMs: number;
  readonly intervalMs?: number;
  readonly description?: string;
}

/** Poll until `until` accepts the value; the two-argument form uses a 10s budget. */
export async function pollUntilResult<T>(options: PollOptions<T>): Promise<T>;

export async function pollUntilResult<T>(
  fn: () => Promise<T> | T,
  until: (value: T) => boolean,
): Promise<T>;

export async function pollUntilResult<T>(
  first: PollOptions<T> | (() => Promise<T> | T),
  until?: (value: T) => boolean,
): Promise<T> {
  const options: PollOptions<T> = typeof first === 'function'
    ? { fn: first, until: until ?? (() => true), timeoutMs: DEFAULT_POLL_TIMEOUT_MS }
    : first;
  const deadline = Date.now() + options.timeoutMs;
  let last: T = await options.fn();
  while (!options.until(last)) {
    if (Date.now() >= deadline) {
      throw new Error(`${options.description ?? 'condition'} not met within ${options.timeoutMs}ms (last: ${JSON.stringify(last)})`);
    }
    await delay(options.intervalMs ?? DEFAULT_POLL_INTERVAL_MS);
    last = await options.fn();
  }
  return last;
}

export async function closeAllEditors(): Promise<void> {
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
}

export function removeTestDir(dir: string): void {
  if (dir === '' || !fs.existsSync(dir)) {
    return;
  }
  fs.rmSync(dir, { recursive: true, force: true });
}

/** A CPU result, overriding the fields a test cares about. */
export function profileResult(overrides: Partial<ProfileResult> = {}): ProfileResult {
  return {
    sessionId: 's-1',
    duration: 2.5,
    totalSamples: 250,
    outputFile: '/tmp/profile.speedscope.json',
    hotFunctions: [],
    hotLines: [],
    ...overrides,
  };
}

/** Assert `html` contains every key (or, with `present` false, none), failing with its reason. */
export function assertHtml(html: string, checks: Record<string, string>, present = true): void {
  for (const [needle, why] of Object.entries(checks)) {
    assert.equal(html.includes(needle), present, why);
  }
}

/** Assert a webview declares a CSP, nonce-gates its script, and kept hostile data inert. */
export function assertHardened(html: string, what: string): void {
  assert.ok(html.includes('Content-Security-Policy'), `${what} must declare a CSP`);
  assert.ok(
    /script-src 'nonce-[^']+'/.test(html),
    `${what}: the inline script must be nonce-gated`,
  );
  assert.ok(!html.includes('</script><img'), `${what}: untrusted data must not close the <script>`);
}

// ── Dashboard snapshots ────────────────────────────────────────────

/** A minimal dashboard snapshot (fresh session: one capture, no diff yet). */
export function dashboardSnapshot(
  overrides: Partial<MemoryDashboardSnapshot> = {},
): MemoryDashboardSnapshot {
  return {
    memorySessionId: 'mem-1',
    snapshotId: 'snap-1',
    currentMemory: 1_048_576,
    peakMemory: 2_097_152,
    gcObjects: 1200,
    gcCounts: [700, 12, 3],
    topAllocations: [{ file: '/app/Program.cs', line: 10, size: 4096, count: 8 }],
    timeline: [],
    heapProfilePath: '',
    ...overrides,
  };
}