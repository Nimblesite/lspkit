// Tests [PROF-WIRE-GEN]: the generator must see every byte the typeDiagram CLI
// prints, or it writes a truncated wire model that no longer compiles.
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { captureStdout, quoteForCmd } from "./run-cli.mjs";

// Writes `size` bytes then exits at once, as the typeDiagram CLI does. Node
// writes to a macOS pipe asynchronously, so an exit straight after the write
// drops everything past the first 8 KiB that reached a piped capture.
const writeThenExit = (size, code = 0) => [
  "-e",
  `process.stdout.write("ab".repeat(${size / 2})); process.exit(${code});`,
];

test("captures every byte of a large output written just before exit", () => {
  const size = 1 << 20;
  const out = captureStdout(process.execPath, writeThenExit(size));
  assert.equal(out.length, size, "no byte past the pipe buffer may be lost");
  assert.ok(out.startsWith("abab"));
  assert.ok(out.endsWith("abab"), "the tail survives the exit");
});

test("captures a small output exactly", () => {
  const out = captureStdout(process.execPath, ["-e", 'process.stdout.write("type A {}\\n")']);
  assert.equal(out, "type A {}\n");
  assert.equal(captureStdout(process.execPath, ["-e", ""]), "", "no output is the empty string");
});

test("a failing CLI fails the capture instead of yielding partial output", () => {
  assert.throws(() => captureStdout(process.execPath, writeThenExit(1 << 16, 3)));
  assert.throws(() => captureStdout("typediagram-does-not-exist", []));
});

// The generator passes an absolute model path, which holds spaces when the
// checkout does. On Windows a `.cmd` shim runs through cmd.exe, which must
// hand the program back exactly the arguments it was given.
test("arguments reach a Windows .cmd shim intact", { skip: process.platform !== "win32" }, () => {
  const dir = mkdtempSync(join(tmpdir(), "run-cli-shim-"));
  try {
    const shim = join(dir, "echo args.cmd");
    const echo = join(dir, "echo.js");
    writeFileSync(echo, "process.stdout.write(JSON.stringify(process.argv.slice(2)));");
    writeFileSync(shim, `@"${process.execPath}" "${echo}" %*\r\n`);
    const args = ["--to", "rust", String.raw`C:\checkout with spaces\model.td`, 'a "quoted" word', String.raw`trailing\\`.slice(0, -1)];
    assert.deepEqual(JSON.parse(captureStdout(shim, args)), args);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("quoting for cmd.exe escapes quotes and the backslashes before them", () => {
  assert.equal(quoteForCmd("plain"), '"plain"');
  assert.equal(quoteForCmd("two words"), '"two words"');
  assert.equal(quoteForCmd('say "hi"'), '"say \\"hi\\""');
  assert.equal(quoteForCmd('a\\"b'), '"a\\\\\\"b"');
  assert.equal(quoteForCmd("dir\\"), '"dir\\\\"');
});
