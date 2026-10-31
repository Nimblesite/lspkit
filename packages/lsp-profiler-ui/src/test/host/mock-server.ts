// A language server that speaks the lsp-profiler wire with canned data.
// Tests [PROF-LSP] from the UI side: every operation is a
// `workspace/executeCommand` under the host prefix, progress arrives as the
// protocol's notification, and the payloads are the generated wire models.
//
// Suites steer it with the `lspui.mock.configure` command: fail the next
// processes fetch, choose a memory mode, or swap the process list.

import type { InitializeResult } from "vscode-languageserver/node";
import {
  ProposedFeatures,
  TextDocuments,
  createConnection,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import type {
  HeapDiff,
  HeapSnapshot,
  MemoryResult,
  MemoryStartResult,
  MemoryStep,
  ProcessInfo,
  ProcessList,
  ProfileProgress,
  ProfileReport,
  ProfileSessionList,
  StartProfileResult,
} from "../../wire-generated";
import { METHODS } from "../../wire-generated";
import { asRecord, booleanField, isRecord, rawField, stringField } from "../../unknown-shape";

const PREFIX = "lspui";
const COMMAND_SEPARATOR = ".";
const PROGRESS_INTERVAL_MS = 200;
const MILLIS_PER_SAMPLE = 10;
const MILLIS_PER_SECOND = 1000;
const DEFAULT_HOT_SAMPLE_PERCENT = 80;
const DEFAULT_HOT_LINE_PERCENT = 70;
const COLD_HOT_LINE_PERCENT = 10;
const HIGH_CPU_PID = 4242;
const HIGH_CPU_PERCENT = 42.5;
const LOW_CPU_PERCENT = 1.5;
const MEGABYTE_BYTES = 1024 * 1024;
const MOCK_RUNTIME_VERSION = "1.0.0";
const MOCK_RUNTIME = "mock";
const MOCK_EXECUTABLE = "/usr/bin/mock";
const SNAPSHOT_SAMPLES = 50;
const MIN_STOP_SAMPLES = 10;
const CPU_SESSION_PREFIX = "cpu";
const MEMORY_SESSION_PREFIX = "mem";
const INGEST_MARKER = "__MOCK__";
const FIRST_SESSION_ID = 1;

const connection = createConnection(ProposedFeatures.all);
const documents = new TextDocuments(TextDocument);

/** Steerable behaviour. */
const mock = {
  failProcesses: false,
  memoryMode: "Direct" as "Direct" | "Courier",
  processes: defaultProcesses(),
  hotFile: "",
  sessions: new Map<string, { pid: number; started: number; timer: NodeJS.Timeout }>(),
  memorySessions: new Map<string, number>(),
  nextId: FIRST_SESSION_ID,
};

/** One canned process: the fields `mockProcess` turns into a wire row. */
interface MockProcessSpec {
  readonly pid: number;
  readonly name: string;
  readonly runtime: string;
  readonly version: string | null;
  readonly executable: string;
  readonly entry: string | null;
  readonly inWorkspace: boolean;
  readonly attachable: boolean;
  readonly reason?: string;
}

/** The next mock session number, consumed once per session. */
function nextSessionNumber(): number {
  const number = mock.nextId;
  mock.nextId += 1;
  return number;
}

function mockProcess(spec: MockProcessSpec): ProcessInfo {
  return {
    pid: spec.pid,
    ppid: 1,
    name: spec.name,
    runtime: spec.runtime,
    ...(spec.version === null ? {} : { runtime_version: spec.version }),
    executable: spec.executable,
    ...(spec.entry === null ? {} : { entry: spec.entry }),
    cpu_percent: spec.pid === HIGH_CPU_PID ? HIGH_CPU_PERCENT : LOW_CPU_PERCENT,
    memory_bytes: 64 * MEGABYTE_BYTES,
    runtime_secs: 120,
    user: "tester",
    requires_elevation: false,
    in_workspace: spec.inWorkspace,
    attachable: spec.attachable,
    ...(spec.reason === undefined ? {} : { unattachable_reason: spec.reason }),
  };
}

function defaultProcesses(): ProcessInfo[] {
  return [
    mockProcess({
      pid: 4242,
      name: "worker",
      runtime: MOCK_RUNTIME,
      version: MOCK_RUNTIME_VERSION,
      executable: MOCK_EXECUTABLE,
      entry: "/work/app/worker.txt",
      inWorkspace: true,
      attachable: true,
    }),
    mockProcess({
      pid: 4343,
      name: "api",
      runtime: MOCK_RUNTIME,
      version: MOCK_RUNTIME_VERSION,
      executable: MOCK_EXECUTABLE,
      entry: "/work/app/api.txt",
      inWorkspace: false,
      attachable: true,
    }),
    mockProcess({
      pid: 4444,
      name: "blocked",
      runtime: MOCK_RUNTIME,
      version: null,
      executable: MOCK_EXECUTABLE,
      entry: null,
      inWorkspace: false,
      attachable: false,
      reason: "no diagnostics endpoint",
    }),
  ];
}

function commandNames(): string[] {
  const advertised = Object.values(METHODS)
    .filter((method) => method !== METHODS.CPU_PROGRESS)
    .map((method) => `${PREFIX}.${method.replaceAll("/", COMMAND_SEPARATOR)}`);
  return [...advertised, `${PREFIX}.mock.configure`];
}

connection.onInitialize((): InitializeResult => ({
  capabilities: {
    executeCommandProvider: { commands: commandNames() },
    experimental: { lspProfiler: { runtimes: [{ id: MOCK_RUNTIME, display_name: "Mock", source_entries: true, cpu: { live_progress: true, line_level: true, native_frames: false, cooperative: false }, memory: { mode: { kind: mock.memoryMode }, allocation_sites: true, references: true, objects_by_type: true, gc_collect: true } }] } },
  },
}));

function report(sessionId: string, pid: number, samples: number): ProfileReport {
  return {
    session_id: sessionId,
    pid,
    runtime: MOCK_RUNTIME,
    duration_secs: 1.5,
    total_samples: samples,
    user_samples: samples,
    sample_rate: 100,
    hot_functions: [
      { name: "hot_function", file: mock.hotFile, line: 3, samples: Math.round(samples * 0.8), percentage: DEFAULT_HOT_SAMPLE_PERCENT, self_samples: Math.round(samples * 0.8), self_percentage: DEFAULT_HOT_SAMPLE_PERCENT },
      { name: "main", file: mock.hotFile, line: 1, samples, percentage: 100, self_samples: 0, self_percentage: 0 },
    ],
    hot_lines: [
      { file: mock.hotFile, line: 4, samples: Math.round(samples * 0.7), percentage: DEFAULT_HOT_LINE_PERCENT },
      { file: mock.hotFile, line: 5, samples: Math.round(samples * 0.1), percentage: COLD_HOT_LINE_PERCENT },
    ],
    artifacts: { speedscope_path: `${mock.hotFile}.speedscope.json`, flamegraph_path: `${mock.hotFile}.svg`, cpuprofile_path: `${mock.hotFile}.cpuprofile` },
    diagnostics: [],
  };
}

function snapshot(id: number): HeapSnapshot {
  return {
    snapshot_id: `snap-${id}`,
    current_bytes: 1_000_000 * id,
    peak_bytes: 1_200_000 * id,
    object_count: 1000 * id,
    gc_generations: [10, 5, 1],
    sites: [
      { file: mock.hotFile, line: 4, type_name: "Payload", size_bytes: 500_000 * id, count: 500 * id, traceback: [] },
      { file: mock.hotFile, line: 2, type_name: "Buffer", size_bytes: 100_000, count: 10, traceback: [] },
    ],
  };
}

function diff(): HeapDiff {
  const grown = { file: mock.hotFile, line: 4, type_name: "Payload", size_diff: 500_000, count_diff: 500, size_bytes: 1_000_000, count: 1000, traceback: [] };
  return { total_growth: 500_000, total_freed: 0, net_growth: 500_000, grown: [grown], freed: [] };
}

function completed(memorySessionId: string, outcome: MemoryResult["outcome"]): MemoryStep {
  return { kind: "Completed", result: { memory_session_id: memorySessionId, outcome, diagnostics: [] } };
}

function snapshotStep(memorySessionId: string, snapshots: number): MemoryStep {
  return completed(memorySessionId, { kind: "Snapshot", snapshot: snapshot(snapshots), timeline: [], trending_upward: snapshots > 1 });
}

function diffStep(memorySessionId: string): MemoryStep {
  return completed(memorySessionId, {
    kind: "Diff",
    diff: diff(),
    leaks: [{ file: mock.hotFile, line: 4, type_name: "Payload", size_growth: 500_000, count_growth: 500, current_size: 1_000_000, current_count: 1000, confidence: { kind: "High" }, reason: "grew on every snapshot", traceback: [] }],
  });
}

function gcStep(memorySessionId: string): MemoryStep {
  return completed(memorySessionId, { kind: "Gc", gc: { collected: 7, uncollectable_count: 1, freed_bytes: 4096, uncollectable: [] } });
}

function referencesStep(memorySessionId: string): MemoryStep {
  return completed(memorySessionId, {
    kind: "Refs",
    graph: {
      nodes: [
        { id: "0x1", type_name: "Payload", label: "0x1", size_bytes: 32, depth: 0, is_target: true, is_root: false },
        { id: "0x2", type_name: "List", label: "static Items", size_bytes: 64, depth: 1, is_target: false, is_root: true, root_kind: "Static" },
      ],
      edges: [{ from: "0x2", to: "0x1" }],
      cycles: [],
      truncated: false,
    },
  });
}

function objectsStep(memorySessionId: string): MemoryStep {
  return completed(memorySessionId, {
    kind: "Objects",
    objects: { type_name: "Payload", total_count: 1000, total_bytes: 32_000, objects: [{ id: "0x1", type_name: "Payload", size_bytes: 32, label: "0x1" }], type_summary: [{ type_name: "Payload", count: 1000, size_bytes: 32_000 }] },
  });
}

function memoryStep(method: string, memorySessionId: string): MemoryStep {
  const snapshots = (mock.memorySessions.get(memorySessionId) ?? 0) + 1;
  mock.memorySessions.set(memorySessionId, snapshots);
  switch (method) {
    case METHODS.MEMORY_SNAPSHOT: {
      return snapshotStep(memorySessionId, snapshots);
    }
    case METHODS.MEMORY_DIFF: {
      return diffStep(memorySessionId);
    }
    case METHODS.MEMORY_GC_COLLECT: {
      return gcStep(memorySessionId);
    }
    case METHODS.MEMORY_REFERENCES: {
      return referencesStep(memorySessionId);
    }
    case METHODS.MEMORY_OBJECTS_BY_TYPE: {
      return objectsStep(memorySessionId);
    }
    default: {
      mock.memorySessions.delete(memorySessionId);
      return completed(memorySessionId, { kind: "Ack" });
    }
  }
}

function startCpu(pid: number): StartProfileResult {
  const sessionId = `${CPU_SESSION_PREFIX}-${nextSessionNumber()}`;
  const started = Date.now();
  const timer = setInterval(() => {
    const elapsed = Date.now() - started;
    const progress: ProfileProgress = { session_id: sessionId, sample_count: Math.round(elapsed / MILLIS_PER_SAMPLE), duration_secs: elapsed / MILLIS_PER_SECOND, top_function: "hot_function" };
    void connection.sendNotification(METHODS.CPU_PROGRESS, progress);
  }, PROGRESS_INTERVAL_MS);
  mock.sessions.set(sessionId, { pid, started, timer });
  return { session_id: sessionId, pid, runtime: MOCK_RUNTIME, runtime_version: MOCK_RUNTIME_VERSION, started_at: new Date(started).toISOString() };
}

function stopCpu(sessionId: string): ProfileReport {
  const session = mock.sessions.get(sessionId);
  if (session === undefined) {
    throw new Error(`unknown session ${sessionId}`);
  }
  clearInterval(session.timer);
  mock.sessions.delete(sessionId);
  const sampled = Math.round((Date.now() - session.started) / MILLIS_PER_SAMPLE);
  return report(sessionId, session.pid, Math.max(MIN_STOP_SAMPLES, sampled));
}

function sessionList(): ProfileSessionList {
  const sessions = [...mock.sessions.entries()].map(([sessionId, session]) => ({
    session_id: sessionId,
    pid: session.pid,
    runtime: MOCK_RUNTIME,
    runtime_version: MOCK_RUNTIME_VERSION,
    started_at: "",
    sample_count: 1,
    duration_secs: 1,
  }));
  return { sessions };
}

function startMemory(): MemoryStartResult {
  const memorySessionId = `${MEMORY_SESSION_PREFIX}-${nextSessionNumber()}`;
  mock.memorySessions.set(memorySessionId, 0);
  const step: MemoryStep = mock.memoryMode === "Courier" ? { kind: "Evaluate", script: "print('start')", output_marker: INGEST_MARKER } : completed(memorySessionId, { kind: "Ack" });
  return { memory_session_id: memorySessionId, runtime: MOCK_RUNTIME, mode: { kind: mock.memoryMode }, step };
}

function handle(method: string, params: Record<string, unknown>): unknown {
  switch (method) {
    case METHODS.PROCESSES: {
      if (mock.failProcesses) {
        mock.failProcesses = false;
        throw new Error("process table unavailable");
      }
      const list: ProcessList = { processes: mock.processes };
      return list;
    }
    case METHODS.KILL: {
      mock.processes = mock.processes.filter((row) => row.pid !== params.pid);
      return { killed: true, pid: params.pid };
    }
    case METHODS.CPU_START: {
      return startCpu(Number(params.pid));
    }
    case METHODS.CPU_STOP: {
      return stopCpu(String(params.session_id));
    }
    case METHODS.CPU_SNAPSHOT: {
      const session = mock.sessions.get(String(params.session_id));
      return session === undefined ? null : report(String(params.session_id), session.pid, SNAPSHOT_SAMPLES);
    }
    case METHODS.CPU_LIST: {
      return sessionList();
    }
    case METHODS.MEMORY_START: {
      return startMemory();
    }
    case METHODS.MEMORY_INGEST: {
      return { memory_session_id: params.memory_session_id, outcome: { kind: "Snapshot", snapshot: snapshot(1), timeline: [], trending_upward: false }, diagnostics: [] };
    }
    default: {
      return memoryStep(method, String(params.memory_session_id));
    }
  }
}

/** One wire process row, when `value` carries the fields a process row must have. */
function isProcessInfo(value: unknown): value is ProcessInfo {
  return isRecord(value) && typeof value.pid === "number" && typeof value.name === "string";
}

/** `value` as wire process rows, or `undefined` when it is not an array. */
function processRows(value: unknown): ProcessInfo[] | undefined {
  return Array.isArray(value) ? value.filter((item) => isProcessInfo(item)) : undefined;
}

/** Apply the steered `lspui.mock.configure` payload. */
function configure(params: Record<string, unknown>): void {
  const failProcesses = booleanField(params, "failProcesses");
  if (failProcesses !== undefined) {
    mock.failProcesses = failProcesses;
  }
  const memoryMode = stringField(params, "memoryMode");
  if (memoryMode === "Direct" || memoryMode === "Courier") {
    mock.memoryMode = memoryMode;
  }
  const hotFile = stringField(params, "hotFile");
  if (hotFile !== undefined) {
    mock.hotFile = hotFile;
  }
  const processes = rawField(params, "processes");
  const rows = processRows(processes);
  if (rows !== undefined) {
    mock.processes = rows;
  }
  if (processes === null) {
    mock.processes = defaultProcesses();
  }
}

connection.onExecuteCommand((request) => {
  const params = asRecord(request.arguments?.[0]);
  if (request.command === `${PREFIX}.mock.configure`) {
    configure(params);
    return { ok: true };
  }
  const method = request.command.replace(`${PREFIX}.`, "").replaceAll(COMMAND_SEPARATOR, "/");
  return handle(method, params);
});

documents.listen(connection);
connection.listen();
