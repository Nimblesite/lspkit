// Implements [PROF-WIRE-GEN-DOCS].
//
// typeDiagram drops `#` comments when it emits code. This module reads them
// back from the .td model so the generated Rust and TypeScript carry the same
// documentation. A comment block documents the declaration directly below it:
// a `type`/`union` (key `Name`), a field (`Name.field`), a variant
// (`Name.Variant`) or a variant field (`Name.Variant.field`).

const OPENERS = ["type ", "union "];

/** Strip a trailing `{` and surrounding whitespace from a declaration head. */
function headName(text) {
  const head = text.trim();
  return (head.endsWith("{") ? head.slice(0, -1) : head).trim();
}

/** Classify one trimmed .td line. */
function classify(line) {
  if (line.startsWith("#")) return { kind: "comment", text: line.slice(1).trim() };
  if (line === "" || line === "typeDiagram") return { kind: "blank" };
  if (line === "}") return { kind: "close" };
  const opener = OPENERS.find((prefix) => line.startsWith(prefix));
  if (opener) return { kind: "item", name: headName(line.slice(opener.length)) };
  const colon = line.indexOf(":");
  if (colon > 0) return { kind: "field", name: line.slice(0, colon).trim() };
  return { kind: "variant", name: headName(line), opens: line.endsWith("{") };
}

/** Build the documentation key for a declaration at the current nesting. */
function keyFor(stack, name) {
  return [...stack, name].join(".");
}

/**
 * Parse the .td source into a `Map<key, doc>`.
 * @param {string} source .td file contents.
 * @returns {Map<string, string>}
 */
export function extractDocs(source) {
  const docs = new Map();
  const stack = [];
  let pending = [];
  for (const raw of source.split("\n")) {
    const line = classify(raw.trim());
    if (line.kind === "comment") {
      pending.push(line.text);
      continue;
    }
    // A blank line ends a comment block: only comments directly above a
    // declaration document it, so section banners never leak into docs.
    if (line.kind === "blank") {
      pending = [];
      continue;
    }
    if (line.kind === "close") {
      stack.pop();
      pending = [];
      continue;
    }
    if (pending.length > 0) docs.set(keyFor(stack, line.name), pending.join(" "));
    pending = [];
    if (line.kind === "item" || (line.kind === "variant" && line.opens)) stack.push(line.name);
  }
  return docs;
}

/**
 * Look a doc up, failing the generation when the model left it undocumented.
 * @param {Map<string, string>} docs
 * @param {string} key
 */
export function requireDoc(docs, key) {
  const doc = docs.get(key);
  if (doc === undefined) {
    throw new Error(`typediagram-gen: add a '#' comment above '${key}' in docs/models/profiler.td`);
  }
  return doc;
}
