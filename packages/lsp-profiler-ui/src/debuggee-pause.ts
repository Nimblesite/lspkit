// Which debug sessions of the host's debug type are paused right now, read off
// the adapter's own messages. Implements [PROF-UI-AUTOPILOT] and
// [PROF-MEMORY-MODE]: a courier runtime is captured at a pause, and a direct
// runtime only while its program runs, because it cannot read a heap the
// debugger holds paused.

import * as vscode from "vscode";
import { debugType } from "./ids";
import { isRecord } from "./unknown-shape";

type SessionListener = (sessionId: string) => void;

/** Requests whose success means the debuggee runs again. */
const RESUMING_REQUESTS = new Set(["continue", "next", "stepIn", "stepOut", "stepBack", "reverseContinue", "goto"]);

const paused = new Set<string>();
const pauseListeners = new Set<SessionListener>();
const resumeListeners = new Set<SessionListener>();

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
  resumeListeners.clear();
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

/** Call `listener` with the session id each time a paused host debuggee runs again. */
export function onDebuggeeResumed(listener: SessionListener): vscode.Disposable {
  resumeListeners.add(listener);
  return { dispose: () => { resumeListeners.delete(listener); } };
}

function observe(sessionId: string, message: unknown): void {
  if (!isRecord(message)) { return; }
  if (message.type === "event") {
    if (message.event === "stopped") {
      paused.add(sessionId);
      pauseListeners.forEach((listener) => { listener(sessionId); });
    } else if (message.event === "continued") {
      resumed(sessionId);
    } else if (message.event === "terminated" || message.event === "exited") {
      paused.delete(sessionId);
    }
    return;
  }
  if (message.type === "response" && message.success === true && RESUMING_REQUESTS.has(String(message.command))) {
    resumed(sessionId);
  }
}

function resumed(sessionId: string): void {
  if (paused.delete(sessionId)) {
    resumeListeners.forEach((listener) => { listener(sessionId); });
  }
}
