/**
 * What memory tracking does next, said in the terms of the runtime it tracks.
 *
 * A courier runtime is captured at each pause; a direct one never is, since a
 * heap the debugger holds paused cannot be read ([PROF-MEMORY-MODE]). Telling a
 * .NET user to "press Continue to auto-capture each pause" sent them looking
 * for captures that never came.
 *
 * Implements [PROF-UI-AUTOPILOT].
 */

import { brand } from "./ids";

/** A courier runtime's next steps: each pause is captured, and so is the exit. */
const COURIER_NEXT =
  "Press Continue to auto-capture each pause, or let the program finish for an automatic final snapshot.";

/** The toast tracking opens with; `intervalSecs` while interval capture is on. */
export function trackingGuidance(direct: boolean, intervalSecs: number | undefined): string {
  return `${brand()}: Memory tracking started. ${direct ? directNext(intervalSecs) : COURIER_NEXT}`;
}

/** A direct runtime's next steps: read while it runs, never while paused. */
function directNext(intervalSecs: number | undefined): string {
  const reads = intervalSecs === undefined
    ? "Take a snapshot while it runs"
    : `It is captured every ${String(intervalSecs)} s while it runs`;
  return `${reads}, never while paused; when it ends, its last snapshot opens.`;
}

/** The toast when tracking stops before any snapshot: how to get one next time. */
export function stoppedWithoutSnapshot(direct: boolean): string {
  const next = direct
    ? "Take a snapshot while the program runs, never while paused"
    : "Take a snapshot while paused, or let the program finish";
  return `${brand()}: Memory tracking stopped — no snapshot was taken. ${next}, to inspect allocations.`;
}
