// Tests for [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
//
// Component tests for the Processes panel (#62). Per CLAUDE.md these
// assert behavior through internal VSIX state — instantiate the provider, feed
// it a stubbed ProcessInfo[] via a fake LSP client, and assert getChildren()
// yields the expected sorted/grouped/filtered rows and that each row carries
// the PID a one-click profiling action needs. No getCommands()/whenCommandReady.


import * as assert from "assert";
import * as vscode from "vscode";
import { ProcessDecorationProvider, ProcessesProvider } from "../../process-explorer";
import { createProcessRowActions, memoryTrackRoute } from "../../process-launch";
import { type ProfilerStore as Store } from "../../store";
import { seedSignal } from "./test-helpers";
import {
  MACHINERY,
  type RecordedRequest,
  STUB_PROCESSES,
  colorId,
  iconId,
  labelText,
  loadedProvider,
  memberPids,
  pidOf,
  processRows,
  profilerStarts,
  resourceUri,
  startPid,
  storeWith,
  tooltipText,
} from "./process-explorer-fixtures";

suite("Processes Panel", () => {
  let provider: ProcessesProvider;

  teardown(() => {
    provider.dispose();
  });

  test("lists every process sorted by CPU descending by default", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    const rows = processRows(provider.getChildren());
    assert.deepStrictEqual(rows.map(pidOf), [200, 100, 300], "CPU 42 > 5 > 1");
  });

  test("opens grouped by runtime version; the group action cycles through flat and back", async () => {
    const store = storeWith(STUB_PROCESSES);
    provider = new ProcessesProvider(store);
    await provider.refreshNow();
    assert.strictEqual(store.processes.value.groupMode, "version", "the default grouping is the runtime version");
    const groups = processRows(provider.getChildren());
    assert.deepStrictEqual(groups.map(labelText), ["3.11.7", "3.12.1"], "one header per version, sorted");
    assert.deepStrictEqual(groups.map((group) => String(group.description)), ["1", "2"], "each header counts its members");
    const twelve = groups.find((group) => labelText(group) === "3.12.1");
    assert.ok(twelve, "3.12.1 group must exist");
    assert.deepStrictEqual((provider.getChildren(twelve)).map(pidOf), [100, 300], "members stay CPU-ordered");

    const modes = [1, 2, 3, 4].map(() => {
      provider.cycleGroupMode();
      return store.processes.value.groupMode;
    });
    assert.deepStrictEqual(modes, ["interpreter", "user", "parent", "none"], "every grouping, then flat");
    assert.deepStrictEqual(processRows(provider.getChildren()).map(pidOf), [200, 100, 300], "flat rows, CPU-ordered");

    provider.cycleGroupMode();
    assert.strictEqual(store.processes.value.groupMode, "version", "one more press returns to the default");
    assert.deepStrictEqual(processRows(provider.getChildren()).map(labelText), ["3.11.7", "3.12.1"], "regrouped");
  });

  test("each row carries its PID so inline Profile starts with no input box", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    const rows = processRows(provider.getChildren());
    for (const row of rows) {
      assert.strictEqual(typeof pidOf(row), "number", "row must carry a numeric pid for the command arg");
    }
    const worker = rows.find((r) => pidOf(r) === 200);
    assert.ok(worker, "the worker process row must exist");
    assert.ok(
      String(worker.description).includes("PID 200"),
      `row description should surface the PID: ${String(worker.description)}`,
    );
  });

  test("rows needing elevation get a distinct contextValue for the lock affordance", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    const rows = provider.getChildren();
    const elevated = rows.find((r) => pidOf(r) === 200);
    const normal = rows.find((r) => pidOf(r) === 100);
    assert.strictEqual(elevated?.contextValue, "processElevated");
    assert.strictEqual(normal?.contextValue, "process");
  });

  test("sort by memory orders rows by resident size descending", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    provider.cycleSortMode(); // cpu → memory
    const rows = processRows(provider.getChildren());
    assert.deepStrictEqual(rows.map(pidOf), [300, 100, 200], "memory 99 > 50 > 10 MB");
  });

  test("group by runtime version buckets processes under collapsible headers", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    provider.cycleGroupMode(); // none → version
    const groups = processRows(provider.getChildren());
    assert.deepStrictEqual(
      groups.map(labelText),
      ["3.11.7", "3.12.1"],
      "groups are sorted by version label",
    );
    const twelve = groups.find((g) => labelText(g) === "3.12.1");
    assert.ok(twelve, "3.12.1 group must exist");
    assert.strictEqual(String(twelve.description), "2", "group shows its member count");

    const members = provider.getChildren(twelve);
    assert.deepStrictEqual(members.map(pidOf), [100, 300], "both 3.12 processes, CPU-ordered");
  });

  test("filter narrows rows by name, script, or PID substring", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    provider.setFilter("worker");
    let rows = processRows(provider.getChildren());
    assert.deepStrictEqual(rows.map(pidOf), [200], "only worker.py matches");

    provider.setFilter("300");
    rows = processRows(provider.getChildren());
    assert.deepStrictEqual(rows.map(pidOf), [300], "PID substring matches");
  });

  // procexp-2: VS Code shows the "No processes running" welcome whenever
  // getChildren returns []. When a filter hides a NON-empty process list, the
  // tree must NOT be empty — it must say processes are running but filtered
  // (the pinned launch rows stay too).
  test("a filter that hides every running process shows an honest placeholder, not 'no processes' (procexp-2)", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    provider.setFilter("nonexistent-zzz");
    const nonAction = processRows(provider.getChildren());
    assert.strictEqual(nonAction.length, 1, "must return a placeholder row, not an empty list that triggers the welcome");
    assert.strictEqual(nonAction[0].contextValue, "processesMessage", "the row is a non-process placeholder");
    const label = labelText(nonAction[0]);
    assert.ok(
      label.includes("nonexistent-zzz") && label.includes("3 running"),
      `the placeholder must explain the filter hid running processes: ${label}`,
    );
  });

  test("group members expose the full member set for the count badge", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    provider.cycleGroupMode();
    const groups = provider.getChildren();
    const eleven = groups.find((g) => labelText(g) === "3.11.7");
    assert.ok(eleven, "3.11.7 group must exist");
    assert.deepStrictEqual(memberPids(eleven), [200]);
  });
});

