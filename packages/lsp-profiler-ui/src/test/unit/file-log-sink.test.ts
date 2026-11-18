// Tests [PROF-UI-LIFETIME]: the file log stays bounded on disk and its stream
// closes on dispose. Plain mocha: the sink imports only `node:fs`.
import * as assert from "node:assert";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import path from "node:path";
import { FileLogSink } from "../../file-log-sink";

/** A small cap, so the test overflows it quickly. */
const CAP_BYTES = 4096;
/** Lines written: several times the cap. */
const LINES = 400;
/** One log line's payload. */
const PAYLOAD = "x".repeat(64);

/** The text of `file`, or nothing when it does not exist. */
async function textOf(file: string): Promise<string> {
  try {
    return await fs.readFile(file, "utf8");
  } catch {
    return "";
  }
}

suite("File log sink stays bounded and closes", () => {
  test("a long-running log keeps at most two capped files and the newest line", async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), "lspui-log-"));
    const file = path.join(dir, "profiler.log");
    const sink = new FileLogSink(file, CAP_BYTES);
    for (let index = 0; index < LINES; index += 1) {
      sink.info(`${String(index)} ${PAYLOAD}`);
    }
    sink.info("newest");
    const closed = sink.dispose();
    sink.info("after dispose");
    await closed;
    const texts = await Promise.all([file, `${file}.1`].map(async (each) => textOf(each)));
    const total = texts.reduce((sum, each) => sum + Buffer.byteLength(each), 0);
    const text = texts.join("");
    await fs.rm(dir, { recursive: true, force: true });
    assert.ok(total <= 2 * CAP_BYTES, `${String(total)} log bytes on disk for a ${String(CAP_BYTES)}-byte cap`);
    assert.ok(text.includes("newest"), "the newest line is on disk");
    assert.ok(!text.includes("after dispose"), "a disposed sink still writes: its file stays open");
  });
});
