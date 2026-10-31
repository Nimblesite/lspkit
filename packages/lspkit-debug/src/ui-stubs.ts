// Queue-backed stubs for VS Code's modal `window.show*Message` dialogs, so an
// integration test can supply FIFO answers and assert what was prompted without
// blocking on real UI. Generalized from SharpLsp's installUiStubs; the window
// object is injected, so it patches the real `vscode.window` in a test host and
// a plain object under unit test.

/** The subset of `vscode.window` the stubs intercept. */
export interface WindowLike {
  showInformationMessage: (...args: unknown[]) => unknown;
  showWarningMessage: (...args: unknown[]) => unknown;
  showErrorMessage: (...args: unknown[]) => unknown;
}

/** Every message the stubbed window was asked to show. */
export interface UiStubLog {
  readonly infoMessages: string[];
  readonly warningMessages: string[];
  readonly errorMessages: string[];
}

/** The installed stub controller. */
export interface UiStubs {
  readonly log: UiStubLog;
  /** Queue the value the next showInformationMessage resolves to. */
  queueInfo: (...values: (string | undefined)[]) => UiStubs;
  /** Queue the value the next showWarningMessage resolves to. */
  queueWarning: (...values: (string | undefined)[]) => UiStubs;
  /** Queue the value the next showErrorMessage resolves to. */
  queueError: (...values: (string | undefined)[]) => UiStubs;
  /** Restore the original window methods. */
  restore: () => void;
}

type ShowKey =
  | "showInformationMessage"
  | "showWarningMessage"
  | "showErrorMessage";

/** Everything a stub needs to log a prompt and hand back a queued answer. */
interface StubState {
  readonly window: WindowLike;
  readonly originals: WindowLike;
  readonly queues: Record<ShowKey, (string | undefined)[]>;
  readonly log: UiStubLog;
}

/** The text a `show*Message` call logged: its first argument, as a string. */
function firstStringArg(args: readonly unknown[]): string {
  const [first] = args;
  if (typeof first === "string") {
    return first;
  }
  if (
    typeof first === "number" ||
    typeof first === "boolean" ||
    typeof first === "bigint"
  ) {
    return String(first);
  }
  return "";
}

/** Append queued answers to the queue one prompt will draw from. */
function pushAnswers(
  queue: (string | undefined)[],
  values: readonly (string | undefined)[],
): void {
  queue.push(...values);
}

/** One patched window method: log its message, resolve the next queued answer. */
function makeStub(
  queue: (string | undefined)[],
  sink: string[],
): (...args: unknown[]) => unknown {
  return (...args: unknown[]): unknown => {
    sink.push(firstStringArg(args));
    return Promise.resolve(queue.shift());
  };
}

/** Snapshot the three window methods so {@link UiStubs.restore} can put them back. */
function snapshotWindow(window: WindowLike): WindowLike {
  return {
    showInformationMessage: window.showInformationMessage,
    showWarningMessage: window.showWarningMessage,
    showErrorMessage: window.showErrorMessage,
  };
}

/** Put the snapshotted methods back onto the window. */
function restoreWindow(window: WindowLike, originals: WindowLike): void {
  window.showInformationMessage = originals.showInformationMessage;
  window.showWarningMessage = originals.showWarningMessage;
  window.showErrorMessage = originals.showErrorMessage;
}

/** Replace the three window methods with queue-backed stubs. */
function patchWindow(state: StubState): void {
  state.window.showInformationMessage = makeStub(
    state.queues.showInformationMessage,
    state.log.infoMessages,
  );
  state.window.showWarningMessage = makeStub(
    state.queues.showWarningMessage,
    state.log.warningMessages,
  );
  state.window.showErrorMessage = makeStub(
    state.queues.showErrorMessage,
    state.log.errorMessages,
  );
}

/** Build the controller the caller queues answers through. */
function makeController(state: StubState): UiStubs {
  const controller: UiStubs = {
    log: state.log,
    queueInfo(...values) {
      pushAnswers(state.queues.showInformationMessage, values);
      return controller;
    },
    queueWarning(...values) {
      pushAnswers(state.queues.showWarningMessage, values);
      return controller;
    },
    queueError(...values) {
      pushAnswers(state.queues.showErrorMessage, values);
      return controller;
    },
    restore: (): void => { restoreWindow(state.window, state.originals); },
  };
  return controller;
}

/** Install the stubs onto `window`, returning a controller. Call restore(). */
export function installUiStubs(window: WindowLike): UiStubs {
  const state: StubState = {
    window,
    originals: snapshotWindow(window),
    queues: {
      showInformationMessage: [],
      showWarningMessage: [],
      showErrorMessage: [],
    },
    log: { infoMessages: [], warningMessages: [], errorMessages: [] },
  };
  patchWindow(state);
  return makeController(state);
}
