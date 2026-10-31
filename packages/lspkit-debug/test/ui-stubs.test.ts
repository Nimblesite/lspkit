import * as assert from "node:assert";
import { installUiStubs } from "../src/ui-stubs";
import type { WindowLike } from "../src/ui-stubs";

function fakeWindow(): WindowLike & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    showInformationMessage: (...args: unknown[]) => {
      calls.push(`orig-info:${String(args[0])}`);
    },
    showWarningMessage: (...args: unknown[]) => {
      calls.push(`orig-warn:${String(args[0])}`);
    },
    showErrorMessage: (...args: unknown[]) => {
      calls.push(`orig-error:${String(args[0])}`);
    },
  };
}

suite("installUiStubs", () => {
  test("records each prompt and returns queued FIFO answers", async () => {
    const window = fakeWindow();
    const stubs = installUiStubs(window);
    stubs.queueInfo("A").queueWarning("B").queueError("C");

    const info = await window.showInformationMessage("pick info", "A", "X");
    const warn = await window.showWarningMessage("pick warn");
    const error = await window.showErrorMessage("pick error");

    assert.strictEqual(info, "A");
    assert.strictEqual(warn, "B");
    assert.strictEqual(error, "C");
    assert.deepStrictEqual(stubs.log.infoMessages, ["pick info"]);
    assert.deepStrictEqual(stubs.log.warningMessages, ["pick warn"]);
    assert.deepStrictEqual(stubs.log.errorMessages, ["pick error"]);
    // The originals were not invoked while stubbed.
    assert.deepStrictEqual(window.calls, []);
  });

  test("resolves undefined when the queue is empty", async () => {
    const window = fakeWindow();
    installUiStubs(window);
    assert.strictEqual(await window.showWarningMessage("no answer queued"), undefined);
  });

  test("coerces a non-string first argument for the log", async () => {
    const window = fakeWindow();
    const stubs = installUiStubs(window);
    await window.showInformationMessage(undefined);
    assert.deepStrictEqual(stubs.log.infoMessages, [""]);
  });

  test("restore puts the original methods back", async () => {
    const window = fakeWindow();
    const stubs = installUiStubs(window);
    stubs.restore();
    window.showInformationMessage("after restore");
    assert.deepStrictEqual(window.calls, ["orig-info:after restore"]);
  });
});