// Tests for [PROF-UI-PROCESSES] issue #79: the inline flame/database
// buttons arrive with `item === undefined` at runtime and must still profile
// the row the user clicked instead of warning "Select a Python process".
suite("Processes Panel — inline launch actions (#79)", () => {
  let provider: ProcessesProvider;

  teardown(() => {
    provider.dispose();
  });

  test("rows keep a stable id across refreshes so inline buttons survive the auto-refresh", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    const before = provider.getChildren();
    provider.refresh();
    const after = provider.getChildren();

    const beforeRow = before.find((row) => pidOf(row) === 200);
    const afterRow = after.find((row) => pidOf(row) === 200);
    assert.ok(beforeRow !== undefined && afterRow !== undefined, "PID 200 row must exist in both passes");
    assert.ok(
      typeof beforeRow.id === "string" && beforeRow.id.length > 0,
      "process rows must carry a stable TreeItem.id so VS Code can map an inline click " +
        `back to the element after a 2s auto-refresh (#79); got: ${String(beforeRow.id)}`,
    );
    assert.strictEqual(beforeRow.id, afterRow.id, "the id must be identical across refreshes");
  });

  test("inline Profile CPU invoked without an argument profiles the selected row", async () => {
    const requests: RecordedRequest[] = [];
    const store = storeWith(STUB_PROCESSES, requests);
    provider = await loadedProvider(store);
    const rows = provider.getChildren();
    const selectedRow = rows.find((row) => pidOf(row) === 200);
    assert.ok(selectedRow !== undefined, "PID 200 row must exist");

    const actions = createProcessRowActions(store, { selection: [selectedRow] });
    // VS Code passed no argument — the runtime shape of issue #79.
    await actions.profileProcess(undefined);

    const starts = profilerStarts(requests);
    assert.strictEqual(
      starts.length,
      1,
      "clicking the inline flame button must start profiling (not warn) when a row is selected (#79)",
    );
    assert.strictEqual(startPid(starts[0]), 200, "profiling must target the selected row's PID");
  });

  test("an explicitly passed row wins over a different selection", async () => {
    const requests: RecordedRequest[] = [];
    const store = storeWith(STUB_PROCESSES, requests);
    provider = await loadedProvider(store);
    const rows = provider.getChildren();
    const clicked = rows.find((row) => pidOf(row) === 300);
    const selected = rows.find((row) => pidOf(row) === 200);
    assert.ok(clicked !== undefined && selected !== undefined, "both rows must exist");

    const actions = createProcessRowActions(store, { selection: [selected] });
    await actions.profileProcess(clicked);

    const starts = profilerStarts(requests);
    assert.strictEqual(starts.length, 1, "the clicked row must be profiled");
    assert.strictEqual(startPid(starts[0]), 300, "the explicit item must win over the selection");
  });

  test("with no item and no selection, nothing is profiled", async () => {
    const requests: RecordedRequest[] = [];
    const store = storeWith(STUB_PROCESSES, requests);
    provider = await loadedProvider(store);
    provider.getChildren();

    const actions = createProcessRowActions(store, { selection: [] });
    await actions.profileProcess(undefined);

    assert.strictEqual(
      profilerStarts(requests).length,
      0,
      "without any resolvable target the action must not fire a profiler.start",
    );
  });
});

