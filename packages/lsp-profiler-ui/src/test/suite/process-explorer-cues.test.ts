// Tests for [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
//
// The Processes panel's display cues, pinned launches and inline action
// targets, through the real provider over the shared fixtures.

import * as assert from "node:assert";
import * as vscode from "vscode";
import { ProcessDecorationProvider } from "../../process-explorer";
import type { ProcessInfo } from "../../process-explorer";
import { createProcessRowActions } from "../../process-launch";
import { seedSignal } from "./test-helpers";
import type { RecordedRequest } from "./process-explorer-fixtures";
import {
  MACHINERY,
  MB,
  STUB_PROCESSES,
  actionRows,
  colorId,
  disposeProviders,
  iconId,
  labelText,
  loadedProvider,
  pidOf,
  processRows,
  profilerStarts,
  recordingRunner,
  resourceUri,
  startPid,
  storeWith,
  tooltipText,
  tracked,
} from "./process-explorer-fixtures";

/** The commands the pinned launch-action rows carry, in row order. */
function commandsOf(rows: vscode.TreeItem[]): (string | undefined)[] {
  return actionRows(rows).map((row) => row.command?.command);
}

/** Run `fn` with showWarningMessage stubbed, returning the captured warnings. */
async function captureWarnings(fn: () => Promise<void>): Promise<string[]> {
  const warnings: string[] = [];
  const original = vscode.window.showWarningMessage;
  (vscode.window as { showWarningMessage: unknown }).showWarningMessage = async (
    message: string,
  ): Promise<void> => {
    warnings.push(message);
    await Promise.resolve();
  };
  try {
    await fn();
  } finally {
    (vscode.window as { showWarningMessage: unknown }).showWarningMessage = original;
  }
  return warnings;
}

