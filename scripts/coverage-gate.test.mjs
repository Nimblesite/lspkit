// Coverage-gate regression: coarse CLI tests that drive the REAL GNU make
// `_coverage_check` target of this repo against fixture LCOV/JSON files on
// disk. The checker used to fail open: with jq missing, with corrupt JSON, or
// with nonnumeric/out-of-range/fractional thresholds it exited 0 and silently
// skipped every per-crate check (verified against published f693 on bare
// Ubuntu: live 81.82<94, server 77.19<98, sidecar 65.43<99 all passed).
// These tests pin the fail-closed contract so it cannot silently disappear.
//
// Cross-platform: the Makefile supplies the make binary and the POSIX shell
// (Git Bash on Windows, /bin/sh on Linux CI), so the same tests run in both:
//   node scripts/coverage-gate.test.mjs --make <make> --shell <shell>

import { spawnSync } from "node:child_process";
import { accessSync, constants, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, isAbsolute, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import assert from "node:assert/strict";
import test from "node:test";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoMakefile = join(repoRoot, "Makefile");
const shellDefault = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "/bin/sh";

function searchPath(command) {
  const exts = process.platform === "win32" ? [".exe", ".cmd", ".bat", ""] : [""];
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    for (const ext of exts) {
      const candidate = join(dir, command + ext);
      try {
        accessSync(candidate, constants.X_OK);
        return candidate;
      } catch {
        // keep scanning PATH
      }
    }
  }
  return null;
}

// Resolved BEFORE any child runs with an emptied PATH, so the missing-jq test
// really exercises the checker instead of failing to spawn make at all.
function resolveTool(spec, envKey, fallback) {
  const raw = spec ?? process.env[envKey] ?? fallback;
  if (isAbsolute(raw)) return raw;
  const found = searchPath(raw);
  assert.ok(found, `cannot resolve ${envKey} (${raw}) from PATH before emptying it`);
  return found;
}

// The caller controls artifact placement through the normal OS temp directory
// or an explicit coverage-gate fixture root.
function resolveTempRoot() {
  const dir = process.env.COVERAGE_GATE_TEMP_DIR ?? tmpdir();
  mkdirSync(dir, { recursive: true });
  return dir;
}

const { values } = parseArgs({ options: { make: { type: "string" }, shell: { type: "string" } } });
const makePath = resolveTool(values.make, "COVERAGE_GATE_MAKE", "make");
const shellPath = resolveTool(values.shell, "COVERAGE_GATE_SHELL", shellDefault);
const tempRoot = resolveTempRoot();

// Real LCOV record shape; the SF marker is exactly what the checker's awk
// accounting matches (`/crates/<crate>/src/`).
function lcovFor(coverage) {
  let text = "";
  for (const [crate, [hit, total]] of coverage) {
    text += `SF:/lcov-fixture/crates/${crate}/src/lib.rs\nLH:${hit}\nLF:${total}\nend_of_record\n`;
  }
  return text;
}

// Fixture thresholds always derive from the real coverage-thresholds.json, so
// the valid fixture exercises the actual configured crate list and floors.
function thresholdsJson(overrides) {
  const real = JSON.parse(readFileSync(join(repoRoot, "coverage-thresholds.json"), "utf8"));
  return JSON.stringify({ ...real, per_crate: { ...real.per_crate, ...overrides } }, null, 2) + "\n";
}

