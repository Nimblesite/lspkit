typeDiagram

# Wire models for the lsp-profiler protocol: every request, response and
# notification shared by the Rust service and the editor UI library.
# Implements [PROF-WIRE]. Field names are snake_case on both sides of the wire.
# Comments directly above a type, variant or field become its doc comment in
# the generated Rust and TypeScript.

# ---------------------------------------------------------------------------
# Runtimes and capabilities — [PROF-RUNTIME]
# ---------------------------------------------------------------------------

# How a runtime acquires memory data. [PROF-MEMORY-MODE]
union MemoryMode {
  # The editor evaluates a script inside the paused debuggee and posts the output back.
  Courier
  # The language server captures the snapshot itself from the process ID.
  Direct
}

# What the CPU profiler can do for one runtime.
type CpuCapabilities {
  # Samples stream while recording, so live progress shows a sample count.
  live_progress: Bool
  # Frames carry file and line, so hot lines exist (not only hot functions).
  line_level: Bool
  # Native (non-managed) frames can be included on request.
  native_frames: Bool
  # The editor can inject a cooperative sampler into a debug session.
  cooperative: Bool
}

# What the memory profiler can do for one runtime.
type MemoryCapabilities {
  # How memory data is acquired.
  mode: MemoryMode
  # Allocation sites carry file and line.
  allocation_sites: Bool
  # Retention graphs (who keeps objects of a type alive) are available.
  references: Bool
  # Objects of one type can be listed.
  objects_by_type: Bool
  # A garbage collection can be forced.
  gc_collect: Bool
}

# One runtime the language server can profile.
type RuntimeInfo {
  # Stable identifier, e.g. `python`, `dotnet`, `native`.
  id: String
  # Human-readable name for the UI.
  display_name: String
  # A process's `entry` is a source file an editor can open (a script), never a binary or assembly.
  source_entries: Bool
  # CPU capabilities, absent when the runtime has no CPU profiler.
  cpu: Option<CpuCapabilities>
  # Memory capabilities, absent when the runtime has no memory profiler.
  memory: Option<MemoryCapabilities>
}

# Result of `lspProfiler/capabilities`.
type ProfilerCapabilities {
  # Every runtime this server profiles.
  runtimes: List<RuntimeInfo>
}

# ---------------------------------------------------------------------------
# Samples and the elevated-helper IPC — [PROF-SAMPLE] [PROF-HELPER-PROTOCOL]
# ---------------------------------------------------------------------------

# One stack frame captured by a sampler.
type StackFrame {
  # Function name as the runtime reports it.
  name: String
  # Source path, when the runtime knows it.
  file: Option<String>
  # 1-based line, when the runtime knows it.
  line: Option<Int>
  # Module, assembly or library that owns the frame, when known.
  module: Option<String>
}

# One thread's stack at one sampling tick.
type ThreadSample {
  # OS or runtime thread identifier.
  thread_id: Int
  # Thread name, when known.
  thread_name: Option<String>
  # The thread was running (not idle) when sampled.
  active: Bool
  # Frames, leaf (innermost) first.
  frames: List<StackFrame>
}

# Why attaching a sampler failed. [PROF-ERRORS]
union AttachErrorKind {
  # The process does not exist or exited before attach.
  ProcessNotFound
  # The process does not run the requested runtime.
  UnsupportedRuntime
  # Attaching needs privileges the profiler does not have.
  PermissionDenied
  # Any other attach failure.
  AttachFailed
}

# A command from the language server to the elevated sampling helper.
union HelperCommand {
  # Attach to a process and stream samples.
  Attach {
    # Target process.
    pid: Int
    # Samples per second.
    sample_rate: Int
    # Include native frames.
    include_native: Bool
  }
  # Stop sampling and exit.
  Stop
}

# A message from the elevated sampling helper to the language server.
union HelperMessage {
  # Attach succeeded.
  Attached {
    # Target process.
    pid: Int
    # Runtime version detected in the target.
    runtime_version: String
  }
  # Samples from one sampling tick.
  Samples {
    # One stack per sampled thread.
    samples: List<ThreadSample>
  }
  # Sampling stopped; the helper is exiting.
  Stopped
  # The helper failed; sent before exit so the cause is never a bare EOF.
  Failure {
    # Classified failure.
    error_kind: AttachErrorKind
    # Underlying cause.
    message: String
  }
}

