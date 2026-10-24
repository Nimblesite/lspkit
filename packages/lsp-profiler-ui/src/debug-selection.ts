// Implements [PROF-UI-DEBUG-LINK]. A selection belongs to one paused debug session.
import { signal, effect } from "@preact/signals-core";
import type * as vscode from "vscode";
import { isRecord } from "./unknown-shape";

export interface ProfileVariable {
  readonly sessionId: string;
  readonly expression: string;
  readonly name: string;
  readonly value: string;
  readonly type: string;
  readonly canInspectType?: boolean;
}
export const selectedProfileVariable = signal<ProfileVariable | undefined>(undefined);

/** Decode the standard VS Code debug variable context without inventing object identity. */
export function profileVariable(context: unknown, sessionId: string): ProfileVariable | undefined {
  if (!isRecord(context) || (context.sessionId !== undefined && context.sessionId !== sessionId)) return undefined;
  const variable = isRecord(context.variable) ? context.variable : context;
  if (typeof variable.name !== "string" || typeof variable.value !== "string") return undefined;
  const expression = typeof variable.evaluateName === "string" ? variable.evaluateName : variable.name;
  return expression === "" ? undefined : { sessionId, expression, name: variable.name,
    value: variable.value, type: typeof variable.type === "string" ? variable.type : "" };
}

/** Keep open panels reactive without replacing their zoom, scroll or selected frame. */
export function bindDebugSelection(panel: vscode.WebviewPanel): vscode.Disposable {
  const dispose = effect(() => { void panel.webview.postMessage({
    type: "profilerVariable", variable: selectedProfileVariable.value,
  }); });
  panel.onDidDispose(dispose);
  return { dispose };
}
