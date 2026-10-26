// Implements [PROF-UI-DEBUG-LINK]. Native Watch/Variables menus share the profiler bridge.
import * as vscode from "vscode";
import { cmd } from "./ids";
import { profileVariable, selectedProfileVariable } from "./debug-selection";
import { buildWebviewDocument, handleSourceNavigation, SingletonWebviewPanel } from "./profiler-webview";
import { PROFILER_CSS_VARS, PROFILER_CSS_RESET } from "./profiler-styles";
import { isRecord } from "./unknown-shape";

export interface DebugLinkHost {
  readonly debugType: string;
  readonly inspectType?: (type: string, sessionId: string) => Promise<void>;
}

export function registerDebugProfilerLink(host: DebugLinkHost): vscode.Disposable[] {
  const panel = new SingletonWebviewPanel(() => cmd("variable"), handleSourceNavigation);
  return [panel, vscode.commands.registerCommand(cmd("inspectVariable"), context => { inspect(context, host, panel); }),
    vscode.commands.registerCommand(cmd("watchVariable"), async () => { await watchSelected(host); }),
    vscode.commands.registerCommand(cmd("inspectVariableType"), async () => { await inspectType(host); }),
    vscode.debug.registerDebugAdapterTrackerFactory(host.debugType, { createDebugAdapterTracker: session => ({
      onDidSendMessage(message: unknown) {
        if (isRecord(message) && message.type === "event" && ["continued", "terminated", "exited"].includes(String(message.event))) {
          if (selectedProfileVariable.value?.sessionId === session.id) selectedProfileVariable.value = undefined;
        }
      },
      onWillStopSession() { if (selectedProfileVariable.value?.sessionId === session.id) selectedProfileVariable.value = undefined; },
    }) }),
  ];
}

function inspect(context: unknown, host: DebugLinkHost, panel: SingletonWebviewPanel): void {
  const session = vscode.debug.activeDebugSession;
  const variable = session?.type === host.debugType ? profileVariable(context, session.id) : undefined;
  if (variable === undefined) { void vscode.window.showWarningMessage("Select a variable in this profiler's active debugger."); return; }
  selectedProfileVariable.value = { ...variable, canInspectType: host.inspectType !== undefined && variable.type !== "" };
  panel.show("Profiler · Debug variable", buildWebviewDocument({ title: "Profiler · Debug variable",
    css: PROFILER_CSS_VARS + PROFILER_CSS_RESET + "body { padding: 16px; }",
    body: "<p>The selected debugger value is linked across the profiler panels.</p>", script: "" }));
}

async function watchSelected(host: DebugLinkHost): Promise<void> {
  const variable = selectedProfileVariable.value;
  const session = vscode.debug.activeDebugSession;
  if (variable === undefined || session?.id !== variable.sessionId || session.type !== host.debugType) {
    throw new Error("The variable's debug session is no longer active.");
  }
  await vscode.commands.executeCommand("debug.addToWatchExpressions", {
    sessionId: session.id, variable: { name: variable.name, evaluateName: variable.expression },
  });
}

async function inspectType(host: DebugLinkHost): Promise<void> {
  const variable = selectedProfileVariable.value;
  if (variable === undefined || vscode.debug.activeDebugSession?.id !== variable.sessionId) throw new Error("Select a current debugger variable first.");
  if (host.inspectType === undefined || variable.type === "") {
    void vscode.window.showInformationMessage("This runtime has no type-retention capture for this selection."); return;
  }
  await host.inspectType(variable.type, variable.sessionId);
}
