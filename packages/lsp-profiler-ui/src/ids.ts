// Implements [PROF-UI-IDS]. Every identifier the library registers or sends is
// derived from the host's prefix, so two hosts (two language servers) coexist
// in one window without a clash and the library never names a product.

let prefix = "lspProfiler";
let brandName = "Profiler";
let section = "lspProfiler";
let debuggerType = "";

/** Install the host's identity. Called once by `createProfilerStore`. */
export function installIds(ids: {
  readonly idPrefix: string;
  readonly brand: string;
  readonly configSection: string;
  readonly debugType: string;
}): void {
  prefix = ids.idPrefix;
  brandName = ids.brand;
  section = ids.configSection;
  debuggerType = ids.debugType;
}

/** The host's product name for user-facing messages, e.g. `SharpLsp`. */
export function brand(): string {
  return brandName;
}

/** The settings section the profiler reads, e.g. `sharplsp` → `sharplsp.profiler.sampleRate`. */
export function configSection(): string {
  return section;
}

/** The host's debug adapter type, used to recognise its own debug sessions. */
export function debugType(): string {
  return debuggerType;
}

/** A client-side command the library registers: `<prefix>.profiler.<name>`. */
export function cmd(name: string): string {
  return `${prefix}.profiler.${name}`;
}

/** A context key the library sets: `<prefix>.profiler.<name>`. */
export function ctx(name: string): string {
  return `${prefix}.profiler.${name}`;
}

/** The processes view ID: `<prefix>.profiler.processes`. */
export function viewId(): string {
  return `${prefix}.profiler.processes`;
}

/** A server command of the shared protocol: `<prefix>.lspProfiler.<area>.<action>`. */
export function lsp(name: string): string {
  return `${prefix}.lspProfiler.${name}`;
}

/** An object whose properties resolve `lsp(name)` on read, so a module-level table follows the installed prefix. */
export function lspTable<const T extends Record<string, string>>(names: T): { readonly [K in keyof T]: string } {
  const table = {} as { [K in keyof T]: string };
  for (const key of Object.keys(names) as (keyof T)[]) {
    Object.defineProperty(table, key, { enumerable: true, get: () => lsp(names[key]) });
  }
  return table;
}

/** The URI scheme of process rows (file decorations). */
export function processUriScheme(): string {
  return `${prefix}-process`;
}
