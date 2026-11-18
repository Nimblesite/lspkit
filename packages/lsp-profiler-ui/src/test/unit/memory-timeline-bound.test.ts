// Tests [MEM-060-UI-LEDGERS]: per-capture state stays bounded within one long
// tracking session. Plain mocha: the module under test imports no `vscode`.
import * as assert from "node:assert";
import { currentTimeline, recordTimelinePoint, resetCaptureState } from "../../memory-capture-state";
import type { MemoryDashboardSnapshot } from "../../memory-dashboard";

/** Captures in one session: an interval capture every 30 s for well over an hour. */
const CAPTURES = 500;
/** The core's timeline bound ([PROF-MEMORY-TIMELINE] `MAX_POINTS`). */
const CORE_MAX_POINTS = 120;

function snapshot(index: number): MemoryDashboardSnapshot {
  return {
    memorySessionId: "mem-1",
    snapshotId: `snap-${String(index)}`,
    currentMemory: index,
    peakMemory: index,
    gcObjects: index,
    gcCounts: [],
    topAllocations: [],
    timeline: [],
    heapProfilePath: "",
  };
}

suite("Memory timeline stays bounded across a long session", () => {
  teardown(() => { resetCaptureState(); });

  test("the timeline keeps at most the core's points, newest last", () => {
    resetCaptureState();
    for (let index = 0; index < CAPTURES; index += 1) {
      recordTimelinePoint(snapshot(index));
    }
    const timeline = currentTimeline();
    assert.ok(
      timeline.length <= CORE_MAX_POINTS,
      `${String(timeline.length)} timeline points kept for ${String(CAPTURES)} captures`,
    );
    assert.strictEqual(timeline.at(-1)?.currentMemory, CAPTURES - 1, "the newest capture is kept");
  });
});
