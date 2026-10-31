// Implements [PROF-CPU-START] + [PROF-UI-WEBVIEW]. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * The ways out of a results view to the raw trace: VS Code's built-in V8
 * viewer, the file itself, and speedscope.app. Shared by the CPU results panel
 * and the memory dashboard; an explicit request never fails silently.
 */

import * as vscode from "vscode";
import { brand } from "./ids";
import { errorText } from "./failure";
import { Logger } from "./logger";
import { serveProfileForBrowser } from "./profile-server";
import type { ProfileResult } from "./profiler-decorations";

/** The on-disk trace a "Reveal" action points at: the `.cpuprofile`, else the speedscope JSON. */
export function traceFileFor(result: ProfileResult): string {
  const cpu = result.cpuProfilePath ?? "";
  return cpu === "" ? result.outputFile : cpu;
}

/**
 * Open a generated profile artifact in VS Code's built-in viewer beside the
 * source, falling back when no file was produced OR when the built-in viewer's
 * open throws (unavailable in the host, or a file it rejects). Opening beside
 * keeps the profiled file (and its inline decorations) visible. The CPU
 * (`.cpuprofile`) and memory (`.heapprofile`) paths share it through
 * `openNativeTraceViewer` so the open-beside-else-fall-back primitive lives
 * once (dry-1).
 */
async function openNativeProfileViewerBeside(filePath: string, fallback: () => void): Promise<void> {
  if (filePath === "") {
    fallback();
    return;
  }
  try {
    await vscode.commands.executeCommand("vscode.open", vscode.Uri.file(filePath), vscode.ViewColumn.Beside);
  } catch (error: unknown) {
    Logger.warn(`Built-in profile viewer failed to open (${errorText(error)}); using the in-extension fallback`);
    fallback();
  }
}

/**
 * Open a V8 trace (`.cpuprofile` / `.heapprofile`) in VS Code's built-in viewer
 * on demand — the user asked for it explicitly (toast action, results-panel
 * button, or memory-dashboard button), so a viewer that cannot open
 * (unavailable in the host, or no trace produced) is said out loud and the
 * trace file is revealed instead; an explicit request never fails silently.
 */
export async function openNativeTraceViewer(cpuProfilePath: string, traceFile: string): Promise<void> {
  await openNativeProfileViewerBeside(cpuProfilePath, () => {
    void vscode.window.showWarningMessage(
      `${brand()}: VS Code's built-in trace viewer could not open the profile — revealing the trace file instead.`,
    );
    void revealTraceFile(traceFile);
  });
}

/** Reveal `file` in the OS file manager; there is nothing to reveal without one. */
export async function revealTraceFile(file: string): Promise<void> {
  if (file !== "") {
    await vscode.commands.executeCommand("revealFileInOS", vscode.Uri.file(file));
  }
}

/**
 * "Open in Speedscope": speedscope.app is served over https and cannot read
 * `file://` URLs — but browsers treat loopback as a potentially-trustworthy
 * origin, so the profile is served from the extension's 127.0.0.1 server and
 * the deep link loads it automatically ([PROF-UI-WEBVIEW]). The value
 * is left unencoded on purpose: it contains only `:` and `/` (legal raw in a
 * fragment), and percent-encoding it invites `vscode.Uri` re-encoding mangling.
 * A browser without the loopback mixed-content exemption still shows
 * speedscope's error page, so a toast always offers the drag-and-drop path;
 * a failure to serve falls back to that path directly.
 *
 * Shared by the CPU results panel (speedscope JSON) and the memory dashboard
 * (V8 `.heapprofile` — a format speedscope also imports).
 */
export async function openSpeedscopeImport(file: string): Promise<void> {
  try {
    const localUrl = await serveProfileForBrowser(file);
    await vscode.env.openExternal(vscode.Uri.parse(`https://www.speedscope.app/#profileURL=${localUrl}`));
    void offerSpeedscopeFallback(file);
  } catch (error: unknown) {
    Logger.warn(`Profile loopback serving failed (${errorText(error)}); falling back to manual speedscope import`);
    await revealTraceFile(file);
    await vscode.env.openExternal(vscode.Uri.parse("https://www.speedscope.app/"));
    vscode.window.showInformationMessage(`${brand()}: Drag the revealed file into speedscope.app to view it — ${file}`);
  }
}

/**
 * The always-works escape hatch behind the speedscope deep link: fire-and-forget
 * toast (sticky with a button — must not block) whose action reveals the JSON
 * for manual drag-and-drop import.
 */
async function offerSpeedscopeFallback(file: string): Promise<void> {
  const choice = await vscode.window.showInformationMessage(
    `${brand()}: Opened in speedscope.app — if the profile doesn't load, drag the trace file in.`,
    "Reveal Trace File",
  );
  if (choice === "Reveal Trace File") {
    await revealTraceFile(file);
  }
}
