// Every `[PROF-…]` citation in the profiler's code and docs must resolve to
// exactly one heading in docs/specs. Implements the spec-citation gate of
// [PROF-TESTING]. Scoped to the profiler's own paths: the rest of lspkit cites
// its own spec.
//
//   node scripts/profiler-spec-citations.mjs   (exit 1 on any undefined or duplicate ID)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const SKIP = new Set(["node_modules", "out", "bin", "obj", "target", ".vscode-test", "dist"]);
const SOURCE_EXT = new Set([".rs", ".ts", ".mts", ".mjs", ".cs", ".md", ".toml", ".csproj"]);

function* walk(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (SKIP.has(entry.name)) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(full);
    else if (SOURCE_EXT.has(path.extname(entry.name)) || entry.name === "Makefile") yield full;
  }
}

const headings = new Map();
for (const file of walk(path.join(root, "docs", "specs"))) {
  for (const match of fs.readFileSync(file, "utf8").matchAll(/^#+ \[(PROF-[A-Z0-9-]+)\]/gm)) {
    headings.set(match[1], (headings.get(match[1]) ?? 0) + 1);
  }
}
const duplicates = [...headings].filter(([, count]) => count > 1).map(([id]) => id);

// A spec-ID-shaped citation (`[WORD-WORD…]`) from any other spec — e.g. one
// carried over from a host product or from lspkit — resolves to nothing here, so it counts
// as undefined too.
const CITATION = /\[([A-Z][A-Z0-9]{2,}(?:-[A-Z0-9]+)+)\]/g;

const PROFILER_PATHS = [
  "crates/lsp-profiler",
  "crates/lsp-profiler-dotnet",
  "packages/lsp-profiler-ui",
  "dotnet/LspProfiler.DotNet.Engine",
  "dotnet/LspProfiler.DotNet.Engine.Tests",
  "dotnet/fixtures",
  "docs/models",
  "docs/specs/LSP-PROFILER-SPEC.md",
  "docs/plans/LSP-PROFILER-PLAN.md",
  "scripts/typediagram",
];

function* profilerFiles() {
  for (const relative of PROFILER_PATHS) {
    const base = path.join(root, relative);
    if (!fs.existsSync(base)) continue;
    if (fs.statSync(base).isDirectory()) yield* walk(base);
    else yield base;
  }
}

const undefinedIds = new Map();
for (const file of profilerFiles()) {
  for (const match of fs.readFileSync(file, "utf8").matchAll(CITATION)) {
    if (!headings.has(match[1])) {
      const where = undefinedIds.get(match[1]) ?? new Set();
      where.add(path.relative(root, file));
      undefinedIds.set(match[1], where);
    }
  }
}

if (duplicates.length > 0) console.error(`duplicate spec headings: ${duplicates.join(", ")}`);
for (const [id, files] of undefinedIds) console.error(`undefined citation [${id}] in ${[...files].join(", ")}`);
if (duplicates.length > 0 || undefinedIds.size > 0) process.exit(1);
console.log(`Every spec citation resolves to exactly one definition (${headings.size} headings).`);
