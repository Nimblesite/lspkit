// Implements [PROF-UI-LIFETIME] (docs/specs/LSP-PROFILER-SPEC.md).
/**
 * Append-only per-session records with an eviction boundary: a tracking
 * session can run for days, so a list that grows once per capture keeps only
 * its newest entries.
 */

/** Append `item`, then drop the oldest entries beyond `max`. */
export function appendBounded<Item>(list: Item[], item: Item, max: number): void {
  list.push(item);
  if (list.length > max) {
    list.splice(0, list.length - max);
  }
}
