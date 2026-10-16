// Tests [PROF-WIRE-GEN]: the generator must see every byte the typeDiagram CLI
// prints, or it writes a truncated wire model that no longer compiles.
import assert from "node:assert/strict";
import test from "node:test";

import { captureStdout } from "./run-cli.mjs";

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
