// Implements [PROF-UI]. See docs/specs/LSP-PROFILER-SPEC.md
/** One spelling for a failed operation: its text, and how the user hears of it. */

import * as vscode from "vscode";
import { brand } from "./ids";
import { Logger } from "./logger";

/** The message an unknown thrown value carries. */
export function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** Log that `what` failed and show the reason under the host's brand. */
export function reportFailure(what: string, err: unknown): void {
  const msg = errorText(err);
  Logger.error(`${what} failed: ${msg}`);
  void vscode.window.showErrorMessage(`${brand()}: ${msg}`);
}
