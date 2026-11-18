// Tests [MEM-060-UI-DISPOSE]. Runs in its own editor instance (the
// `ui-teardown` entry in .vscode-test.mjs): it disposes the host UI, as a host
// does before registering again, which would break every suite after it.
import * as assert from "node:assert";
import { hostApi } from "../suite/test-helpers";

suite("Profiler UI teardown releases the processes view", () => {
  test("disposing the UI disposes the processes provider and its effects", async () => {
    const api = await hostApi();
    let repaints = 0;
    const listener = api.ui.processes.onDidChangeTreeData(() => { repaints += 1; });
    api.ui.processes.refresh();
    assert.strictEqual(repaints, 1, "the live provider repaints");
    api.ui.dispose();
    api.ui.processes.refresh();
    listener.dispose();
    assert.strictEqual(
      repaints,
      1,
      "the disposed UI's processes provider still repaints: its poll and effects outlive the UI",
    );
  });
});