suite("Processes Panel — Track Memory routing", () => {
  // Tests for the memory leg of [PROF-UI-PROCESSES]: tracemalloc rides
  // the DAP courier, so the row action may only ever target the live debuggee.

  /** Drive the row's Track Memory action against PID 100 with the given session. */
  async function trackMemoryOnPid100(
    session: { id: string; type: string } | undefined,
    arrange: (store: Store) => void = () => undefined,
  ): Promise<{ requests: RecordedRequest[]; executed: string[] }> {
    const requests: RecordedRequest[] = [];
    const executed: string[] = [];
    const store = storeWith(STUB_PROCESSES, requests);
    arrange(store);
    const rows = (await loadedProvider(store)).getChildren();
    const selectedRow = rows.find((row) => pidOf(row) === 100);
    assert.ok(selectedRow !== undefined, "PID 100 row must exist");

    const actions = createProcessRowActions(store, { selection: [selectedRow] }, {
      runCommand: async (command) => { executed.push(command); },
      activeSession: () => session,
    });
    await actions.memoryTrackProcess(undefined);
    return { requests, executed };
  }

  test("on the live debuggee it routes to real memory tracking — never a CPU start", async () => {
    const { requests, executed } = await trackMemoryOnPid100(
      { id: "session-1", type: "lspui-debug" },
      (store) => { seedSignal(store.sessionIdToPid, new Map([["session-1", 100]])); },
    );

    assert.deepStrictEqual(
      executed,
      ["lspui.profiler.memoryStart"],
      "Track Memory on the debuggee row must start tracemalloc tracking",
    );
    assert.strictEqual(
      profilerStarts(requests).length,
      0,
      "Track Memory must NEVER start a CPU profiling session (the preset:'memory' defect)",
    );
  });

  test("on an external process it starts nothing and offers the launch flow", async () => {
    // No debug session at all — PID 100 is a foreign process.
    const { requests, executed } = await trackMemoryOnPid100(undefined);

    assert.deepStrictEqual(executed, [], "no memory command can run against a foreign PID");
    assert.strictEqual(
      profilerStarts(requests).length,
      0,
      "an external row must not silently fall back to CPU profiling",
    );
  });

  test("memoryTrackRoute targets the debuggee only when session and PID both match", () => {
    const store = storeWith(STUB_PROCESSES);
    seedSignal(store.sessionIdToPid, new Map([["session-1", 100]]));
    const basilisk = { id: "session-1", type: "lspui-debug" };

    assert.strictEqual(memoryTrackRoute(store, 100, basilisk), "start-tracking");
    assert.strictEqual(memoryTrackRoute(store, 200, basilisk), "offer-launch", "PID mismatch");
    assert.strictEqual(memoryTrackRoute(store, 100, undefined), "offer-launch", "no session");
    assert.strictEqual(
      memoryTrackRoute(store, 100, { id: "session-1", type: "python" }),
      "offer-launch",
      "foreign debug adapter",
    );
  });
});

