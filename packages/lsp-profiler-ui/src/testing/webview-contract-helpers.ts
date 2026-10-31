// Implements [PROF-TESTING]. One fixture and assertion contract for every real host.
import * as assert from 'node:assert';
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

export interface PollOptions<Value> {
  readonly fn: () => Promise<Value> | Value;
  readonly until: (value: Value) => boolean;
  readonly timeoutMs: number;
  readonly intervalMs?: number;
  readonly description?: string;
}

/** Poll until `until` accepts the value; the two-argument form uses a 10s budget. */
export async function pollUntilResult<Value>(options: PollOptions<Value>): Promise<Value>;

export async function pollUntilResult<Value>(
  fn: () => Promise<Value> | Value,
  until: (value: Value) => boolean,
): Promise<Value>;

export async function pollUntilResult<Value>(
  first: PollOptions<Value> | (() => Promise<Value> | Value),
  until?: (value: Value) => boolean,
): Promise<Value> {
  const options: PollOptions<Value> = typeof first === 'function'
    ? { fn: first, until: until ?? (() => true), timeoutMs: DEFAULT_POLL_TIMEOUT_MS }
    : first;
  return pollFor(options, Date.now() + options.timeoutMs);
}

/** Poll `options.fn` until `options.until` accepts a value or `deadline` passes. */
async function pollFor<Value>(options: PollOptions<Value>, deadline: number): Promise<Value> {
  const last = await options.fn();
  if (options.until(last)) { return last; }
  if (Date.now() >= deadline) {
    throw new Error(`${options.description ?? 'condition'} not met within ${String(options.timeoutMs)}ms (last: ${JSON.stringify(last)})`);
  }
  await delay(options.intervalMs ?? DEFAULT_POLL_INTERVAL_MS);
  return pollFor(options, deadline);
}

export async function closeAllEditors(): Promise<void> {
  await vscode.commands.executeCommand("workbench.action.closeAllEditors");
}

export function removeTestDir(dir: string): void {
  if (dir === '' || !fs.existsSync(dir)) {
    return;
  }
  fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
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
    /script-src 'nonce-[^']+'/u.test(html),
    `${what}: the inline script must be nonce-gated`,
  );
  assert.ok(!html.includes('</script><img'), `${what}: untrusted data must not close the <script>`);
}

// ── Dashboard snapshots ────────────────────────────────────────────

/** Sample collection counts for three heap generations, used by the fixture. */
const GENERATION_0_COLLECTIONS = 700;
const GENERATION_1_COLLECTIONS = 12;
const GENERATION_2_COLLECTIONS = 3;

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
    gcCounts: [GENERATION_0_COLLECTIONS, GENERATION_1_COLLECTIONS, GENERATION_2_COLLECTIONS],
    topAllocations: [{ file: '/app/Program.cs', line: 10, size: 4096, count: 8 }],
    timeline: [],
    heapProfilePath: '',
    ...overrides,
  };
}