# ---------------------------------------------------------------------------
# Processes — [PROF-PROCESSES]
# ---------------------------------------------------------------------------

# One process the profiler recognised as belonging to a supported runtime.
type ProcessInfo {
  # Operating-system process ID.
  pid: Int
  # Parent process ID, when known.
  ppid: Option<Int>
  # Process name as reported by the OS.
  name: String
  # Identifier of the runtime that recognised the process.
  runtime: String
  # Runtime version (interpreter or framework), when known.
  runtime_version: Option<String>
  # Host executable (interpreter, `dotnet`, or the binary itself).
  executable: Option<String>
  # What the process runs: a script, an assembly, or a binary path.
  entry: Option<String>
  # CPU usage in percent of one core.
  cpu_percent: Float
  # Resident memory in bytes.
  memory_bytes: Int
  # Seconds since the process started.
  runtime_secs: Int
  # Owning user, when known.
  user: Option<String>
  # Attaching needs elevated privileges.
  requires_elevation: Bool
  # The process belongs to an open workspace root.
  in_workspace: Bool
  # Known launcher or framework (`uvicorn`, `pytest`, `aspnet`), when detected.
  launcher: Option<String>
  # The profiler can attach to this process.
  attachable: Bool
  # Why the profiler cannot attach, when `attachable` is false.
  unattachable_reason: Option<String>
}

# Result of `lspProfiler/processes`.
type ProcessList {
  # Recognised processes, attachable ones first.
  processes: List<ProcessInfo>
}

# Parameters of `lspProfiler/processes`.
type ProcessListParams {
  # Workspace roots used to mark `in_workspace`.
  workspace_roots: List<String>
}

# Parameters of `lspProfiler/kill`.
type KillProcessParams {
  # Process to terminate; refused unless a runtime recognises it.
  pid: Int
}

# Result of `lspProfiler/kill`.
type KillProcessResult {
  # Terminated process ID.
  pid: Int
  # Always true on success.
  killed: Bool
}

# ---------------------------------------------------------------------------
# CPU profiling — [PROF-CPU]
# ---------------------------------------------------------------------------

# Parameters of `lspProfiler/cpu/start`.
type StartProfileParams {
  # Process to attach to.
  pid: Int
  # Runtime to use; detected from the process when absent.
  runtime: Option<String>
  # Samples per second.
  sample_rate: Option<Int>
  # Include native frames when the runtime supports them.
  include_native: Option<Bool>
  # Stop automatically after this many seconds.
  duration_secs: Option<Float>
  # Named preset (`quick`, `detailed`, `longRunning`); absent or `default` uses the defaults.
  preset: Option<String>
}

# Result of starting a CPU session.
type StartProfileResult {
  # New session identifier.
  session_id: String
  # Profiled process.
  pid: Int
  # Runtime that owns the session.
  runtime: String
  # Runtime version reported by the backend.
  runtime_version: String
  # RFC 3339 start time.
  started_at: String
}

# Parameters of `lspProfiler/cpu/cooperativeScript`.
type CooperativeScriptParams {
  # Runtime whose sampler script is wanted.
  runtime: String
  # Samples per second.
  sample_rate: Option<Int>
}

# A sampler script for the editor to inject into a paused debuggee.
type CooperativeScript {
  # Source to evaluate in the debuggee.
  script: String
  # File the injected sampler streams to; pass it to `cooperativeAttach`.
  sample_file: String
}

# Parameters of `lspProfiler/cpu/cooperativeAttach`.
type CooperativeAttachParams {
  # Runtime whose sampler was injected.
  runtime: String
  # File returned by `cooperativeScript`.
  sample_file: String
  # Samples per second.
  sample_rate: Option<Int>
}

# Parameters naming one CPU session.
type ProfileSessionParams {
  # Session to act on.
  session_id: String
}

