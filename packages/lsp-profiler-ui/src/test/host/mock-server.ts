// A language server that speaks the lsp-profiler wire with canned data.
// Tests [PROF-LSP] from the UI side: every operation is a
// `workspace/executeCommand` under the host prefix, progress arrives as the
// protocol's notification, and the payloads are the generated wire models.
//
// Suites steer it with the `lspui.mock.configure` command: fail the next
// processes fetch, choose a memory mode, or swap the process list.

import {
  createConnection,
  ProposedFeatures,
  TextDocuments,
  type InitializeResult,
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
  StartProfileResult,
} from "../../wire-generated";
import { METHODS } from "../../wire-generated";

const PREFIX = "lspui";
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
  nextId: 1,
};

function defaultProcesses(): ProcessInfo[] {
  return [
    process_(4242, "worker", "mock", "1.0.0", "/usr/bin/mock", "/work/app/worker.txt", true, true),
    process_(4343, "api", "mock", "1.0.0", "/usr/bin/mock", "/work/app/api.txt", false, true),
    process_(4444, "blocked", "mock", null, "/usr/bin/mock", null, false, false, "no diagnostics endpoint"),
  ];
}

function process_(
  pid: number,
  name: string,
  runtime: string,
  version: string | null,
  executable: string,
  entry: string | null,
  inWorkspace: boolean,
  attachable: boolean,
  reason?: string,
): ProcessInfo {
  return {
    pid,
    ppid: 1,
    name,
    runtime,
    ...(version === null ? {} : { runtime_version: version }),
    executable,
    ...(entry === null ? {} : { entry }),
    cpu_percent: pid === 4242 ? 42.5 : 1.5,
    memory_bytes: 64 * 1024 * 1024,
    runtime_secs: 120,
    user: "tester",
    requires_elevation: false,
    in_workspace: inWorkspace,
    attachable,
    ...(reason === undefined ? {} : { unattachable_reason: reason }),
  };
}

function commandNames(): string[] {
  return Object.values(METHODS)
    .filter((method) => method !== METHODS.CPU_PROGRESS)
    .map((method) => `${PREFIX}.${method.replace(/\//g, ".")}`)
    .concat([`${PREFIX}.mock.configure`]);
}

connection.onInitialize((): InitializeResult => ({
  capabilities: {
    executeCommandProvider: { commands: commandNames() },
    experimental: { lspProfiler: { runtimes: [{ id: "mock", display_name: "Mock", source_entries: true, cpu: { live_progress: true, line_level: true, native_frames: false, cooperative: false }, memory: { mode: { kind: mock.memoryMode }, allocation_sites: true, references: true, objects_by_type: true, gc_collect: true } }] } },
  },
}));

