// Implements [PROF-WIRE-GEN-RUST].
//
// Decorates typeDiagram's plain Rust output so it compiles under the
// workspace lints and round-trips through serde exactly as the TypeScript
// side expects: every item derives serde, unions are internally tagged on
// `kind` (matching typeDiagram's TypeScript discriminant), absent `Option`
// fields are omitted rather than serialised as `null`, and every item, field
// and variant carries the doc comment written in the .td model.

import { members } from "./members.mjs";
import { requireDoc } from "./td-docs.mjs";

const DERIVES = "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]";
const OPTION_ATTR = '#[serde(default, skip_serializing_if = "Option::is_none")]';

/** Split `a: A, b: Option<B<C, D>>` at top-level commas into [name, type] pairs. */
function splitFields(blob) {
  return members(blob, ",");
}

/** Emit one documented field line (struct field or variant field). */
function fieldLines(indent, key, name, type, docs, isPub) {
  const out = [`${indent}/// ${requireDoc(docs, key)}`];
  if (type.startsWith("Option<")) out.push(`${indent}${OPTION_ATTR}`);
  out.push(`${indent}${isPub ? "pub " : ""}${name}: ${type},`);
  return out;
}

/** Emit a variant line, expanding inline struct variants one field per line. */
function variantLines(item, line, docs) {
  const trimmed = line.trim().replace(/,$/u, "");
  const brace = trimmed.indexOf("{");
  const name = brace < 0 ? trimmed : trimmed.slice(0, brace).trim();
  const head = [`    /// ${requireDoc(docs, `${item}.${name}`)}`];
  if (brace < 0) return [...head, `    ${name},`];
  const fields = splitFields(trimmed.slice(brace + 1, trimmed.lastIndexOf("}")));
  const body = fields.flatMap(([field, type]) =>
    fieldLines("        ", `${item}.${name}.${field}`, field, type, docs, false),
  );
  return [...head, `    ${name} {`, ...body, "    },"];
}

/** Emit the attributes that precede an item declaration. */
function itemHead(kind, name, docs) {
  const head = [`/// ${requireDoc(docs, name)}`, DERIVES];
  if (kind === "enum") head.push('#[serde(tag = "kind")]');
  return head;
}

/**
 * Turn raw typeDiagram Rust into the decorated module body.
 * @param {string} raw `typediagram --to rust` output.
 * @param {Map<string, string>} docs Docs extracted from the .td model.
 */
export function decorateRust(raw, docs) {
  const out = [];
  let item = null;
  let kind = null;
  for (const line of raw.split("\n")) {
    const decl = line.match(/^pub (struct|enum) (\w+) \{$/u);
    if (decl) {
      [kind, item] = [decl[1], decl[2]];
      out.push(...itemHead(kind, item, docs), line);
    } else if (line.startsWith("pub type ")) {
      const name = line.slice("pub type ".length, line.indexOf("=")).trim();
      out.push(`/// ${requireDoc(docs, name)}`, line);
    } else if (line === "}") {
      out.push(line);
      item = null;
    } else if (item && kind === "struct" && line.trim().startsWith("pub ")) {
      const [name, type] = splitFields(line.trim().slice(4).replace(/,$/u, ""))[0];
      out.push(...fieldLines("    ", `${item}.${name}`, name, type, docs, true));
    } else if (item && kind === "enum" && line.trim() !== "") {
      out.push(...variantLines(item, line, docs));
    } else {
      out.push(line);
    }
  }
  return out.join("\n");
}

/** Rust constants for the method table and error codes. */
export function rustConstants(table) {
  const methods = Object.entries(table.methods).map(
    ([name, value]) => `    /// \`${value}\`\n    pub const ${name}: &str = "${value}";`,
  );
  const errors = Object.entries(table.errors).map(
    ([name, value]) => `    /// JSON-RPC error code \`${value}\`.\n    pub const ${name}: i64 = ${value};`,
  );
  return [
    "/// JSON-RPC method names of the lsp-profiler protocol. [PROF-WIRE-METHODS]",
    "pub mod methods {",
    ...methods,
    "}",
    "",
    "/// JSON-RPC error codes of the lsp-profiler protocol. [PROF-ERRORS]",
    "pub mod error_codes {",
    ...errors,
    "}",
  ].join("\n");
}