suite("Processes Panel — display cues: icons, precedence, tooltip, grouping", () => {
  teardown(disposeProviders);

  test("each process state renders its own info icon (R4)", async () => {
    const plain: ProcessInfo = { ...STUB_PROCESSES[0], pid: 111, inWorkspace: false };
    const store = storeWith([MACHINERY, plain, ...STUB_PROCESSES]);
    // Mark PID 100 as actively profiled (store-derived, #148).
    store.profilerActive(100, "sess-cpu");
    const provider = tracked(await loadedProvider(store));
    const rows = provider.getChildren();
    const icons = new Map(rows.map((row) => [pidOf(row), iconId(row)]));
    assert.strictEqual(icons.get(100), "flame", "the actively-profiled row shows the flame");
    assert.strictEqual(icons.get(200), "lock", "an elevation row stays debuggable but shows the lock");
    assert.strictEqual(icons.get(300), "rocket", "a launcher row shows the rocket");
    assert.strictEqual(icons.get(111), "vm-running", "a plain interpreter shows the running-VM glyph");
    assert.strictEqual(icons.get(900), "circle-slash", "a non-debuggable row shows circle-slash");
  });

  test("greying wins over green for a non-debuggable workspace process (R5 > R6)", async () => {
    const wsMachinery: ProcessInfo = { ...MACHINERY, inWorkspace: true };
    const provider = tracked(await loadedProvider(storeWith([wsMachinery])));
    const decorations = new ProcessDecorationProvider(provider);
    try {
      const processRow = processRows(provider.getChildren()).find((row) => pidOf(row) === 900);
      assert.ok(processRow, "the process row must exist");
      const uri = resourceUri(processRow);
      assert.ok(uri, "the row must carry a resourceUri");
      const deco = decorations.provideFileDecoration(uri);
      assert.strictEqual(
        colorId(deco?.color),
        "disabledForeground",
        "a workspace process you can't debug must be greyed, not green",
      );
    } finally {
      decorations.dispose();
    }
  });

  test("the tooltip surfaces every resolved detail (R8)", async () => {
    const rich: ProcessInfo = {
      pid: 555, ppid: 1, name: "python3.12", interpreterPath: "/usr/bin/python3.12",
      script: "/app/svc.py", runtime: "python", runtimeVersion: "3.12.1", cpuPercent: 7, memoryBytes: 12 * MB,
      runtimeSecs: 65, user: "carol", requiresElevation: false,
      inWorkspace: true, launcher: "gunicorn", debuggable: true, undebuggableReason: null,
    };
    const provider = tracked(await loadedProvider(storeWith([rich])));
    const processRow = processRows(provider.getChildren()).find((row) => pidOf(row) === 555);
    assert.ok(processRow, "the process row must exist");
    const tip = tooltipText(processRow);
    for (const needle of [
      "PID 555", "Executable: /usr/bin/python3.12", "Entry: /app/svc.py",
      "CPython: 3.12.1", "Runtime:", "User: carol", "Launcher: gunicorn", "Workspace",
    ]) {
      assert.ok(tip.includes(needle), `tooltip must surface "${needle}": ${tip}`);
    }
  });

  test("a row opens its entry on click only when the runtime advertises source entries", async () => {
    const script: ProcessInfo = {
      pid: 557, ppid: 1, name: "python3", interpreterPath: "/usr/bin/python3", script: "/app/run.py",
      runtime: "python", runtimeVersion: "3.12.1", cpuPercent: 0, memoryBytes: MB, runtimeSecs: 1,
      user: null, requiresElevation: false, inWorkspace: false, launcher: null, debuggable: true, undebuggableReason: null,
    };
    const binary: ProcessInfo = { ...script, pid: 558, name: "App", interpreterPath: "/app/App", script: "/app/App", runtime: "dotnet" };
    const provider = tracked(await loadedProvider(storeWith([script, binary])));
    const rows = processRows(provider.getChildren());
    const opens = rows.find((row) => pidOf(row) === 557)?.command;
    assert.strictEqual(opens?.command, "vscode.open", "a source entry opens in the editor");
    assert.strictEqual(String(opens?.arguments?.[0]), vscode.Uri.file("/app/run.py").toString());
    assert.strictEqual(
      rows.find((row) => pidOf(row) === 558)?.command,
      undefined,
      "a runtime that never advertised source entries never opens its (binary) entry",
    );
  });

  test("a runtime the server never advertised is labelled by its wire id, not a guessed name", async () => {
    const odd: ProcessInfo = {
      pid: 556, ppid: 1, name: "dotnet", interpreterPath: null, script: null,
      runtime: "dotnet", runtimeVersion: "8.0.1", cpuPercent: 0, memoryBytes: MB,
      runtimeSecs: 1, user: null, requiresElevation: false,
      inWorkspace: false, launcher: null, debuggable: true, undebuggableReason: null,
    };
    const provider = tracked(await loadedProvider(storeWith([odd])));
    const processRow = processRows(provider.getChildren()).find((row) => pidOf(row) === 556);
    assert.ok(processRow, "the process row must exist");
    const tip = tooltipText(processRow);
    assert.ok(tip.includes("dotnet: 8.0.1"), `the wire id labels the version: ${tip}`);
    assert.ok(!tip.includes("Dotnet:"), `no capitalised guess: ${tip}`);
  });

  test("when grouped, a non-debuggable process sinks within its group (R5)", async () => {
    // Machinery that shares 100 and 300's runtime-version group.
    const machinery: ProcessInfo = { ...MACHINERY, runtime: "python", runtimeVersion: "3.12.1" };
    const provider = tracked(await loadedProvider(storeWith([machinery, ...STUB_PROCESSES])));
    // Cycle: none → version.
    provider.cycleGroupMode();
    const groups = provider.getChildren();
    const twelve = groups.find((group) => labelText(group) === "3.12.1");
    assert.ok(twelve, "the 3.12.1 group must exist");
    const members = provider.getChildren(twelve);
    assert.deepStrictEqual(
      members.map((row) => pidOf(row)),
      [100, 300, 900],
      "debuggable rows first (CPU-ordered), the non-debuggable one sinks last within the group",
    );
  });
});

