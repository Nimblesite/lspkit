// Mocha entry of the UI suites. Tests [PROF-TESTING].
import * as path from "node:path";
import Mocha from "mocha";
import { glob } from "glob";
import { SUITE_SETUP_TIMEOUT_MS, waitForLspReady } from "./test-helpers";

export async function run(): Promise<void> {
  const mocha = new Mocha({
    ui: "tdd",
    color: true,
    timeout: parseInt(process.env.MOCHA_TIMEOUT ?? "60000", 10),
    ...(process.env.LSPUI_TEST_GREP !== undefined ? { grep: process.env.LSPUI_TEST_GREP } : {}),
    rootHooks: {
      beforeAll(this: Mocha.Context, done: Mocha.Done) {
        this.timeout(SUITE_SETUP_TIMEOUT_MS);
        waitForLspReady().then(() => done(), done);
      },
    },
  });
  const testsRoot = path.resolve(__dirname);
  const files = await glob("**/**.test.js", { cwd: testsRoot });
  files.forEach((f) => mocha.addFile(path.resolve(testsRoot, f)));
  return new Promise<void>((resolve, reject) => {
    mocha.run((failures) => (failures > 0 ? reject(new Error(`${failures} tests failed.`)) : resolve()));
  });
}