function report(sessionId: string, pid: number, samples: number): ProfileReport {
  return {
    session_id: sessionId,
    pid,
    runtime: "mock",
    duration_secs: 1.5,
    total_samples: samples,
    user_samples: samples,
    sample_rate: 100,
    hot_functions: [
      { name: "hot_function", file: mock.hotFile, line: 3, samples: Math.round(samples * 0.8), percentage: 80, self_samples: Math.round(samples * 0.8), self_percentage: 80 },
      { name: "main", file: mock.hotFile, line: 1, samples, percentage: 100, self_samples: 0, self_percentage: 0 },
    ],
    hot_lines: [
      { file: mock.hotFile, line: 4, samples: Math.round(samples * 0.7), percentage: 70 },
      { file: mock.hotFile, line: 5, samples: Math.round(samples * 0.1), percentage: 10 },
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

function memoryStep(method: string, memorySessionId: string): MemoryStep {
  const snapshots = (mock.memorySessions.get(memorySessionId) ?? 0) + 1;
  mock.memorySessions.set(memorySessionId, snapshots);
  switch (method) {
    case METHODS.MEMORY_SNAPSHOT:
      return completed(memorySessionId, { kind: "Snapshot", snapshot: snapshot(snapshots), timeline: [], trending_upward: snapshots > 1 });
    case METHODS.MEMORY_DIFF:
      return completed(memorySessionId, {
        kind: "Diff",
        diff: diff(),
        leaks: [{ file: mock.hotFile, line: 4, type_name: "Payload", size_growth: 500_000, count_growth: 500, current_size: 1_000_000, current_count: 1000, confidence: { kind: "High" }, reason: "grew on every snapshot", traceback: [] }],
      });
    case METHODS.MEMORY_GC_COLLECT:
      return completed(memorySessionId, { kind: "Gc", gc: { collected: 7, uncollectable_count: 1, freed_bytes: 4096, uncollectable: [] } });
    case METHODS.MEMORY_REFERENCES:
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
    case METHODS.MEMORY_OBJECTS_BY_TYPE:
      return completed(memorySessionId, {
        kind: "Objects",
        objects: { type_name: "Payload", total_count: 1000, total_bytes: 32_000, objects: [{ id: "0x1", type_name: "Payload", size_bytes: 32, label: "0x1" }], type_summary: [{ type_name: "Payload", count: 1000, size_bytes: 32_000 }] },
      });
    default:
      mock.memorySessions.delete(memorySessionId);
      return completed(memorySessionId, { kind: "Ack" });
  }
}

function startCpu(pid: number): StartProfileResult {
  const sessionId = `cpu-${mock.nextId++}`;
  const started = Date.now();
  const timer = setInterval(() => {
    const progress: ProfileProgress = { session_id: sessionId, sample_count: Math.round((Date.now() - started) / 10), duration_secs: (Date.now() - started) / 1000, top_function: "hot_function" };
    void connection.sendNotification(METHODS.CPU_PROGRESS, progress);
  }, 200);
  mock.sessions.set(sessionId, { pid, started, timer });
  return { session_id: sessionId, pid, runtime: "mock", runtime_version: "1.0.0", started_at: new Date(started).toISOString() };
}

function stopCpu(sessionId: string): ProfileReport {
  const session = mock.sessions.get(sessionId);
  if (session === undefined) {
    throw new Error(`unknown session ${sessionId}`);
  }
  clearInterval(session.timer);
  mock.sessions.delete(sessionId);
  return report(sessionId, session.pid, Math.max(10, Math.round((Date.now() - session.started) / 10)));
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
    case METHODS.KILL:
      mock.processes = mock.processes.filter((p) => p.pid !== params.pid);
      return { killed: true, pid: params.pid };
    case METHODS.CPU_START:
      return startCpu(Number(params.pid));
    case METHODS.CPU_STOP:
      return stopCpu(String(params.session_id));
    case METHODS.CPU_SNAPSHOT: {
      const session = mock.sessions.get(String(params.session_id));
      return session === undefined ? null : report(String(params.session_id), session.pid, 50);
    }
    case METHODS.CPU_LIST:
      return { sessions: [...mock.sessions.entries()].map(([id, s]) => ({ session_id: id, pid: s.pid, runtime: "mock", runtime_version: "1.0.0", started_at: "", sample_count: 1, duration_secs: 1 })) };
    case METHODS.MEMORY_START: {
      const memorySessionId = `mem-${mock.nextId++}`;
      mock.memorySessions.set(memorySessionId, 0);
      const step: MemoryStep = mock.memoryMode === "Courier" ? { kind: "Evaluate", script: "print('start')", output_marker: "__MOCK__" } : completed(memorySessionId, { kind: "Ack" });
      const result: MemoryStartResult = { memory_session_id: memorySessionId, runtime: "mock", mode: { kind: mock.memoryMode }, step };
      return result;
    }
    case METHODS.MEMORY_INGEST:
      return { memory_session_id: params.memory_session_id, outcome: { kind: "Snapshot", snapshot: snapshot(1), timeline: [], trending_upward: false }, diagnostics: [] };
    default:
      return memoryStep(method, String(params.memory_session_id));
  }
}

connection.onExecuteCommand((request) => {
  const params = (request.arguments?.[0] ?? {}) as Record<string, unknown>;
  if (request.command === `${PREFIX}.mock.configure`) {
    if (typeof params.failProcesses === "boolean") mock.failProcesses = params.failProcesses;
    if (params.memoryMode === "Direct" || params.memoryMode === "Courier") mock.memoryMode = params.memoryMode;
    if (typeof params.hotFile === "string") mock.hotFile = params.hotFile;
    if (Array.isArray(params.processes)) mock.processes = params.processes as ProcessInfo[];
    if (params.processes === null) mock.processes = defaultProcesses();
    return { ok: true };
  }
  const method = request.command.replace(`${PREFIX}.`, "").replace(/\./g, "/");
  return handle(method, params);
});

documents.listen(connection);
connection.listen();
