// Which debug sessions of the host's debug type are paused right now, read off
// the adapter's own messages. Implements [PROF-UI-AUTOPILOT] and
// [PROF-MEMORY-MODE]: a courier runtime is captured at a pause, and a direct
// runtime never while paused, because it cannot read a heap the debugger holds.

import * as vscode from "vscode";
import { debugType } from "./ids";
import { isRecord } from "./unknown-shape";

type SessionListener = (sessionId: string) => void;

/** Requests whose success means the debuggee runs again. */
const RESUMING_REQUESTS = new Set(["continue", "next", "stepIn", "stepOut", "stepBack", "reverseContinue", "goto"]);

const paused = new Set<string>();
const pauseListeners = new Set<SessionListener>();

/** Follow every host debug session's pauses and resumptions. */
export function registerDebuggeePauseTracking(): vscode.Disposable[] {
  return [
    vscode.debug.registerDebugAdapterTrackerFactory(debugType(), {
      createDebugAdapterTracker: (session) => ({
        onDidSendMessage: (message: unknown) => { observe(session.id, message); },
        onWillStopSession: () => { paused.delete(session.id); },
      }),
    }),
    { dispose: disposeDebuggeePauseTracking },
  ];
}

/** Forget every session and listener (deactivation, test teardown). */
export function disposeDebuggeePauseTracking(): void {
  paused.clear();
  pauseListeners.clear();
}

/** Whether the debugger holds `sessionId`'s program paused. */
export function isDebuggeePaused(sessionId: string | undefined): boolean {
  return sessionId !== undefined && paused.has(sessionId);
}

/** Call `listener` with the session id each time a host debuggee pauses. */
export function onDebuggeePaused(listener: SessionListener): vscode.Disposable {
  pauseListeners.add(listener);
  return { dispose: () => { pauseListeners.delete(listener); } };
}

function observe(sessionId: string, message: unknown): void {
  if (!isRecord(message)) { return; }
  if (message.type === "event") {
    observeEvent(sessionId, message.event);
    return;
  }
  if (message.type === "response" && message.success === true && RESUMING_REQUESTS.has(String(message.command))) {
    resumed(sessionId);
  }
}

/** Apply one DAP event to the pause bookkeeping. */
function observeEvent(sessionId: string, event: unknown): void {
  if (event === "stopped") {
    paused.add(sessionId);
    for (const listener of pauseListeners) { listener(sessionId); }
    return;
  }
  if (event === "continued") {
    resumed(sessionId);
    return;
  }
  if (event === "terminated" || event === "exited") { paused.delete(sessionId); }
}

function resumed(sessionId: string): void {
  paused.delete(sessionId);
}
