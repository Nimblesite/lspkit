// Tests for [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
//
// Issue #148: the Processes panel must be a pure projection of
// centralised store Signals — no panel-local timer, no panel-owned data. These
// tests drive the store's `processes` Signal and assert the tree re-renders
// reactively through the production wiring (`subscribeRevision` over
// `store.processesRevision`, the exact subscription registerPythonProcesses
// installs), that view state (sort/group/filter/debuggee) is shared by every
// subscriber, and that the poll feeding the store lives store-side
// (process-poll.ts), gated on view visibility.

import * as assert from "node:assert";
import * as vscode from "vscode";
import type { ProcessInfo } from "../../process-explorer";
import { ProcessesProvider } from "../../process-explorer";
import { bindProcessPolling, fetchProcessesIntoStore } from "../../process-poll";
import { subscribeRevision } from "../../reactive-refresh";
import type { ProfilerStore as Store } from "../../store";
import { createProfilerStore } from "../../store";
import { fakeHost, fakeLanguageClient, nextMicrotask, pollUntilResult, seedSignal, toWireProcess, ungroup } from "./test-helpers";
import { numberField, rawField } from "../../unknown-shape";

const MB = 1024 * 1024;
/** How long a fire-and-forget fetch may take to land in the store. */
const FETCH_SETTLE_WAIT_MS = 2000;
const FETCH_SETTLE_POLL_MS = 10;

/** A minimal, debuggable process row. */
function proc(pid: number, overrides: Partial<ProcessInfo> = {}): ProcessInfo {
  return {
    pid, ppid: 1, name: `python-${pid}`, interpreterPath: "/usr/bin/python3.12",
    script: `/app/p${pid}.py`, runtime: "python", runtimeVersion: "3.12.1", cpuPercent: pid, memoryBytes: pid * MB,
    runtimeSecs: 10, user: "alice", requiresElevation: false,
    inWorkspace: false, launcher: null, debuggable: true, undebuggableReason: null,
    ...overrides,
  };
}

/**
 * A client stand-in whose `sendRequest` answers with `answer()`.
 *
 * No runtime check can produce the rest of `LanguageClient`, so the stand-in
 * is built through `fakeLanguageClient`, whose type guard is the one licensing
 * point — it is the test double itself, not a payload being read.
 */
function clientAnswering(answer: () => unknown): ReturnType<typeof fakeLanguageClient> {
  return fakeLanguageClient({
    isRunning: (): boolean => true,
    onDidChangeState: (): vscode.Disposable => ({ dispose: (): undefined => undefined }),
    sendRequest: async (): Promise<unknown> => {
      await nextMicrotask();
      return answer();
    },
  });
}

/** A store whose fake LSP client serves the given process table. */
function storeServing(processes: readonly ProcessInfo[]): Store {
  const store = createProfilerStore(fakeHost());
  const rows = processes.map((row) => toWireProcess(row));
  seedSignal(store.client, clientAnswering(() => ({ processes: rows })));
  return store;
}

/** A flat provider wired to the store exactly as registerPythonProcesses wires it. */
function subscribedProvider(store: Store): ProcessesProvider {
  ungroup(store);
  const provider = new ProcessesProvider(store);
  subscribeRevision(store.processesRevision, provider);
  return provider;
}

/** The PIDs of the plain process rows in a root listing. */
function pidsOf(rows: vscode.TreeItem[]): number[] {
  // `process` is attached to the row by the provider under test, not by
  // `vscode.TreeItem`, so it is read back by name rather than asserted on.
  return rows
    .map((row) => numberField(rawField(row, "process"), "pid"))
    .filter((pid): pid is number => pid !== undefined);
}

/** `pids` in ascending order, leaving `pids` itself untouched. */
function sortedAscending(pids: number[]): number[] {
  const ordered = [...pids];
  ordered.sort((left, right) => left - right);
  return ordered;
}

