import * as assert from "assert";
import { installUiStubs, WindowLike } from "../src/ui-stubs";

function fakeWindow(): WindowLike & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    showInformationMessage: (...a: unknown[]) => { calls.push(`orig-info:${String(a[0])}`); return undefined; },
    showWarningMessage: (...a: unknown[]) => { calls.push(`orig-warn:${String(a[0])}`); return undefined; },
    showErrorMessage: (...a: unknown[]) => { calls.push(`orig-error:${String(a[0])}`); return undefined; },
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
    await window.showInformationMessage(undefined as unknown as string);
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
