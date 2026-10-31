// Implements [PROF-UI-MEMORY]. See docs/specs/LSP-PROFILER-SPEC.md
import * as vscode from "vscode";
import { cmd } from "./ids";
import type { ProfilerStore as Store } from "./store";
import { withUserProgress } from "./progress-ops";
import { memStartTitle } from "./memory-capture";
import { settleMemoryOnSessionEnd } from "./memory-finalize";
import { trackMemoryOnLaunch } from "./memory-start";
import { handleMemoryDiff, handleMemoryGcCollect, handleMemoryMenu, handleMemoryReferences, handleMemorySnapshot } from "./memory-inspect-commands";
import { runMemoryStartCommand, runMemoryStopCommand } from "./memory-start-stop-commands";

/** The memory command registrations and debug-session listeners. */
export function memoryCommandDisposables(store: Store): vscode.Disposable[] {
  return [
    vscode.commands.registerCommand(cmd("memoryMenu"), async () => { await handleMemoryMenu(store); }),
    vscode.commands.registerCommand(cmd("memoryStart"), async (pid?: unknown) => { await withUserProgress(memStartTitle(), async (report) => { await runMemoryStartCommand(store, report, typeof pid === "number" ? pid : undefined); },
      ); },
    ),
    vscode.commands.registerCommand(cmd("memorySnapshot"), async () => { await handleMemorySnapshot(store); }),
    vscode.commands.registerCommand(cmd("memoryDiff"), async () => { await handleMemoryDiff(store); }),
    vscode.commands.registerCommand(cmd("memoryGcCollect"), async () => { await handleMemoryGcCollect(store); }),
    vscode.commands.registerCommand(cmd("memoryStop"), async () => { await runMemoryStopCommand(store); }),
    vscode.commands.registerCommand(cmd("memoryReferences"), async () => { await handleMemoryReferences(store); }),
    // The status-bar show/hide on session changes is bound in memory-status.ts.
    vscode.debug.onDidStartDebugSession((session) => { trackMemoryOnLaunch(store, session); }),
    vscode.debug.onDidTerminateDebugSession((session) => { settleMemoryOnSessionEnd(store, session); }),
  ];
}
