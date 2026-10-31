// Implements [PROF-UI-CPU]. Runtime-provided sampled profile metadata.
export interface FiberInfo {
  id: number;
  label: string;
  samples: number;
  oncpuSamples: number;
}

export interface HotFunctionInfo {
  name: string;
  file: string;
  line: number;
  selfPct: number;
  totalPct: number;
  selfSamples: number;
  totalSamples: number;
  kind: string;
}

export interface HotLineInfo {
  file: string;
  line: number;
  pct: number;
  samples: number;
}

export interface ProfileSummary {
  schedulingKnown?: boolean;
  version: number;
  program: string;
  wallSeconds: number;
  cpuSeconds: number;
  sampleCount: number;
  rateHz: number;
  droppedSamples: number;
  fibers: FiberInfo[];
  hotFunctions: HotFunctionInfo[];
  hotLines: HotLineInfo[];
}

const SECONDS_SMALL_LIMIT = 0.1;
const MS_PER_SECOND = 1000;
const SECONDS_LARGE_LIMIT = 100;
const PERCENT = 100;

function formatSeconds(seconds: number): string {
  if (seconds > 0 && seconds < SECONDS_SMALL_LIMIT) {
    return `${(seconds * MS_PER_SECOND).toFixed(1)}ms`;
  }
  return `${seconds >= SECONDS_LARGE_LIMIT ? seconds.toFixed(0) : seconds.toFixed(1)}s`;
}

/** `4182 samples · 997Hz · 4.2s wall · 3.9s CPU · 3 fibers` (+ dropped). */
export function formatSummaryHeader(summary: ProfileSummary): string {
  if (summary.schedulingKnown === false) { return `${String(summary.sampleCount)} samples · ${formatSeconds(summary.wallSeconds)} wall · ${String(summary.fibers.length)} threads`; }
  const fibers = `${String(summary.fibers.length)} ${summary.fibers.length === 1 ? "fiber" : "fibers"}`;
  const dropped =
    summary.droppedSamples > 0 ? ` · ${String(summary.droppedSamples)} dropped` : "";
  return (
    `${String(summary.sampleCount)} samples · ${String(summary.rateHz)}Hz · ` +
    `${formatSeconds(summary.wallSeconds)} wall · ${formatSeconds(summary.cpuSeconds)} CPU · ` +
    `${fibers}${dropped}`
  );
}

/** A fiber's on-CPU share as a whole percentage (0 when it never sampled). */
export function onCpuPct(fiber: FiberInfo): number {
  return fiber.samples > 0
    ? Math.round((PERCENT * fiber.oncpuSamples) / fiber.samples)
    : 0;
}
