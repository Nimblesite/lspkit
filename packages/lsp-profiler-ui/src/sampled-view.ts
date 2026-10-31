// Implements [PROF-UI-SAMPLED]. Shared interactive view for every sampled runtime.
import { readFileSync } from "node:fs";
import type { FlameModel } from "./flame-model";
import { buildFlameModel, parseSpeedscope } from "./flame-model";
import { FLAME_CSS, flameBody } from "./flame-html";
import { FLAME_SCRIPT } from "./flame-script";
import { paletteFrames, profilerPalette } from "./palette";
import type { ProfileSummary } from "./sample-summary";
import type { ProfileResult } from "./profiler-decorations";
import { Logger } from "./logger";

export interface SampledView { readonly model: FlameModel; readonly summary: ProfileSummary; }

/** Load the existing speedscope artifact; metadata from the native host stays intact. */
export function sampledView(result: ProfileResult): SampledView | undefined {
  if (result.sampled !== undefined) { return result.sampled; }
  if (result.outputFile === "") { return undefined; }
  return sampledViewFromFile(result);
}

/** Build the sampled view from the existing speedscope artifact on disk. */
function sampledViewFromFile(result: ProfileResult): SampledView | undefined {
  try {
    const parsed = parseSpeedscope(readFileSync(result.outputFile, "utf8"));
    if (!parsed.ok) { Logger.warn(parsed.error); return undefined; }
    const model = buildFlameModel(parsed.value);
    return { model, summary: summaryFromResult(result, model) };
  } catch (error) {
    Logger.warn(`Sampled view unavailable: ${String(error)}`);
    return undefined;
  }
}

function summaryFromResult(result: ProfileResult, model: FlameModel): ProfileSummary {
  return {
    schedulingKnown: false, version: 1, program: "", wallSeconds: result.duration, cpuSeconds: 0,
    sampleCount: result.totalSamples, rateHz: 0, droppedSamples: 0,
    fibers: model.fibers.map((fiber, id) => ({ id, label: fiber.name,
      samples: fiber.sampleCount, oncpuSamples: 0 })),
    hotFunctions: [], hotLines: [],
  };
}

/** One renderer and interaction script, shared with native host imports. */
export function sampledParts(view: SampledView | undefined): { css: string; body: string; script: string } {
  if (view === undefined) { return { css: "", body: "", script: "" }; }
  const palette = profilerPalette();
  const model = { ...view.model, frames: paletteFrames(view.model.frames, palette) };
  return { css: FLAME_CSS, body: flameBody(model, view.summary), script: FLAME_SCRIPT };
}
