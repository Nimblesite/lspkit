// Implements [PROF-UI-REFGRAPH].
// See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The data-driven type picker for "Show Reference Graph".
 *
 * The walk needs the runtime's exact name for a type, so the candidates are the
 * types the runtime itself reported in this session's latest snapshot, in the
 * snapshot's order. They are never names guessed from source text or one
 * language's builtins: another runtime names its types differently and the walk
 * would find nothing ([PROF-INVARIANTS]). Free text stays one pick away for a
 * type the snapshot did not list.
 */

import * as vscode from "vscode";
import type { ProfilerStore as Store } from "./store";
import type { MemoryDashboardSnapshot } from "./memory-dashboard";
import { LSP_MEM_CMD, lastMemorySnapshot, runMemoryScript } from "./memory-capture";
import { decodeRefGraph, openRefGraphWebview } from "./memory-ref-graph";

/** Reference-graph traversal bounds. */
const REF_GRAPH_MAX_DEPTH = 5;
const REF_GRAPH_MAX_NODES = 200;

/** The Quick Pick item that drops to free-text entry. */
const OTHER_TYPE_LABEL = "$(edit) Other type…";

/** The distinct runtime-reported type names of `snapshot`'s allocation sites, in order. */
export function referenceTypeCandidates(snapshot: MemoryDashboardSnapshot | undefined): string[] {
  const names = (snapshot?.topAllocations ?? []).flatMap((site) =>
    site.typeName === undefined ? [] : [site.typeName],
  );
  return [...new Set(names)];
}

/** Ask for a type name in free text. */
async function askForTypeName(): Promise<string | undefined> {
  return await vscode.window.showInputBox({
    prompt: "Type to inspect, exactly as the runtime names it",
    placeHolder: "Take a snapshot first to pick from the types it found",
  });
}

/**
 * Offer the snapshot's types, returning the chosen one (or undefined when
 * cancelled). With no snapshot types, or on "Other type…", ask in free text.
 */
export async function pickReferenceType(): Promise<string | undefined> {
  const candidates = referenceTypeCandidates(lastMemorySnapshot());
  if (candidates.length === 0) { return await askForTypeName(); }
  const pick = await vscode.window.showQuickPick(
    [...candidates.map((type) => ({ label: type })), { label: OTHER_TYPE_LABEL }],
    { placeHolder: "Inspect retainers of which type? (types in the latest snapshot)" },
  );
  if (pick === undefined) { return undefined; }
  return pick.label === OTHER_TYPE_LABEL ? await askForTypeName() : pick.label;
}

/** Run the reference-graph walk for `typeName` and open the graph webview. */
export async function walkReferences(store: Store, typeName: string): Promise<void> {
  const result = await runMemoryScript(store, LSP_MEM_CMD.references, {
    type_name: typeName,
    max_depth: REF_GRAPH_MAX_DEPTH,
    limit: REF_GRAPH_MAX_NODES,
  });
  if (result?.kind === "refs") {
    openRefGraphWebview({
      targetType: typeName,
      maxDepth: REF_GRAPH_MAX_DEPTH,
      maxNodes: REF_GRAPH_MAX_NODES,
      script: "",
      graph: decodeRefGraph(result.graph),
    });
  }
}
