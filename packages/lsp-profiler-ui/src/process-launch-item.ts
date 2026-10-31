// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";

/**
 * A persistent "Run & …(Current File)" launch row pinned to the top of the tree.
 * VS Code only renders the big `viewsWelcome` buttons when the tree is EMPTY, so
 * once any process appears those buttons vanish — these rows keep the current-file
 * launches reachable alongside a populated list ([PROF-UI-PROCESSES]).
 * Gated per-activity (hidden while the matching metric is busy) like the title-bar
 * buttons, so a second same-metric run is never offered ([PROF-UI-PROCESSES]).
 */
export class LaunchActionItem extends vscode.TreeItem {
  public constructor(label: string, command: string, icon: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.id = `launchAction:${command}`;
    this.contextValue = "launchAction";
    this.iconPath = new vscode.ThemeIcon(icon, new vscode.ThemeColor("textLink.foreground"));
    this.command = { command, title: label };
  }
}
