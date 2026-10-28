// Implements [PROF-WIRE-GEN-TS].
//
// Decorates typeDiagram's TypeScript output: fields become `readonly`, an
// `Option<T>` field (serialised by serde only when present) becomes an
// optional `field?: T`, and every interface, alias, union variant and field
// carries the doc comment written in the .td model, as the Rust side does.

import { members } from "./members.mjs";
import { requireDoc } from "./td-docs.mjs";

const UNDEFINED = " | undefined";

/** One documented, readonly member: an interface field or a variant field. */
function memberLines(indent, key, name, type, docs) {
  const optional = type.endsWith(UNDEFINED);
  const bare = optional ? type.slice(0, -UNDEFINED.length) : type;
  return [
    `${indent}/** ${requireDoc(docs, key)} */`,
    `${indent}readonly ${name}${optional ? "?" : ""}: ${bare};`,
  ];
}

/** Rewrite one interface field line. */
function fieldLine(item, line, docs) {
  const [[name, type]] = members(line.trim(), ";");
  return memberLines("  ", `${item}.${name}`, name, type, docs);
}

/**
 * One union member, `| { kind: "Attach"; pid: number }`, documented like the
 * Rust variant it mirrors: the variant's doc above it and, when it carries
 * fields, one documented readonly field per line. [PROF-WIRE-GEN-DOCS]
 */
function variantLines(alias, line, docs) {
  const trimmed = line.trim();
  const end = trimmed.endsWith(";") ? ";" : "";
  const body = trimmed.slice(trimmed.indexOf("{") + 1, trimmed.lastIndexOf("}"));
  const [[, tag], ...fields] = members(body, ";");
  const variant = JSON.parse(tag);
  const head = `  /** ${requireDoc(docs, `${alias}.${variant}`)} */`;
  if (fields.length === 0) return [head, `  | { readonly kind: ${tag} }${end}`];
  const lines = fields.flatMap(([name, type]) =>
    memberLines("      ", `${alias}.${variant}.${name}`, name, type, docs),
  );
  return [head, "  | {", `      readonly kind: ${tag};`, ...lines, `    }${end}`];
}

/** The declaration a line opens: an interface, an alias, or neither. */
function opened(line) {
  const iface = line.match(/^export interface (\w+) \{$/u);
  if (iface) return { kind: "interface", name: iface[1] };
  const alias = line.match(/^export type (\w+) =/u);
  return alias ? { kind: "alias", name: alias[1] } : null;
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
    const decl = opened(line);
    if (decl) {
      item = decl;
      out.push(`/** ${requireDoc(docs, decl.name)} */`, line);
    } else if (line === "" || line === "}") {
      out.push(line);
      item = null;
    } else if (item?.kind === "interface") {
      out.push(...fieldLine(item.name, line, docs));
    } else if (item?.kind === "alias" && line.trim().startsWith("|")) {
      out.push(...variantLines(item.name, line, docs));
    } else {
      out.push(line);
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
