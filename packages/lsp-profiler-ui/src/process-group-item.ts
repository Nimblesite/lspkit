// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import type { ProcessInfo } from "./process-explorer-rows";

/** A collapsible group header (when grouping is active). */
export class ProcessGroupItem extends vscode.TreeItem {
  public readonly label: string;
  public readonly members: readonly ProcessInfo[];
  public constructor(label: string, members: readonly ProcessInfo[]) {
    super(label, vscode.TreeItemCollapsibleState.Expanded);
    this.label = label;
    this.members = members;
    // Stable identity so VS Code preserves expansion state across refreshes.
    this.id = `processGroup:${label}`;
    this.description = String(members.length);
    this.contextValue = "processGroup";
    this.iconPath = new vscode.ThemeIcon("folder");
  }
}
