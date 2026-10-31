// Async polling helper shared by every LspKit debugger integration test.
//
// Two call shapes are accepted so the same primitive drops into either house
// style without a shim:
//   - object form  : pollUntilResult({ fn, predicate, timeoutMs, intervalMs })
//   - positional   : pollUntilResult(fn, predicate, timeoutMs, intervalMs)
//
// It resolves with the LAST observed value (it never throws on timeout) so the
// caller can assert on what was actually seen and produce a useful message.

import { setTimeout as delay } from "node:timers/promises";

/** Options object form of {@link pollUntilResult}. */
export interface PollOptions<TResult> {
  /** The probe to run each interval. */
  fn: () => PromiseLike<TResult>;
  /** Returns true once the probed value is acceptable. */
  predicate: (result: TResult) => boolean;
  /** Total budget in milliseconds (default 15000). */
  timeoutMs?: number;
  /** Delay between probes in milliseconds (default 100). */
  intervalMs?: number;
}

/** The positional call shape, carried as one value to keep helpers small. */
interface PollArgs<TResult> {
  /** The options object, or the probe itself for the positional shape. */
  optionsOrFn: PollOptions<TResult> | (() => PromiseLike<TResult>);
  /** Predicate from the positional shape. */
  predicate?: (result: TResult) => boolean;
  /** Timeout from the positional shape. */
  timeoutMs?: number;
  /** Interval from the positional shape. */
  intervalMs?: number;
}

export const DEFAULT_POLL_TIMEOUT_MS = 15_000;
export const DEFAULT_POLL_INTERVAL_MS = 100;

/** The default predicate: accept the first value seen. */
function alwaysAccept(): boolean {
  return true;
}

/** Resolve after `milliseconds`. */
export async function sleep(milliseconds: number): Promise<void> {
  await delay(milliseconds);
}

function normalizePollArgs<TResult>(
  args: PollArgs<TResult>,
): Required<PollOptions<TResult>> {
  if (typeof args.optionsOrFn === "function") {
    return {
      fn: args.optionsOrFn,
      predicate: args.predicate ?? alwaysAccept,
      timeoutMs: args.timeoutMs ?? DEFAULT_POLL_TIMEOUT_MS,
      intervalMs: args.intervalMs ?? DEFAULT_POLL_INTERVAL_MS,
    };
  }
  return {
    fn: args.optionsOrFn.fn,
    predicate: args.optionsOrFn.predicate,
    timeoutMs: args.optionsOrFn.timeoutMs ?? DEFAULT_POLL_TIMEOUT_MS,
    intervalMs: args.optionsOrFn.intervalMs ?? DEFAULT_POLL_INTERVAL_MS,
  };
}

/** Sleep one interval, re-probe, and continue polling. */
async function pollAgain<TResult>(
  opts: Required<PollOptions<TResult>>,
  deadline: number,
  last: TResult,
): Promise<TResult> {
  if (opts.predicate(last) || Date.now() >= deadline) {
    return last;
  }
  await sleep(opts.intervalMs);
  const next = await opts.fn();
  return pollAgain(opts, deadline, next);
}

/**
 * Poll `fn` until `predicate` accepts its result or the timeout elapses,
 * returning the last observed value either way.
 */
export async function pollUntilResult<TResult>(
  optionsOrFn: PollOptions<TResult> | (() => PromiseLike<TResult>),
  predicate?: (result: TResult) => boolean,
  timeoutMs?: number,
  intervalMs?: number,
): Promise<TResult> {
  const opts = normalizePollArgs({ optionsOrFn, predicate, timeoutMs, intervalMs });
  const first = await opts.fn();
  return pollAgain(opts, Date.now() + opts.timeoutMs, first);
}

/**
 * Like {@link pollUntilResult} but THROWS a labelled error if the predicate is
 * never satisfied. Use when a missing condition must fail the test loudly.
 */
export async function waitForCondition<TResult>(
  label: string,
  optionsOrFn: PollOptions<TResult> | (() => PromiseLike<TResult>),
  predicate?: (result: TResult) => boolean,
  timeoutMs?: number,
  intervalMs?: number,
): Promise<TResult> {
  const opts = normalizePollArgs({ optionsOrFn, predicate, timeoutMs, intervalMs });
  const last = await pollUntilResult(opts);
  if (!opts.predicate(last)) {
    throw new Error(
      `Timed out after ${opts.timeoutMs}ms waiting for ${label}; last value: ${safeStringify(last)}`,
    );
  }
  return last;
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}
