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
  queueInfo(...values: (string | undefined)[]): UiStubs;
  /** Queue the value the next showWarningMessage resolves to. */
  queueWarning(...values: (string | undefined)[]): UiStubs;
  /** Queue the value the next showErrorMessage resolves to. */
  queueError(...values: (string | undefined)[]): UiStubs;
  /** Restore the original window methods. */
  restore(): void;
}

type ShowKey =
  | "showInformationMessage"
  | "showWarningMessage"
  | "showErrorMessage";

function firstStringArg(args: unknown[]): string {
  return typeof args[0] === "string" ? args[0] : String(args[0] ?? "");
}

/** Install the stubs onto `window`, returning a controller. Call restore(). */
export function installUiStubs(window: WindowLike): UiStubs {
  const infoMessages: string[] = [];
  const warningMessages: string[] = [];
  const errorMessages: string[] = [];
  const queues: Record<ShowKey, (string | undefined)[]> = {
    showInformationMessage: [],
    showWarningMessage: [],
    showErrorMessage: [],
  };
  const originals: Record<ShowKey, unknown> = {
    showInformationMessage: window.showInformationMessage,
    showWarningMessage: window.showWarningMessage,
    showErrorMessage: window.showErrorMessage,
  };

  const mutableWindow = window as unknown as Record<string, unknown>;
  const patch = (key: ShowKey, sink: string[]): void => {
    mutableWindow[key] = (...args: unknown[]) => {
      sink.push(firstStringArg(args));
      return Promise.resolve(queues[key].shift());
    };
  };
  patch("showInformationMessage", infoMessages);
  patch("showWarningMessage", warningMessages);
  patch("showErrorMessage", errorMessages);

  const controller: UiStubs = {
    log: { infoMessages, warningMessages, errorMessages },
    queueInfo(...values) {
      queues.showInformationMessage.push(...values);
      return controller;
    },
    queueWarning(...values) {
      queues.showWarningMessage.push(...values);
      return controller;
    },
    queueError(...values) {
      queues.showErrorMessage.push(...values);
      return controller;
    },
    restore() {
      for (const key of Object.keys(originals) as ShowKey[]) {
        mutableWindow[key] = originals[key];
      }
    },
  };
  return controller;
}
