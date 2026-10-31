// Mocha entry of the UI suites. Tests [PROF-TESTING].
import path from "node:path";
import { once } from "node:events";
import { env } from "node:process";
import Mocha from "mocha";
import { glob } from "glob";
import { SUITE_SETUP_TIMEOUT_MS, waitForLspReady } from "./test-helpers";

const DEFAULT_TIMEOUT_MS = 60_000;
const TIMEOUT_ENV = "MOCHA_TIMEOUT";
const GREP_ENV = "LSPUI_TEST_GREP";
const TEST_FILE_GLOB = "**/**.test.js";
const SUITE_END_EVENT = "end";

/** The configured suite timeout: the environment override, else the default. */
function configuredTimeout(): number {
  const override = env[TIMEOUT_ENV];
  return override === undefined ? DEFAULT_TIMEOUT_MS : Math.trunc(Number(override));
}

/** The optional `--grep` pattern, absent when the environment does not set one. */
function configuredGrep(): { grep?: string } {
  const pattern = env[GREP_ENV];
  return pattern === undefined ? {} : { grep: pattern };
}

/** Mocha root hook: waits for the language server before the first suite. */
async function beforeAll(this: Mocha.Context): Promise<void> {
  this.timeout(SUITE_SETUP_TIMEOUT_MS);
  await waitForLspReady();
}

export async function run(): Promise<void> {
  const mocha = new Mocha({
    ui: "tdd",
    color: true,
    timeout: configuredTimeout(),
    ...configuredGrep(),
    rootHooks: { beforeAll },
  });
  const testsRoot = path.resolve(__dirname);
  for (const file of await glob(TEST_FILE_GLOB, { cwd: testsRoot })) {
    mocha.addFile(path.resolve(testsRoot, file));
  }
  const runner = mocha.run();
  await once(runner, SUITE_END_EVENT);
  const { failures } = runner;
  if (failures > 0) {
    throw new Error(`${failures} tests failed.`);
  }
}
