// Implements [PROF-UI-PALETTE]. Apply settings changes to already-open panels.
import * as vscode from "vscode";
import { configSection } from "./ids";
import { profilerPalette } from "./palette";

export function bindPanelPalette(panel: vscode.WebviewPanel): vscode.Disposable {
  const subscription = vscode.workspace.onDidChangeConfiguration(event => {
    if (event.affectsConfiguration(`${configSection()}.profiler.palette`)) {
      void panel.webview.postMessage({ type: "profilerPalette", palette: profilerPalette() });
    }
  });
  panel.onDidDispose(() => subscription.dispose());
  return subscription;
}
