// Implements [PROF-UI-HOST] wire adaptation. The library renders from its own
// view models; this is the single place that reads the protocol's wire types
// ([PROF-WIRE]) so every screen stays ignorant of field spelling, and a wire
// change touches one file.

import type { ProfileResult } from "./profiler-decorations";
import type { ProcessInfo } from "./process-explorer-rows";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import type {
  HeapDiff,
  HeapSnapshot,
  MemoryOutcome,
  MemoryResult,
  ProcessInfo as WireProcess,
  ProfileReport,
  RefGraph,
  StartProfileResult,
  SuspectedLeak,
  TimelinePoint,
} from "./wire-generated";

/** A started CPU session as the screens track it. */
export interface StartedSession {
  readonly sessionId: string;
  readonly pid: number;
  readonly runtimeVersion: string;
  readonly runtime: string;
}

export function toStartedSession(result: StartProfileResult): StartedSession {
  return {
    sessionId: result.session_id,
    pid: result.pid,
    runtimeVersion: result.runtime_version,
    runtime: result.runtime,
  };
}

export function toProfileResult(report: ProfileReport): ProfileResult {
  const artifacts = report.artifacts;
  return {
    sessionId: report.session_id,
    duration: report.duration_secs,
    totalSamples: report.total_samples,
    outputFile: artifacts.speedscope_path ?? "",
    ...(artifacts.cpuprofile_path !== undefined ? { cpuProfilePath: artifacts.cpuprofile_path } : {}),
    ...(artifacts.flamegraph_path !== undefined ? { flamegraphPath: artifacts.flamegraph_path } : {}),
    ...(artifacts.export_error !== undefined ? { exportError: artifacts.export_error } : {}),
    hotFunctions: report.hot_functions.map((fn) => ({
      name: fn.name,
      ...located(fn),
      samples: fn.samples,
      percentage: fn.percentage,
      selfPercentage: fn.self_percentage,
    })),
    hotLines: report.hot_lines.map((line) => ({
      file: line.file,
      line: line.line,
      samples: line.samples,
      percentage: line.percentage,
    })),
  };
}

export function toUiProcess(process: WireProcess): ProcessInfo {
  return {
    pid: process.pid,
    ppid: process.ppid ?? 0,
    name: process.name,
    runtime: process.runtime,
    interpreterPath: process.executable ?? null,
    script: process.entry ?? null,
    runtimeVersion: process.runtime_version ?? null,
    cpuPercent: process.cpu_percent,
    memoryBytes: process.memory_bytes,
    runtimeSecs: process.runtime_secs,
    user: process.user ?? null,
    requiresElevation: process.requires_elevation,
    inWorkspace: process.in_workspace,
    launcher: process.launcher ?? null,
    debuggable: process.attachable,
    undebuggableReason: process.unattachable_reason ?? null,
  };
}

/** [PROF-WIRE]: a source location is both fields or neither, never `""` and line 0. */
function located(entry: { file?: string | null; line?: number | null }): { file?: string; line?: number } {
  return entry.file != null && entry.line != null ? { file: entry.file, line: entry.line } : {};
}

function allocation(site: { file?: string | null; line?: number | null; size_bytes: number; count: number; type_name?: string | null }): Record<string, unknown> {
  return { ...located(site), size: site.size_bytes, count: site.count, typeName: site.type_name ?? "" };
}

function snapshotFields(snapshot: HeapSnapshot, timeline: TimelinePoint[], heapProfilePath: string | null | undefined): Record<string, unknown> {
  return {
    snapshotId: snapshot.snapshot_id,
    currentMemory: snapshot.current_bytes,
    peakMemory: snapshot.peak_bytes,
    gcObjects: snapshot.object_count,
    gcCounts: snapshot.gc_generations,
    generationSizes: snapshot.generation_sizes?.map((region) => ({ name: region.name, size: region.size_bytes })) ?? [],
    topAllocations: snapshot.sites.map(allocation),
    timeline: timeline.map((point) => ({
      timestamp: point.elapsed_secs,
      currentMemory: point.current_bytes,
      peakMemory: point.peak_bytes,
      gcObjects: point.object_count,
    })),
    heapProfilePath: heapProfilePath ?? "",
  };
}

function leak(entry: SuspectedLeak): Record<string, unknown> {
  return {
    file: entry.file ?? "",
    line: entry.line ?? 0,
    typeName: entry.type_name ?? "",
    sizeGrowth: entry.size_growth,
    countGrowth: entry.count_growth,
    currentSize: entry.current_size,
    currentCount: entry.current_count,
    confidence: entry.confidence.kind.toUpperCase(),
    reason: entry.reason,
  };
}

function diffFields(diff: HeapDiff, leaks: SuspectedLeak[]): Record<string, unknown> {
  return {
    totalGrowth: diff.total_growth,
    totalFreed: diff.total_freed,
    netGrowth: diff.net_growth,
    grownAllocations: diff.grown.map((site) => ({ ...allocation(site), sizeDiff: site.size_diff, countDiff: site.count_diff })),
    suspectedLeaks: leaks.map(leak),
  };
}

/** The retention graph as the webview draws it: numeric node IDs, `repr` labels. */
export function toRefGraph(graph: RefGraph): Record<string, unknown> {
  const ids = new Map<string, number>(graph.nodes.map((node, index) => [node.id, index]));
  return {
    nodes: graph.nodes.map((node, index) => ({
      id: index,
      type: node.type_name,
      size: node.size_bytes,
      repr: node.label,
      depth: node.depth,
      isTarget: node.is_target,
      isRoot: node.is_root,
      rootKind: node.root_kind,
    })),
    edges: graph.edges.map((edge) => ({ from: ids.get(edge.from) ?? -1, to: ids.get(edge.to) ?? -1, label: edge.label ?? "" })),
    cycles: graph.cycles.map((cycle) => cycle.map((id) => ids.get(id) ?? -1)),
    truncated: graph.truncated,
  };
}

function outcomeFields(outcome: MemoryOutcome): MemoryIngestResult {
  switch (outcome.kind) {
    case "Ack":
      return { kind: "ack" };
    case "Snapshot":
      return { kind: "snapshot", ...snapshotFields(outcome.snapshot, outcome.timeline, outcome.heapprofile_path), trendingUpward: outcome.trending_upward };
    case "Diff":
      return { kind: "diff", ...diffFields(outcome.diff, outcome.leaks) };
    case "Gc":
      return { kind: "gc", collected: outcome.gc.collected, uncollectable: outcome.gc.uncollectable_count, freedBytes: outcome.gc.freed_bytes };
    case "Refs":
      return { kind: "refs", graph: toRefGraph(outcome.graph) };
    case "Objects":
      return {
        kind: "objects",
        typeName: outcome.objects.type_name,
        totalCount: outcome.objects.total_count,
        totalBytes: outcome.objects.total_bytes,
        objects: outcome.objects.objects.map((o) => ({ id: o.id, type: o.type_name, size: o.size_bytes, repr: o.label })),
        typeSummary: outcome.objects.type_summary.map((t) => ({ type: t.type_name, count: t.count, size: t.size_bytes })),
      };
  }
}

/** A completed memory step as the dashboard, decorations and autopilot consume it. */
export function toMemoryIngestResult(result: MemoryResult): MemoryIngestResult {
  return { memorySessionId: result.memory_session_id, ...outcomeFields(result.outcome) };
}
