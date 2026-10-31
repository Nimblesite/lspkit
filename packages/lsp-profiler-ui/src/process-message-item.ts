// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";

/**
 * A non-process placeholder row. Returned (instead of an empty list) when the
 * user's search filter empties a NON-empty process list, so the empty-tree
 * viewsWelcome never claims "No processes running" while processes are in
 * fact running, just filtered (procexp-2).
 */
export class MessageTreeItem extends vscode.TreeItem {
  public constructor(label: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.contextValue = "processesMessage";
    this.iconPath = new vscode.ThemeIcon("filter");
  }
}
