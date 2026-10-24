// Implements [PROF-UI-PALETTE]. Apply settings changes to already-open panels.
import * as vscode from "vscode";
import { configSection } from "./ids";
import { paletteVars, profilerPalette } from "./palette";

export function bindPanelPalette(panel: vscode.WebviewPanel): vscode.Disposable {
  const subscription = vscode.workspace.onDidChangeConfiguration(event => {
    if (event.affectsConfiguration(`${configSection()}.profiler.palette`)) {
      void panel.webview.postMessage({ type: "profilerPalette", palette: profilerPalette(), vars: paletteVars() });
    }
  });
  panel.onDidDispose(() => subscription.dispose());
  return subscription;
}
