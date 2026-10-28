// Tests [PROF-WIRE-GEN] and [PROF-WIRE-GEN-DOCS] end to end: generating from the
// real model emits every declaration, each carrying its `#` comment from the
// .td, in both languages. macOS once cut the TypeScript off at the 64 KiB pipe
// buffer, which surfaced in a release build as TS1128 in wire-generated.ts.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const inRepo = (path) => fileURLToPath(new URL(`../../${path}`, import.meta.url));

function generate() {
  execFileSync(process.execPath, [inRepo("scripts/typediagram/generate.mjs")], { stdio: "ignore" });
  return {
    ts: readFileSync(inRepo("packages/lsp-profiler-ui/src/wire-generated.ts"), "utf8"),
    rust: readFileSync(inRepo("crates/lsp-profiler/src/wire_generated.rs"), "utf8"),
  };
}

test("the model's last type arrives whole and documented in both languages", () => {
  const { ts, rust } = generate();
  const doc = "Parameters of `lspProfiler/memory/ingest`.";
  assert.ok(ts.includes(`/** ${doc} */\nexport interface MemoryIngestParams {`), "TypeScript reaches the end of the model");
  assert.ok(rust.includes(`/// ${doc}`), "Rust carries the same doc");
  assert.ok(rust.includes("pub struct MemoryIngestParams"), "Rust reaches the end of the model");
});

const VARIANT_DOCS = ["Attach to a process and stream samples.", "Attach succeeded.", "Samples from one sampling tick."];

test("variants that open a body are documented like the types around them", () => {
  const { rust } = generate();
  for (const doc of VARIANT_DOCS) assert.ok(rust.includes(`/// ${doc}`), `Rust documents "${doc}"`);
});

test("TypeScript documents union variants like Rust does", () => {
  const { ts } = generate();
  for (const doc of VARIANT_DOCS) assert.ok(ts.includes(`/** ${doc} */`), `TypeScript documents "${doc}"`);
  const attach = [
    '  /** Attach to a process and stream samples. */',
    "  | {",
    '      readonly kind: "Attach";',
    "      /** Target process. */",
    "      readonly pid: number;",
    "      /** Samples per second. */",
    "      readonly sample_rate: number;",
  ].join("\n");
  assert.ok(ts.includes(attach), "each variant field carries its own doc, readonly, one per line");
  assert.ok(ts.includes('  /** Stop sampling and exit. */\n  | { readonly kind: "Stop" };'), "a unit variant closes the union");
  assert.ok(!/\| \{ kind:/u.test(ts), "no variant keeps a writable discriminant");
});

test("generating again leaves the outputs byte for byte the same", () => {
  const first = generate();
  assert.deepEqual(generate(), first, "generation is deterministic, so cargo never rebuilds for nothing");
});
