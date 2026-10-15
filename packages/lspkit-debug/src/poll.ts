// Async polling helper shared by every LspKit debugger integration test.
//
// Two call shapes are accepted so the same primitive drops into either house
// style without a shim:
//   - object form  : pollUntilResult({ fn, predicate, timeoutMs, intervalMs })
//   - positional   : pollUntilResult(fn, predicate, timeoutMs, intervalMs)
//
// It resolves with the LAST observed value (it never throws on timeout) so the
// caller can assert on what was actually seen and produce a useful message.

/** Options object form of {@link pollUntilResult}. */
export interface PollOptions<T> {
  /** The probe to run each interval. */
  fn: () => PromiseLike<T>;
  /** Returns true once the probed value is acceptable. */
  predicate: (result: T) => boolean;
  /** Total budget in milliseconds (default 15000). */
  timeoutMs?: number;
  /** Delay between probes in milliseconds (default 100). */
  intervalMs?: number;
}

export const DEFAULT_POLL_TIMEOUT_MS = 15_000;
export const DEFAULT_POLL_INTERVAL_MS = 100;

/** Resolve after `ms` milliseconds. */
export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function normalizePollArgs<T>(
  optionsOrFn: PollOptions<T> | (() => PromiseLike<T>),
  predicate?: (result: T) => boolean,
  timeoutMs?: number,
  intervalMs?: number,
): Required<PollOptions<T>> {
  if (typeof optionsOrFn === "function") {
    return {
      fn: optionsOrFn,
      predicate: predicate ?? (() => true),
      timeoutMs: timeoutMs ?? DEFAULT_POLL_TIMEOUT_MS,
      intervalMs: intervalMs ?? DEFAULT_POLL_INTERVAL_MS,
    };
  }
  return {
    fn: optionsOrFn.fn,
    predicate: optionsOrFn.predicate,
    timeoutMs: optionsOrFn.timeoutMs ?? DEFAULT_POLL_TIMEOUT_MS,
    intervalMs: optionsOrFn.intervalMs ?? DEFAULT_POLL_INTERVAL_MS,
  };
}

/**
 * Poll `fn` until `predicate` accepts its result or the timeout elapses,
 * returning the last observed value either way.
 */
export async function pollUntilResult<T>(
  optionsOrFn: PollOptions<T> | (() => PromiseLike<T>),
  predicate?: (result: T) => boolean,
  timeoutMs?: number,
  intervalMs?: number,
): Promise<T> {
  const opts = normalizePollArgs(optionsOrFn, predicate, timeoutMs, intervalMs);
  const deadline = Date.now() + opts.timeoutMs;
  let last = await opts.fn();
  while (!opts.predicate(last) && Date.now() < deadline) {
    await sleep(opts.intervalMs);
    last = await opts.fn();
  }
  return last;
}

/**
 * Like {@link pollUntilResult} but THROWS a labelled error if the predicate is
 * never satisfied. Use when a missing condition must fail the test loudly.
 */
export async function waitForCondition<T>(
  label: string,
  optionsOrFn: PollOptions<T> | (() => PromiseLike<T>),
  predicate?: (result: T) => boolean,
  timeoutMs?: number,
  intervalMs?: number,
): Promise<T> {
  const opts = normalizePollArgs(optionsOrFn, predicate, timeoutMs, intervalMs);
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