# A source line that crossed the hot-line threshold.
type HotLine {
  # Absolute source path.
  file: String
  # 1-based line number.
  line: Int
  # Samples touching the line.
  samples: Int
  # Share of samples touching the line, 0–100.
  percentage: Float
}

# A function that crossed the hot-function threshold.
type HotFunction {
  # Function name as the runtime reports it.
  name: String
  # Absolute source path, when the runtime or a source resolver knows it.
  file: Option<String>
  # 1-based definition line, when known.
  line: Option<Int>
  # Samples with the function anywhere on the stack.
  samples: Int
  # Inclusive share, 0–100.
  percentage: Float
  # Samples with the function as the leaf.
  self_samples: Int
  # Exclusive share, 0–100.
  self_percentage: Float
}

# Files written for a profile.
type ProfileArtifacts {
  # Speedscope JSON.
  speedscope_path: Option<String>
  # Self-contained flame graph SVG.
  flamegraph_path: Option<String>
  # V8 `.cpuprofile` for the editor's built-in viewer.
  cpuprofile_path: Option<String>
  # Why an export was refused or failed.
  export_error: Option<String>
}

# Severity of a profiler diagnostic.
union DiagnosticLevel {
  # Informational hint.
  Hint
  # Information.
  Information
  # Warning.
  Warning
  # Error.
  Error
}

# One diagnostic the editor shows in its own profiler collection.
type ProfilerDiagnostic {
  # 1-based line.
  line: Int
  # Message text.
  message: String
  # Stable diagnostic code.
  code: String
  # Severity.
  severity: DiagnosticLevel
  # Diagnostic source label.
  source: String
}

# Diagnostics for one file.
type FileDiagnostics {
  # `file://` URI.
  uri: String
  # Diagnostics for the file; empty clears it.
  diagnostics: List<ProfilerDiagnostic>
}

# Result of `lspProfiler/cpu/stop` and `lspProfiler/cpu/snapshot`.
type ProfileReport {
  # Session identifier.
  session_id: String
  # Profiled process.
  pid: Int
  # Runtime that owns the session.
  runtime: String
  # Seconds recorded.
  duration_secs: Float
  # Sampling ticks taken.
  total_samples: Int
  # Ticks in which a thread sample survived runtime filtering (user code): every share's denominator.
  user_samples: Int
  # Samples per second.
  sample_rate: Int
  # Hot functions, hottest first.
  hot_functions: List<HotFunction>
  # Hot lines, hottest first.
  hot_lines: List<HotLine>
  # Exported files.
  artifacts: ProfileArtifacts
  # Hotspot diagnostics to show.
  diagnostics: List<FileDiagnostics>
}

# One active CPU session.
type ProfileSessionInfo {
  # Session identifier.
  session_id: String
  # Profiled process.
  pid: Int
  # Runtime that owns the session.
  runtime: String
  # Runtime version.
  runtime_version: String
  # RFC 3339 start time.
  started_at: String
  # Samples so far.
  sample_count: Int
  # Seconds so far.
  duration_secs: Float
}

# Result of `lspProfiler/cpu/list`.
type ProfileSessionList {
  # Active sessions.
  sessions: List<ProfileSessionInfo>
}

# Notification `lspProfiler/cpu/progress`, sent once per second per session.
type ProfileProgress {
  # Session identifier.
  session_id: String
  # Samples so far.
  sample_count: Int
  # Seconds so far.
  duration_secs: Float
  # Hottest function so far.
  top_function: Option<String>
}

# ---------------------------------------------------------------------------
# Memory profiling — [PROF-MEMORY]
# ---------------------------------------------------------------------------

# One frame of an allocation traceback.
type TraceFrame {
  # Source path.
  file: String
  # 1-based line.
  line: Int
}

# Where memory was allocated: a source line, a type, or both.
type AllocationSite {
  # Source path, when the runtime attributes allocations to lines.
  file: Option<String>
  # 1-based line, when known.
  line: Option<Int>
  # Type name, when the runtime attributes allocations to types.
  type_name: Option<String>
  # Live bytes.
  size_bytes: Int
  # Live objects.
  count: Int
  # Allocation call stack, root first.
  traceback: List<TraceFrame>
}