suite("Processes — pure projection of store Signals (#148)", () => {
  test("a store bump repaints the tree through the production subscription — no fetch, no timer", () => {
    // Deliberately NO LSP client: if the panel needed to fetch anything itself,
    // this test could not render a single row.
    const store = createProfilerStore(fakeHost());
    const provider = subscribedProvider(store);
    try {
      let repaints = 0;
      provider.disposables.push(provider.onDidChangeTreeData(() => { repaints += 1; }));

      assert.deepStrictEqual(provider.getChildren(), [], "nothing to render before the store holds data");

      store.processesLoaded([proc(7)]);
      assert.strictEqual(repaints, 1, "the revision bump must repaint the subscribed tree");
      assert.deepStrictEqual(pidsOf(provider.getChildren()), [7], "the tree renders exactly the store's list");

      store.processesLoaded([]);
      assert.strictEqual(repaints, 2, "clearing the store repaints again");
      assert.deepStrictEqual(provider.getChildren(), [], "an emptied store empties the tree");
    } finally {
      provider.dispose();
    }
  });

  test("the provider owns no data: a second panel over the same store renders the fetched list without fetching", async () => {
    const store = storeServing([proc(1), proc(2)]);
    const fetcher = subscribedProvider(store);
    // Never fetches.
    const observer = subscribedProvider(store);
    try {
      await fetcher.refreshNow();
      const observerRows = observer.getChildren();
      assert.deepStrictEqual(
        sortedAscending(pidsOf(observerRows)),
        [1, 2],
        "a panel that never fetched renders the centralised list — the data lives in the store",
      );
    } finally {
      fetcher.dispose();
      observer.dispose();
    }
  });

  test("view state (sort, filter, debuggee) is centralised — one panel's change drives every subscriber", async () => {
    const store = storeServing([proc(10, { cpuPercent: 1, memoryBytes: 99 * MB }), proc(20, { cpuPercent: 50, memoryBytes: MB })]);
    const panelA = subscribedProvider(store);
    const panelB = subscribedProvider(store);
    try {
      await panelA.refreshNow();
      assert.deepStrictEqual(pidsOf(panelB.getChildren()), [20, 10], "default sort: CPU descending");

      // CPU → memory.
      panelA.cycleSortMode();
      assert.deepStrictEqual(
        pidsOf(panelB.getChildren()),
        [10, 20],
        "panel A's sort change re-orders panel B — the mode lives in the store, not the panel",
      );

      panelA.setFilter("p20");
      assert.deepStrictEqual(pidsOf(panelB.getChildren()), [20], "the filter is centralised too");
      panelA.setFilter("");

      store.setActiveDebuggeePid(20);
      const debuggeeRow = panelB.getChildren().find(
        (row) => numberField(rawField(row, "process"), "pid") === 20,
      );
      assert.strictEqual(
        debuggeeRow?.contextValue,
        "processDebuggee",
        "the debuggee marker is a store signal every panel projects",
      );
    } finally {
      panelA.dispose();
      panelB.dispose();
    }
  });

  test("the poll lives store-side: binding a visible view fetches into the store immediately", async () => {
    const store = storeServing([proc(42)]);
    const visibility = new vscode.EventEmitter<vscode.TreeViewVisibilityChangeEvent>();
    const polling = bindProcessPolling(store, { visible: true, onDidChangeVisibility: visibility.event });
    try {
      // The immediate fetch is fire-and-forget; wait for the signal to settle.
      await pollUntilResult({
        fn: () => store.processes.value.fetch,
        until: (fetchState) => fetchState === "loaded",
        timeoutMs: FETCH_SETTLE_WAIT_MS,
        intervalMs: FETCH_SETTLE_POLL_MS,
        description: "the store-side poll to fetch on bind",
      });
      assert.strictEqual(store.processes.value.fetch, "loaded", "the store-side poll must fetch on bind");
      assert.deepStrictEqual(store.processes.value.list.map((row) => row.pid), [42], "the fetch landed in the store signal");
    } finally {
      polling.dispose();
      visibility.dispose();
    }
  });

  test("every fetch outcome lands honestly in the store (#147 via the store path)", async () => {
    // No client → still loading (never "no processes").
    const bare = createProfilerStore(fakeHost());
    await fetchProcessesIntoStore(bare);
    assert.strictEqual(bare.processes.value.fetch, "loading");

    // Failing client → error, and any stale rows are dropped.
    const failing = createProfilerStore(fakeHost());
    seedSignal(failing.client, clientAnswering((): never => { throw new Error("disconnected"); }));
    failing.processesLoaded([proc(1)]);
    await fetchProcessesIntoStore(failing);
    assert.strictEqual(failing.processes.value.fetch, "error");
    assert.deepStrictEqual(failing.processes.value.list, [], "a failed fetch never leaves stale rows on screen");
  });
});
