// Implements [PROF-UI-MEMORY]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { brand, cmd } from "./ids";
import type { ProfilerStore as Store } from "./store";
import type { MemoryIngestResult } from "./memory-dashboard-mapping";
import { formatBytes } from "./profiler-styles";
import { LSP_MEM_CMD, presentDiff, presentSnapshot, runMemoryScript } from "./memory-capture";
import { pickReferenceType, walkReferences } from "./memory-ref-picker";

/** Quick-pick menu of memory actions — the clickable alternative to the palette. */
export async function handleMemoryMenu(store: Store): Promise<void> {
  const tracking = store.profiler.value.memory === "active";
  const items: { label: string; command: string }[] = tracking
    ? [
        { label: "$(device-camera) Take Memory Snapshot", command: cmd("memorySnapshot") },
        { label: "$(diff) Compare Memory Snapshots", command: cmd("memoryDiff") },
        { label: "$(type-hierarchy) Show Reference Graph", command: cmd("memoryReferences") },
        { label: "$(trash) Force Garbage Collection", command: cmd("memoryGcCollect") },
        { label: "$(debug-stop) Stop Memory Tracking", command: cmd("memoryStop") },
      ]
    : [{ label: "$(database) Start Memory Tracking", command: cmd("memoryStart") }];
  const pick = await vscode.window.showQuickPick(items, {
    placeHolder: tracking ? `${brand()} memory profiling` : "Start memory tracking (briefly pauses the program)",
  });
  if (pick !== undefined) {
    await vscode.commands.executeCommand(pick.command);
  }
}

export async function handleMemorySnapshot(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.snapshot);
  if (result?.kind === "snapshot") {
    await presentSnapshot(result, { openResultsView: true });
  }
}

export async function handleMemoryDiff(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.diff);
  if (result?.kind === "diff") {
    const diff = await presentDiff(result);
    void vscode.window.showInformationMessage(
      `${brand()}: Compared snapshots — ${String(diff.suspectedLeaks.length)} suspected leak(s)`,
    );
  }
}

export async function handleMemoryGcCollect(store: Store): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.gcCollect);
  if (result?.kind === "gc") {
    void vscode.window.showInformationMessage(gcSummary(result));
  }
}

/**
 * The toast after a forced collection, in terms every runtime shares: bytes
 * released and, when the runtime reports any, what could not be collected.
 * `collected` is runtime-defined (objects for some, collections observed for
 * others — [PROF-MEMORY-OUTCOME]), so it is never shown as an object count.
 */
export function gcSummary(result: MemoryIngestResult): string {
  const freed = formatBytes(Math.max(0, Number(result.freedBytes ?? 0)));
  const uncollectable = Number(result.uncollectable ?? 0);
  const stuck = uncollectable > 0 ? `; ${String(uncollectable)} object(s) could not be collected` : "";
  return `${brand()}: Garbage collection ran — freed ${freed}${stuck}`;
}

/**
 * Show Reference Graph — pick a type from the data-driven Quick Pick (the active
 * file's classes + container builtins) and walk its retainers
 * ([PROF-UI-REFGRAPH]); no blank "type a name" box.
 */
export async function handleMemoryReferences(store: Store): Promise<void> {
  if (store.profiler.value.memory !== "active") {
    void vscode.window.showWarningMessage(`${brand()}: Start memory tracking first.`);
    return;
  }
  const typeName = await pickReferenceType();
  if (typeName === undefined || typeName.trim() === "") { return; }
  await walkReferences(store, typeName.trim());
}