// Tests for [PROF-UI-PROCESSES] / [PROF-PROCESSES-SCOPE]: the panel
// shows EVERY process (zero filters) and renders cues — launcher chips, a green
// workspace row, and a 🚫 / greyed / sunk row for anything it can't profile.
suite("Processes Panel — zero-filter display cues", () => {
  let provider: ProcessesProvider;
  teardown(() => { provider.dispose(); });

  test("launchers are always listed and carry a framework chip (zero filters)", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    const rows = provider.getChildren();
    const uvicorn = rows.find((r) => pidOf(r) === 300);
    assert.ok(uvicorn, "the uvicorn launcher must always be listed — nothing is hidden");
    assert.ok(
      String(uvicorn.description).includes("[uvicorn]"),
      `the launcher framework must render as a chip: ${String(uvicorn.description)}`,
    );
  });

  test("a workspace process resolves to a green decoration; an outside one does not", async () => {
    provider = await loadedProvider(storeWith(STUB_PROCESSES));
    const decorations = new ProcessDecorationProvider(provider);
    try {
      const rows = provider.getChildren();
      const inside = rows.find((r) => pidOf(r) === 100); // inWorkspace: true
      const outside = rows.find((r) => pidOf(r) === 300); // inWorkspace: false, debuggable
      assert.ok(inside && outside, "both the workspace and outside rows must exist");
      const insideUri = resourceUri(inside);
      const outsideUri = resourceUri(outside);
      assert.ok(insideUri && outsideUri, "process rows must carry a resourceUri for decoration");

      const insideDeco = decorations.provideFileDecoration(insideUri);
      assert.strictEqual(colorId(insideDeco?.color), "charts.green", "a workspace row must be green");
      const outsideDeco = decorations.provideFileDecoration(outsideUri);
      assert.strictEqual(outsideDeco, undefined, "a non-workspace debuggable row keeps the default colour");

      // A non-process URI is ignored, and a tree refresh re-fires decorations.
      assert.strictEqual(
        decorations.provideFileDecoration(vscode.Uri.file("/tmp/unrelated")),
        undefined,
        "URIs from other schemes are not decorated",
      );
      let fired = false;
      const sub = decorations.onDidChangeFileDecorations(() => { fired = true; });
      provider.refresh();
      sub.dispose();
      assert.ok(fired, "a tree refresh must re-fire decorations so colours never go stale");
    } finally {
      decorations.dispose();
    }
  });

  test("a non-debuggable process is 🚫-marked, greyed, and sorted to the bottom", async () => {
    // MACHINERY has the highest CPU (99%) but must still sink below the others.
    provider = await loadedProvider(storeWith([MACHINERY, ...STUB_PROCESSES]));
    const decorations = new ProcessDecorationProvider(provider);
    try {
      const rows = provider.getChildren();
      assert.strictEqual(
        pidOf(rows[rows.length - 1]),
        900,
        "the non-debuggable row sinks to the bottom despite the highest CPU",
      );
      const machineryRow = rows.find((r) => pidOf(r) === 900);
      assert.ok(machineryRow, "the machinery process must still be LISTED, not hidden");
      assert.ok(
        labelText(machineryRow).startsWith("🚫"),
        `a non-debuggable row must be prefixed with 🚫: ${labelText(machineryRow)}`,
      );
      assert.strictEqual(iconId(machineryRow), "circle-slash", "non-debuggable icon is circle-slash");

      const machineryUri = resourceUri(machineryRow);
      assert.ok(machineryUri, "the machinery row must carry a resourceUri");
      const deco = decorations.provideFileDecoration(machineryUri);
      assert.strictEqual(colorId(deco?.color), "disabledForeground", "a non-debuggable row is greyed");
      assert.ok(
        tooltipText(machineryRow).includes("debugger machinery"),
        `the tooltip must explain why it can't be profiled: ${tooltipText(machineryRow)}`,
      );
    } finally {
      decorations.dispose();
    }
  });
});

// Icons, decoration precedence, tooltip detail, and within-group sinking —
// [PROF-UI-PROCESSES] (R4, R5, R6, R8).