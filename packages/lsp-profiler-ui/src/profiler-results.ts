// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The last CPU result the UI presented, and how it reaches the user.
 */

import * as vscode from "vscode";
import { brand } from "./ids";
import { Logger } from "./logger";
import type { ProfileResult } from "./profiler-decorations";
import { applyProfileDecorations } from "./profiler-decorations";
import { openFlamegraphWebview, presentProfileResult } from "./profiler-flamegraph-html";

const lastProfile: { result: ProfileResult | undefined } = { result: undefined };

/** The most recent CPU result the UI presented (e2e seam). */
export function lastProfileResult(): ProfileResult | undefined {
  return lastProfile.result;
}

/** Forget the last result (extension teardown). */
export function clearLastProfileResult(): void {
  lastProfile.result = undefined;
}

/** Keep `result` as the last one and paint its heat on the editors. */
function keepResult(result: ProfileResult): void {
  lastProfile.result = result;
  applyProfileDecorations(result);
}

/** Keep and present a stop result, when the server returned one. */
export function presentStopResult(result: ProfileResult | undefined): void {
  if (result === undefined) { return; }
  keepResult(result);
  // Land the user on the self-contained results panel, beside the
  // heat-mapped source; the raw `.cpuprofile` stays one click away
  // ([PROF-CPU-START], #145).
  presentProfileResult(result);
}

/**
 * Keep a snapshot's result and offer its panel. Fire-and-forget: a toast with
 * a button is sticky and must not block the snapshot handler ([PROF-CPU-START]).
 */
export function announceSnapshot(result: ProfileResult): void {
  keepResult(result);
  Logger.info(`Profile snapshot: ${String(result.totalSamples)} samples so far`);
  void offerSnapshotView(result);
}

/** Offer the snapshot's results panel and open it when the user accepts. */
async function offerSnapshotView(result: ProfileResult): Promise<void> {
  const choice = await vscode.window.showInformationMessage(
    `${brand()}: Snapshot — ${String(result.totalSamples)} samples (profiling continues)`,
    "View Results",
  );
  if (choice === "View Results") { openFlamegraphWebview(result); }
}

/**
 * Re-open the results panel for the most recent profile. The panel is a
 * singleton the user can close; this palette command is the always-available
 * way back in — results are never trapped behind a dismissed completion toast
 * ([PROF-CPU-START]).
 */
export function handleProfileShowResults(): void {
  if (lastProfile.result === undefined) {
    vscode.window.showInformationMessage(
      `${brand()}: No profile results yet — run a profiling session first.`,
    );
    return;
  }
  openFlamegraphWebview(lastProfile.result);
}