// The big "Run & …(Current File)" buttons can't live in viewsWelcome once the
// tree is populated (VS Code renders welcome only for an EMPTY view), so they are
// pinned as rows at the top — gated per activity. [PROF-UI-PROCESSES]
// / [PROF-UI-PROCESSES].
suite("Processes Panel — pinned launch buttons", () => {
  teardown(disposeProviders);

  test("the current-file launches are pinned above the process rows even when a process is listed", async () => {
    const provider = tracked(await loadedProvider(storeWith(STUB_PROCESSES)));
    const rows = provider.getChildren();
    assert.deepStrictEqual(
      commandsOf(rows),
      ["lspui.profiler.profileCurrentFileCpu", "lspui.profiler.trackMemoryCurrentFile"],
      "both launches must be pinned, CPU then memory",
    );
    assert.strictEqual(rows[0].contextValue, "launchAction", "a launch row is first");
    assert.ok(processRows(rows).length > 0, "the process rows still follow the launches");
  });

  test("a busy metric hides ITS launch row but leaves the other (both: CPU during memory, memory during CPU)", async () => {
    const store = storeWith(STUB_PROCESSES);
    const provider = tracked(await loadedProvider(store));

    // CPU busy.
    store.profilerActive(4242, "sess-cpu");
    assert.deepStrictEqual(
      commandsOf(provider.getChildren()),
      ["lspui.profiler.trackMemoryCurrentFile"],
      "while CPU profiles, the CPU launch is hidden but the memory launch remains",
    );

    store.profilerStopped();
    // Memory busy.
    store.memoryTrackingActive("sess-mem");
    assert.deepStrictEqual(
      commandsOf(provider.getChildren()),
      ["lspui.profiler.profileCurrentFileCpu"],
      "while memory tracks, the memory launch is hidden but the CPU launch remains",
    );
    store.memoryTrackingStopped();
  });

  test("with no processes the tree is empty so the viewsWelcome big buttons render", async () => {
    const provider = tracked(await loadedProvider(storeWith([])));
    assert.deepStrictEqual(
      provider.getChildren(),
      [],
      "an empty process list defers to the welcome buttons rather than pinning rows",
    );
  });
});

// Memory tracking can only target the active LspUi debuggee, so the panel
// reveals the inline Track Memory action on that row alone and warns elsewhere —
// answering "why not grey it out beforehand?" ([PROF-UI-PROCESSES]).
suite("Processes Panel — Track Memory is debuggee-only", () => {
  teardown(disposeProviders);

  test("only the active-debuggee row carries the Track-Memory-enabling contextValue", async () => {
    const store = storeWith(STUB_PROCESSES);
    // PID 100 is the active debuggee (centralised, #148).
    store.setActiveDebuggeePid(100);
    const provider = tracked(await loadedProvider(store));
    const rows = processRows(provider.getChildren());
    function ctxOf(pid: number): string | undefined {
      return rows.find((row) => pidOf(row) === pid)?.contextValue;
    }
    assert.strictEqual(ctxOf(100), "processDebuggee", "the debuggee row enables Track Memory");
    assert.strictEqual(ctxOf(200), "processElevated", "an external (elevated) row does not");
    assert.strictEqual(ctxOf(300), "process", "an external launcher row does not");
  });

  test("non-debuggee rows warn that memory tracking is unavailable here; the debuggee does not", async () => {
    const store = storeWith(STUB_PROCESSES);
    store.setActiveDebuggeePid(100);
    const provider = tracked(await loadedProvider(store));
    const rows = processRows(provider.getChildren());
    const debuggee = rows.find((row) => pidOf(row) === 100);
    const external = rows.find((row) => pidOf(row) === 300);
    assert.ok(debuggee && external, "both rows must exist");
    assert.ok(
      !tooltipText(debuggee).includes("Memory tracking needs"),
      "the debuggee row offers tracking, so it shows no caveat",
    );
    assert.ok(
      tooltipText(external).includes("Memory tracking needs"),
      `a non-debuggee row must warn memory tracking is unavailable: ${tooltipText(external)}`,
    );
  });

  test("with no active debuggee, no row enables Track Memory", async () => {
    const store = storeWith(STUB_PROCESSES);
    store.setActiveDebuggeePid(undefined);
    const provider = tracked(await loadedProvider(store));
    const rows = processRows(provider.getChildren());
    assert.ok(
      !rows.some((row) => row.contextValue === "processDebuggee"),
      "no row may offer Track Memory when nothing runs under Basilisk",
    );
  });
});

// ── Inline action target resolution (issue #79) [PROF-UI-PROCESSES] ──
//
// Clicking the inline flame / database icon on a process row must act on
// THAT row. At runtime VS Code has been observed to invoke the command with
// `item === undefined`; the handler must fall back to the tree view's current
// selection — and only warn when there is truly no target.

