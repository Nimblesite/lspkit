// Implements [PROF-UI-IDS] for context keys: every `setContext` the library
// performs goes through here, under the host's prefix, and the last value of
// each key is recorded so suites can prove the panel's `when` clauses see it.
import * as vscode from "vscode";
import { ctx } from "./ids";

const values = new Map<string, unknown>();

/** Set `<prefix>.profiler.<name>` to `value`. */
export function setContextKey(name: string, value: unknown): void {
  const key = ctx(name);
  values.set(key, value);
  void vscode.commands.executeCommand("setContext", key, value);
}

/** The last value written per fully-qualified key (e2e seam). */
export function contextKeyValues(): ReadonlyMap<string, unknown> {
  return values;
}
