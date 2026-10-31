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
  ProfileReport,
  RefGraph,
  StartProfileResult,
  SuspectedLeak,
  TimelinePoint,
  ProcessInfo as WireProcess,
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
  const { artifacts } = report;
  return {
    sessionId: report.session_id,
    duration: report.duration_secs,
    totalSamples: report.total_samples,
    outputFile: artifacts.speedscope_path ?? "",
    ...(artifacts.cpuprofile_path === undefined ? {} : { cpuProfilePath: artifacts.cpuprofile_path }),
    ...(artifacts.flamegraph_path === undefined ? {} : { flamegraphPath: artifacts.flamegraph_path }),
    ...(artifacts.export_error === undefined ? {} : { exportError: artifacts.export_error }),
    hotFunctions: report.hot_functions.map((fn) => ({
      name: fn.name, ...located(fn), samples: fn.samples, percentage: fn.percentage, selfPercentage: fn.self_percentage,
    })),
    hotLines: report.hot_lines.map((line) => ({ file: line.file, line: line.line, samples: line.samples, percentage: line.percentage })),
  };
}

export function toUiProcess(process: WireProcess): ProcessInfo {
  return {
    pid: process.pid, ppid: process.ppid ?? 0,
    name: process.name, runtime: process.runtime,
    interpreterPath: process.executable ?? null,
    script: process.entry ?? null,
    runtimeVersion: process.runtime_version ?? null,
    cpuPercent: process.cpu_percent, memoryBytes: process.memory_bytes,
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
  const { file, line } = entry;
  return file !== null && file !== undefined && line !== null && line !== undefined ? { file, line } : {};
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
    suspectedLeaks: leaks.map((entry) => leak(entry)),
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

type MemoryStepOutcome = Extract<MemoryOutcome, { kind: "Snapshot" | "Diff" | "Gc" }>;
type RetentionOutcome = Extract<MemoryOutcome, { kind: "Refs" | "Objects" }>;

function outcomeFields(outcome: MemoryOutcome): MemoryIngestResult {
  if (outcome.kind === "Ack") { return { kind: "ack" }; }
  if (outcome.kind === "Snapshot" || outcome.kind === "Diff" || outcome.kind === "Gc") {
    return memoryStepOutcomeFields(outcome);
  }
  return retentionOutcomeFields(outcome);
}

/** The ingest shape for a snapshot, diff, or gc outcome. */
function memoryStepOutcomeFields(outcome: MemoryStepOutcome): MemoryIngestResult {
  if (outcome.kind === "Snapshot") {
    return { kind: "snapshot", ...snapshotFields(outcome.snapshot, outcome.timeline, outcome.heapprofile_path), trendingUpward: outcome.trending_upward };
  }
  if (outcome.kind === "Diff") {
    return { kind: "diff", ...diffFields(outcome.diff, outcome.leaks) };
  }
  return { kind: "gc", collected: outcome.gc.collected, uncollectable: outcome.gc.uncollectable_count, freedBytes: outcome.gc.freed_bytes };
}

/** The ingest shape for a reference-graph or live-objects outcome. */
function retentionOutcomeFields(outcome: RetentionOutcome): MemoryIngestResult {
  if (outcome.kind === "Refs") {
    return { kind: "refs", graph: toRefGraph(outcome.graph) };
  }
  return { kind: "objects", ...objectsFields(outcome.objects) };
}

/** One type's live objects and the per-type totals beside them. */
function objectsFields(objects: Extract<MemoryOutcome, { kind: "Objects" }>["objects"]): Omit<MemoryIngestResult, "kind"> {
  return {
    typeName: objects.type_name,
    totalCount: objects.total_count,
    totalBytes: objects.total_bytes,
    objects: objects.objects.map((object) => ({ id: object.id, type: object.type_name, size: object.size_bytes, repr: object.label })),
    typeSummary: objects.type_summary.map((type) => ({ type: type.type_name, count: type.count, size: type.size_bytes })),
  };
}

/** A completed memory step as the dashboard, decorations and autopilot consume it. */
export function toMemoryIngestResult(result: MemoryResult): MemoryIngestResult {
  return { memorySessionId: result.memory_session_id, ...outcomeFields(result.outcome) };
}