suite("Processes Panel — inline action target (issue #79)", () => {
  test("undefined item falls back to the tree selection and profiles that PID — without warning", async () => {
    const requests: RecordedRequest[] = [];
    const store = storeWith(STUB_PROCESSES, requests);
    const provider = await loadedProvider(store);
    try {
      const rows = provider.getChildren();
      const selected = rows.find((row) => pidOf(row) === 100);
      assert.ok(selected, "expected the PID 100 row");

      const actions = createProcessRowActions(store, { selection: [selected] });
      const warnings = await captureWarnings(async () => actions.profileProcess(undefined));

      assert.deepStrictEqual(warnings, [], "must not warn when a row is selected");
      const starts = profilerStarts(requests);
      assert.strictEqual(starts.length, 1, "profiler start must be requested");
      assert.strictEqual(startPid(starts[0]), 100, "must profile the selected row's PID");
    } finally {
      provider.dispose();
    }
  });

  test("memory tracking falls back to the tree selection the same way — without warning", async () => {
    const requests: RecordedRequest[] = [];
    const executed: string[] = [];
    const store = storeWith(STUB_PROCESSES, requests);
    seedSignal(store.sessionIdToPid, new Map([["session-1", 200]]));
    const provider = await loadedProvider(store);
    try {
      const rows = provider.getChildren();
      const selected = rows.find((row) => pidOf(row) === 200);
      assert.ok(selected, "expected the PID 200 row");

      const actions = createProcessRowActions(store, { selection: [selected] }, {
        runCommand: recordingRunner(executed),
        activeSession: () => ({ id: "session-1", type: "lspui-debug" }),
      });
      const warnings = await captureWarnings(async () => actions.memoryTrackProcess(undefined));

      assert.deepStrictEqual(warnings, [], "must not warn when a row is selected");
      assert.deepStrictEqual(
        executed,
        ["lspui.profiler.memoryStart"],
        "the selection fallback must reach the real memory-tracking flow",
      );
    } finally {
      provider.dispose();
    }
  });

  test("warns exactly once when there is neither an item nor a selection", async () => {
    const requests: RecordedRequest[] = [];
    const store = storeWith(STUB_PROCESSES, requests);

    const actions = createProcessRowActions(store, { selection: [] });
    const warnings = await captureWarnings(async () => actions.profileProcess(undefined));

    assert.strictEqual(warnings.length, 1, "must warn exactly once");
    assert.strictEqual(
      profilerStarts(requests).length,
      0,
      "must not start profiling without a target",
    );
  });
});

// A row the panel itself marks 🚫 (non-debuggable) must never offer the Profile
// CPU action: package.json reveals it via `viewItem =~ /^process/`, and the
// command handler is the last line of defence against a stale/raced row (#266).
suite("Processes Panel — blocked rows never offer Profile CPU (#266)", () => {
  const BLOCKED_MACHINERY: ProcessInfo = {
    pid: 400, ppid: 100, name: "python3.13", interpreterPath: "/usr/bin/python3.13",
    script: null, runtime: "python", runtimeVersion: "3.13.7", cpuPercent: 0, memoryBytes: 5 * MB,
    runtimeSecs: 60, user: "alice", requiresElevation: true,
    inWorkspace: false, launcher: null, debuggable: false, undebuggableReason: "debugger machinery",
  };
  teardown(disposeProviders);

  test("a non-debuggable row's contextValue opts out of the /^process/ menu clauses", async () => {
    const provider = tracked(await loadedProvider(storeWith([...STUB_PROCESSES, BLOCKED_MACHINERY])));
    const rows = processRows(provider.getChildren());
    const blocked = rows.find((row) => pidOf(row) === 400);
    assert.ok(blocked !== undefined, "the machinery row must still be listed (zero-filter)");
    const ctx = blocked.contextValue ?? "";
    // The package manifest's clause is `viewItem =~ /^process/` — an anchored
    // prefix match, i.e. exactly a startsWith check.
    assert.ok(
      !ctx.startsWith("process"),
      "package.json reveals Profile CPU on `viewItem =~ /^process/`, so a row the panel " +
        `already marks 🚫 "Can't profile" must not match it; got contextValue: ${ctx}`,
    );
  });

  test("invoking Profile CPU on a blocked row refuses instead of attaching", async () => {
    const requests: RecordedRequest[] = [];
    const store = storeWith([...STUB_PROCESSES, BLOCKED_MACHINERY], requests);
    const provider = tracked(await loadedProvider(store));
    const rows = processRows(provider.getChildren());
    const blocked = rows.find((row) => pidOf(row) === 400);
    assert.ok(blocked !== undefined, "the machinery row must exist");

    const actions = createProcessRowActions(store, { selection: [blocked] });
    await actions.profileProcess(blocked);

    assert.strictEqual(
      profilerStarts(requests).length,
      0,
      "a row flagged \"Can't profile\" must never send basilisk.profiler.start — " +
        "the attach is known to fail (#266)",
    );
  });
});
