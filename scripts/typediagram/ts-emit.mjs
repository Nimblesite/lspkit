// Implements [PROF-WIRE-GEN-TS].
//
// Decorates typeDiagram's TypeScript output: fields become `readonly`, an
// `Option<T>` field (serialised by serde only when present) becomes an
// optional `field?: T`, and every interface, alias and field carries the doc
// comment written in the .td model.

import { requireDoc } from "./td-docs.mjs";

/** Rewrite one interface field line. */
function fieldLine(item, line, docs) {
  const trimmed = line.trim().replace(/;$/u, "");
  const colon = trimmed.indexOf(":");
  const name = trimmed.slice(0, colon).trim();
  const type = trimmed.slice(colon + 1).trim();
  const optional = type.endsWith(" | undefined");
  const bare = optional ? type.slice(0, -" | undefined".length) : type;
  return [
    `  /** ${requireDoc(docs, `${item}.${name}`)} */`,
    `  readonly ${name}${optional ? "?" : ""}: ${bare};`,
  ];
}

/** Make the inline fields of a union variant readonly and optional-aware. */
function variantLine(line) {
  return line
    .replace(/(\w+): ([^;}]+) \| undefined/gu, "$1?: $2")
    .replace(/\{ kind:/u, "{ readonly kind:")
    .replace(/; (\w+)(\??):/gu, "; readonly $1$2:");
}

/**
 * Turn raw typeDiagram TypeScript into the decorated module body.
 * @param {string} raw `typediagram --to typescript` output.
 * @param {Map<string, string>} docs Docs extracted from the .td model.
 */
export function decorateTs(raw, docs) {
  const out = [];
  let item = null;
  for (const line of raw.split("\n")) {
    const iface = line.match(/^export interface (\w+) \{$/u);
    const alias = line.match(/^export type (\w+) =/u);
    if (iface) {
      item = iface[1];
      out.push(`/** ${requireDoc(docs, item)} */`, line);
    } else if (alias) {
      out.push(`/** ${requireDoc(docs, alias[1])} */`, line);
    } else if (line === "}") {
      out.push(line);
      item = null;
    } else if (item && line.trim() !== "") {
      out.push(...fieldLine(item, line, docs));
    } else {
      out.push(line.trim().startsWith("|") ? variantLine(line) : line);
    }
  }
  return out.join("\n");
}

/** TypeScript constants for the method table and error codes. */
export function tsConstants(table) {
  const methods = Object.entries(table.methods)
    .map(([name, value]) => `  ${name}: "${value}",`)
    .join("\n");
  const errors = Object.entries(table.errors)
    .map(([name, value]) => `  ${name}: ${value},`)
    .join("\n");
  return [
    "/** JSON-RPC method names of the lsp-profiler protocol. [PROF-WIRE-METHODS] */",
    `export const METHODS = {\n${methods}\n} as const;`,
    "",
    "/** JSON-RPC error codes of the lsp-profiler protocol. [PROF-ERRORS] */",
    `export const ERROR_CODES = {\n${errors}\n} as const;`,
  ].join("\n");
}