# Heap bytes for one runtime-named generation or heap region.
type GenerationSize {
  # Runtime label, such as Gen 0 or LOH.
  name: String
  # Bytes retained in the region after collection.
  size_bytes: Int
}

# A point-in-time heap summary.
type HeapSnapshot {
  # Snapshot identifier.
  snapshot_id: String
  # Live bytes now.
  current_bytes: Int
  # Peak bytes since tracking began.
  peak_bytes: Int
  # Live objects tracked.
  object_count: Int
  # Collection counts per generation.
  gc_generations: List<Int>
  # Generation sizes, distinct from collection counts; absent when unsupported.
  generation_sizes: Option<List<GenerationSize>>
  # Largest allocation sites.
  sites: List<AllocationSite>
}

# Change at one allocation site between two snapshots.
type SiteGrowth {
  # Source path, when known.
  file: Option<String>
  # 1-based line, when known.
  line: Option<Int>
  # Type name, when known.
  type_name: Option<String>
  # Byte change (negative when freed).
  size_diff: Int
  # Object-count change.
  count_diff: Int
  # Live bytes now.
  size_bytes: Int
  # Live objects now.
  count: Int
  # Allocation call stack, root first.
  traceback: List<TraceFrame>
}

# Comparison of the two most recent snapshots.
type HeapDiff {
  # Bytes added across growing sites.
  total_growth: Int
  # Bytes released across shrinking sites.
  total_freed: Int
  # Growth minus freed.
  net_growth: Int
  # Growing sites, largest first.
  grown: List<SiteGrowth>
  # Shrinking sites, largest first.
  freed: List<SiteGrowth>
}

# Confidence that a growing site leaks. [PROF-MEMORY-CONFIDENCE]
union LeakConfidence {
  # One growth.
  Low
  # Two consecutive growths, or one growth over 10 MiB.
  Medium
  # Three or more consecutive growths.
  High
  # A finaliser-carrying object in an uncollectable cycle.
  Definite
}

# A growing allocation site scored for leak confidence.
type SuspectedLeak {
  # Source path, when known.
  file: Option<String>
  # 1-based line, when known.
  line: Option<Int>
  # Type name, when known.
  type_name: Option<String>
  # Byte growth in the latest diff.
  size_growth: Int
  # Object growth in the latest diff.
  count_growth: Int
  # Live bytes now.
  current_size: Int
  # Live objects now.
  current_count: Int
  # Confidence level.
  confidence: LeakConfidence
  # Why the level was chosen.
  reason: String
  # Allocation call stack, root first.
  traceback: List<TraceFrame>
}

# An object the collector could not free.
type UncollectableObject {
  # Runtime object identity.
  id: String
  # Type name.
  type_name: String
  # Short textual rendering.
  label: String
  # Shallow size in bytes.
  size_bytes: Int
  # Why it cannot be collected.
  reason: String
}

# Result of a forced collection.
type GcResult {
  # Objects collected.
  collected: Int
  # Objects that could not be collected.
  uncollectable_count: Int
  # Bytes released.
  freed_bytes: Int
  # Details of uncollectable objects.
  uncollectable: List<UncollectableObject>
}

# One object in a retention graph.
type RefNode {
  # Runtime object identity.
  id: String
  # Type name.
  type_name: String
  # Short textual rendering.
  label: String
  # Shallow size in bytes.
  size_bytes: Int
  # Retained size in bytes, when the runtime computes it.
  retained_bytes: Option<Int>
  # Distance from the target objects.
  depth: Int
  # One of the objects the graph was built for.
  is_target: Bool
  # A garbage-collection root.
  is_root: Bool
  # Kind of root, when `is_root`.
  root_kind: Option<String>
}

# A reference from `from` (the holder) to `to` (the held object).
type RefEdge {
  # Holder identity.
  from: String
  # Held identity.
  to: String
  # Field, key or index holding the reference.
  label: Option<String>
}

# Who keeps objects of one type alive.
type RefGraph {
  # Objects.
  nodes: List<RefNode>
  # References.
  edges: List<RefEdge>
  # Reference cycles, as node-ID paths.
  cycles: List<List<String>>
  # The node budget cut the walk short.
  truncated: Bool
}

