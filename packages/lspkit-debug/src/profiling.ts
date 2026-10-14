// Profiling / memory-tracking extension points.
//
// A top-tier debugger leaves room to run the SAME launch as a profiling or
// memory-tracking session. These pure helpers (modelled on Basilisk's
// profileOnLaunch / memoryTrackOnLaunch / suppressBreakpointsForProfiling) let
// an editor add CPU/memory modes without re-plumbing the adapter: a profiling
// launch reuses the debug build but suppresses breakpoints so sampling is not
// perturbed.

import type { DapMessage } from "./dap-messages";
import type { DebugConfigLike } from "./launch-config";

/** Which extra signal a launch should collect, beyond plain debugging. */
export type LaunchMetric = "debug" | "cpu" | "memory";

/** Launch flags that select a profiling/tracking mode. */
export interface ProfilingFlags {
  /** Collect a CPU profile across this run. */
  profileOnLaunch?: boolean;
  /** Track allocations across this run (usually implies stopOnEntry). */
  memoryTrackOnLaunch?: boolean;
}

/** True when the config requests any profiling/tracking mode. */
export function isProfilingLaunch(config: ProfilingFlags): boolean {
  return Boolean(config.profileOnLaunch) || Boolean(config.memoryTrackOnLaunch);
}

/** The metric a config selects (memory takes precedence over cpu). */
export function launchMetric(config: ProfilingFlags): LaunchMetric {
  if (config.memoryTrackOnLaunch) {
    return "memory";
  }
  if (config.profileOnLaunch) {
    return "cpu";
  }
  return "debug";
}

/**
 * Strip breakpoint-setting requests when a profiling launch is in flight, so the
 * profiler/sampler runs uninterrupted. Non-breakpoint messages and ordinary
 * (non-profiling) launches pass through unchanged. Pure: returns a new message,
 * never mutates the input.
 */
export function suppressBreakpointsForProfiling(
  msg: DapMessage,
  profilingLaunch: boolean,
): DapMessage {
  if (!profilingLaunch || msg.type !== "request") {
    return msg;
  }
  switch (msg.command) {
    case "setBreakpoints":
    case "setFunctionBreakpoints":
      return { ...msg, arguments: { ...msg.arguments, breakpoints: [] } };
    case "setExceptionBreakpoints":
      return {
        ...msg,
        arguments: { ...msg.arguments, filters: [], filterOptions: [], exceptionOptions: [] },
      };
    default:
      return msg;
  }
}

/**
 * Derive a profiling/memory launch config from a base debug config. Memory
 * tracking forces stopOnEntry (an allocator must attach before user code runs);
 * `darwinCpuStopOnEntry` opts CPU runs into stop-on-entry on macOS, where a
 * cooperative sampler needs the paused debuggee first.
 */
export function buildMetricLaunchConfig(
  base: DebugConfigLike,
  metric: LaunchMetric,
  options: { platform: NodeJS.Platform; darwinCpuStopOnEntry?: boolean } = {
    platform: "linux",
  },
): DebugConfigLike {
  if (metric === "memory") {
    return { ...base, memoryTrackOnLaunch: true, stopOnEntry: true };
  }
  if (metric === "cpu") {
    const stopOnEntry =
      options.platform === "darwin" && options.darwinCpuStopOnEntry === true;
    return stopOnEntry
      ? { ...base, profileOnLaunch: true, stopOnEntry: true }
      : { ...base, profileOnLaunch: true };
  }
  return { ...base };
}
