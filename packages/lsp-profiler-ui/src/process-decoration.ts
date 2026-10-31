// Implements [PROF-UI-PROCESSES]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { processUriScheme } from "./ids";
import type { ProcessesProvider } from "./process-explorer";

/**
 * Colours whole process-row labels green (workspace) or grey (non-debuggable),
 * keyed on the synthetic `<prefix>-process:` URI each row carries — the only way
 * to tint a tree item's full label. Implements [PROF-UI-PROCESSES].
 *
 * Greying wins over green: a process you cannot debug is never shown as an
 * actionable workspace row.
 */
export class ProcessDecorationProvider implements vscode.FileDecorationProvider, vscode.Disposable {
  private readonly emitter = new vscode.EventEmitter<undefined>();
  public readonly onDidChangeFileDecorations = this.emitter.event;
  private readonly subscription: vscode.Disposable;

  public constructor(provider: ProcessesProvider) {
    // Rows are recreated on every refresh with state baked into their URI query;
    // re-query decorations whenever the tree repaints so colours never go stale.
    this.subscription = provider.onDidChangeTreeData(() => { this.emitter.fire(undefined); });
  }

  public provideFileDecoration(uri: vscode.Uri): vscode.FileDecoration | undefined {
    if (uri.scheme !== processUriScheme()) { return undefined; }
    const params = new URLSearchParams(uri.query);
    if (params.get("dbg") === "0") {
      return { color: new vscode.ThemeColor("disabledForeground"), tooltip: "Can't profile" };
    }
    if (params.get("ws") === "1") {
      return { color: new vscode.ThemeColor("charts.green"), tooltip: "Workspace process" };
    }
    return undefined;
  }

  public dispose(): void {
    this.subscription.dispose();
    this.emitter.dispose();
  }
}