# One live object of a requested type.
type ObjectSample {
  # Runtime object identity.
  id: String
  # Type name.
  type_name: String
  # Shallow size in bytes.
  size_bytes: Int
  # Reference count, when the runtime has one.
  refcount: Option<Int>
  # Short textual rendering.
  label: String
}

# Aggregate count and size of one type.
type TypeStat {
  # Type name.
  type_name: String
  # Live objects.
  count: Int
  # Live bytes.
  size_bytes: Int
}

# Live objects of one type plus a per-type summary.
type ObjectList {
  # Requested type.
  type_name: String
  # Live objects of the type.
  total_count: Int
  # Live bytes of the type.
  total_bytes: Int
  # Largest objects of the type.
  objects: List<ObjectSample>
  # Largest types overall, then the requested type's own row when it is too small to rank.
  type_summary: List<TypeStat>
}

# One point on the memory timeline.
type TimelinePoint {
  # Seconds since tracking began.
  elapsed_secs: Float
  # Live bytes.
  current_bytes: Int
  # Peak bytes.
  peak_bytes: Int
  # Live objects.
  object_count: Int
  # Snapshot that produced the point.
  snapshot_id: String
}

# What one memory step produced. [PROF-MEMORY-OUTCOME]
union MemoryOutcome {
  # Acknowledged with no data (start/stop).
  Ack
  # A heap snapshot.
  Snapshot {
    # The snapshot.
    snapshot: HeapSnapshot
    # Every snapshot of the session so far, oldest first.
    timeline: List<TimelinePoint>
    # Live bytes rose across the recent timeline.
    trending_upward: Bool
    # V8 `.heapprofile` export, when written.
    heapprofile_path: Option<String>
  }
  # A comparison with leak scoring.
  Diff {
    # The comparison.
    diff: HeapDiff
    # Growing sites scored for leak confidence.
    leaks: List<SuspectedLeak>
  }
  # A forced collection.
  Gc {
    # The collection result.
    gc: GcResult
  }
  # A retention graph.
  Refs {
    # The retention graph.
    graph: RefGraph
  }
  # Objects of one type.
  Objects {
    # The objects.
    objects: ObjectList
  }
}

# Scored outcome plus the diagnostics it implies.
type MemoryResult {
  # Memory session the result belongs to.
  memory_session_id: String
  # What happened.
  outcome: MemoryOutcome
  # Diagnostics to show.
  diagnostics: List<FileDiagnostics>
}

# What the editor must do next for a memory step. [PROF-MEMORY-STEP]
union MemoryStep {
  # Evaluate `script` in the paused debuggee, then send its output to `lspProfiler/memory/ingest`.
  Evaluate {
    # Source to evaluate in the debuggee.
    script: String
    # Text that marks the script's output as complete; the editor collects evaluate results and debuggee output until it appears.
    output_marker: String
  }
  # The server already captured the data.
  Completed {
    # The captured result.
    result: MemoryResult
  }
}

# Parameters of `lspProfiler/memory/start`.
type MemoryStartParams {
  # Process to track.
  pid: Int
  # Runtime to use; detected from the process when absent.
  runtime: Option<String>
  # Allocation traceback depth, for runtimes that record tracebacks.
  traceback_depth: Option<Int>
}

# Result of `lspProfiler/memory/start`.
type MemoryStartResult {
  # New memory session.
  memory_session_id: String
  # Runtime that owns the session.
  runtime: String
  # Mode the runtime uses.
  mode: MemoryMode
  # First step.
  step: MemoryStep
}

# Parameters naming one memory session.
type MemorySessionParams {
  # Session to act on.
  memory_session_id: String
}

# Parameters of `references` and `objectsByType`.
type MemoryTypeParams {
  # Session to act on.
  memory_session_id: String
  # Type to inspect.
  type_name: String
  # Maximum objects or nodes.
  limit: Option<Int>
  # Maximum retention depth.
  max_depth: Option<Int>
}

# Parameters of `lspProfiler/memory/ingest`.
type MemoryIngestParams {
  # Session the output belongs to.
  memory_session_id: String
  # Raw output of an evaluated script.
  output: String
}
