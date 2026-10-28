// Implements [PROF-WIRE-GEN].
//
// Reads the members of one declaration typeDiagram printed on a single line:
// a Rust struct variant's `a: A, b: Option<B<C, D>>` or a TypeScript union
// member's `kind: "Attach"; pid: number`. Both emitters decorate from these
// parsed pairs rather than rewriting the line.

const OPENERS = "<{[(";
const CLOSERS = ">}])";

/**
 * Split `blob` at each `separator` outside any `<>`, `{}`, `[]` or `()`.
 * @param {string} blob
 * @param {string} separator One character.
 * @returns {string[]}
 */
export function splitTopLevel(blob, separator) {
  const parts = [];
  let depth = 0;
  let current = "";
  for (const char of blob) {
    depth += OPENERS.includes(char) ? 1 : CLOSERS.includes(char) ? -1 : 0;
    if (char === separator && depth === 0) {
      parts.push(current);
      current = "";
    } else {
      current += char;
    }
  }
  if (current.trim() !== "") parts.push(current);
  return parts;
}

/**
 * The `[name, type]` pairs of `blob`'s members.
 * @param {string} blob
 * @param {string} separator
 * @returns {[string, string][]}
 */
export function members(blob, separator) {
  return splitTopLevel(blob, separator).map((part) => {
    const colon = part.indexOf(":");
    return [part.slice(0, colon).trim(), part.slice(colon + 1).trim()];
  });
}