function runGate(dir, { emptyPath = false } = {}) {
  const env = { ...process.env };
  if (emptyPath) env.PATH = "";
  const result = spawnSync(makePath, ["-C", dir, "-f", repoMakefile, "_coverage_check", `SHELL=${shellPath}`, ".SHELLFLAGS=-c"], {
    cwd: dir,
    env,
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  return { status: result.status ?? -1, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
}

function withTempGate(files, run) {
  const dir = mkdtempSync(join(tempRoot, "coverage-gate-"));
  try {
    for (const [name, content] of files) writeFileSync(join(dir, name), content);
    return run(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// Crate names reported by the checker's OK:/FAIL: threshold verdict lines
// (padded by printf; "no lines" FAILs carry no percentage and never match).
function verdictCrates(output, verdict) {
  const crates = new Set();
  for (const line of output.split("\n")) {
    const match = line.match(new RegExp(`^${verdict}:\\s+(\\S+)\\s+\\d`));
    if (match) crates.add(match[1]);
  }
  return crates;
}

const configuredCrates = Object.keys(JSON.parse(readFileSync(join(repoRoot, "coverage-thresholds.json"), "utf8")).per_crate);
const allAt100 = new Map(configuredCrates.map((crate) => [crate, [2, 2]]));

test("a valid 100% fixture passes and prints every configured crate", () => {
  const gate = withTempGate(
    [
      ["coverage-thresholds.json", thresholdsJson({})],
      ["lcov.info", lcovFor(allAt100)],
    ],
    (dir) => runGate(dir),
  );
  assert.equal(gate.status, 0, `expected pass, got:\n${gate.stdout}\n${gate.stderr}`);
  assert.deepEqual(verdictCrates(gate.stdout, "OK"), new Set(configuredCrates));
  assert.equal(verdictCrates(gate.stdout, "FAIL").size, 0);
  assert.match(gate.stdout, />= /);
});

test("coverage below a per-crate floor fails that crate and the whole gate", () => {
  const gate = withTempGate(
    [
      ["coverage-thresholds.json", thresholdsJson({})],
      ["lcov.info", lcovFor(new Map(configuredCrates.map((crate) => [crate, crate === "lspkit-live" ? [1, 2] : [2, 2]])))],
    ],
    (dir) => runGate(dir),
  );
  assert.notEqual(gate.status, 0, `expected failure, got:\n${gate.stdout}`);
  assert.deepEqual(verdictCrates(gate.stdout, "FAIL"), new Set(["lspkit-live"]));
  assert.match(gate.stdout, /50\.00% < 94%/);
  assert.equal(verdictCrates(gate.stdout, "OK").size, configuredCrates.length - 1, "other crates stay reported OK");
  assert.match(gate.stdout, /below per-crate threshold/);
});

test("a missing lcov.info fails closed", () => {
  const gate = withTempGate([["coverage-thresholds.json", thresholdsJson({})]], (dir) => runGate(dir));
  assert.notEqual(gate.status, 0);
  assert.match(gate.stdout, /lcov\.info not found/);
});

test("a missing coverage-thresholds.json fails closed", () => {
  const gate = withTempGate([["lcov.info", lcovFor(allAt100)]], (dir) => runGate(dir));
  assert.notEqual(gate.status, 0);
  assert.match(gate.stdout, /coverage-thresholds\.json not found/);
});

// Runs make with an emptied PATH; make and the shell were resolved by absolute
// path beforehand, so a missing jq is what the checker itself must detect.
test("a missing jq fails closed instead of skipping every crate check", () => {
  const gate = withTempGate(
    [
      ["coverage-thresholds.json", thresholdsJson({})],
      ["lcov.info", lcovFor(allAt100)],
    ],
    (dir) => runGate(dir, { emptyPath: true }),
  );
  assert.notEqual(gate.status, 0, `fail-open: exited 0 with jq missing\n${gate.stdout}`);
  assert.match(gate.stdout + gate.stderr, /jq/);
  assert.doesNotMatch(gate.stdout, /OK:/, "no crate may be reported OK when the gate cannot run");
});

for (const invalid of [
  { name: "corrupt JSON", json: '{"per_crate":' },
  { name: "empty per_crate", json: '{"per_crate":{}}' },
  { name: "nonnumeric", overrides: { lspkit: "high" } },
  { name: "fractional", overrides: { "lspkit-live": 94.5 } },
  { name: "above 100", overrides: { lspkit: 150 } },
  { name: "negative", overrides: { "lspkit-server": -10 } },
  { name: "null", overrides: { lspkit: null } },
]) {
  test(`a ${invalid.name} threshold fails closed as a config error`, () => {
    const gate = withTempGate(
      [
        ["coverage-thresholds.json", invalid.json ?? thresholdsJson(invalid.overrides)],
        ["lcov.info", lcovFor(allAt100)],
      ],
      (dir) => runGate(dir),
    );
    assert.notEqual(gate.status, 0, `fail-open: exited 0 on ${invalid.name} threshold\n${gate.stdout}`);
    assert.match(gate.stdout, /per_crate must be a nonempty map of integer thresholds 0\.\.100/);
    assert.doesNotMatch(gate.stdout, /OK:/, "no crate may be judged against an invalid floor");
  });
}

process.stdout.write(`coverage-gate CLI regression: make=${makePath} shell=${shellPath} temp=${tempRoot}\n`);
