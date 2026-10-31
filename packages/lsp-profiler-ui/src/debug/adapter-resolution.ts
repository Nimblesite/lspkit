// DAP adapter binary resolution, language-neutral.
//
// Every LspKit editor needs the same resolution chain to find its debug
// adapter executable (lldb-dap for Osprey, netcoredbg for SharpLsp, debugpy for
// Basilisk): a launch-config override, then a user setting, then PATH, then an
// optional platform probe (e.g. `xcrun -f`), then a list of common install
// locations. This file owns that chain so no extension re-implements it.

import path from "node:path";

/** All host effects the resolver needs, injected so it is unit-testable. */
export interface AdapterResolutionRequest {
  /** Per-launch override from the debug configuration (highest priority). */
  configValue?: string;
  /** User/workspace setting value (second priority). */
  settingValue?: string;
  /** Bare adapter command, e.g. "lldb-dap" or "netcoredbg". */
  commandName: string;
  /** Alternate bare names to also try on PATH, e.g. ["lldb-vscode"]. */
  legacyNames?: string[];
  /** Common absolute install locations, scanned in order. */
  candidates: string[];
  /** Node platform string. */
  platform: NodeJS.Platform;
  /** Process environment (for PATH). */
  env: NodeJS.ProcessEnv;
  /** Filesystem existence check. */
  existsSync: (filePath: string) => boolean;
  /**
   * Optional platform tool probe, e.g. running `xcrun -f lldb-dap` on macOS.
   * Returns the resolved absolute path or undefined/throws when unavailable.
   */
  probe?: (command: string, args: readonly string[]) => string | undefined;
}

/** True when `value` is a filesystem path rather than a bare PATH command. */
export function looksLikePath(value: string): boolean {
  return value.includes("/") || value.includes("\\");
}

/** True when `value` holds actual text rather than being absent or empty. */
function hasText(value: string | undefined): value is string {
  return value !== undefined && value !== "";
}

/** First value that holds text, so an empty setting never shadows a real one. */
function firstNonEmpty(values: readonly (string | undefined)[]): string | undefined {
  return values.find((value) => hasText(value));
}

/** Split PATH into its non-empty directories. */
function pathDirectories(env: NodeJS.ProcessEnv): string[] {
  return (env.PATH ?? "").split(path.delimiter).filter((dir) => dir !== "");
}

/** First directory on PATH that contains `command`, or undefined. */
export function findExecutableOnPath(
  command: string,
  env: NodeJS.ProcessEnv,
  fileExists: (filePath: string) => boolean,
): string | undefined {
  return pathDirectories(env)
    .map((dir) => path.join(dir, command))
    .find((candidate) => fileExists(candidate));
}

function resolveConfigured(
  configured: string,
  req: AdapterResolutionRequest,
): string | undefined {
  const { existsSync: fileExists } = req;
  if (looksLikePath(configured)) {
    return fileExists(configured) ? configured : undefined;
  }
  return findExecutableOnPath(configured, req.env, fileExists);
}

/** Every bare command name to try on PATH, primary name first. */
function adapterCommandNames(req: AdapterResolutionRequest): string[] {
  return [req.commandName, ...(req.legacyNames ?? [])];
}

function resolveFromPathNames(req: AdapterResolutionRequest): string | undefined {
  const { existsSync: fileExists } = req;
  return adapterCommandNames(req)
    .map((name) => findExecutableOnPath(name, req.env, fileExists))
    .find((found) => hasText(found));
}

function findExistingCandidate(req: AdapterResolutionRequest): string | undefined {
  const { existsSync: fileExists } = req;
  return req.candidates.find((candidate) => fileExists(candidate));
}

function probePlatform(req: AdapterResolutionRequest): string | undefined {
  if (req.probe === undefined) {
    return undefined;
  }
  const { existsSync: fileExists } = req;
  try {
    const resolved = req.probe("xcrun", ["-f", req.commandName]);
    return hasText(resolved) && fileExists(resolved) ? resolved : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Resolve the adapter to an absolute, existing path, or undefined when nothing
 * usable is found. A configured value that is an explicit path but missing
 * resolves to undefined (so the caller surfaces a precise error rather than
 * spawning a non-existent binary).
 */
export function resolveAdapterPath(
  req: AdapterResolutionRequest,
): string | undefined {
  const configured = firstNonEmpty([req.configValue, req.settingValue]);
  if (configured !== undefined) {
    return resolveConfigured(configured, req);
  }
  return (
    resolveFromPathNames(req) ??
    probePlatform(req) ??
    findExistingCandidate(req)
  );
}

/**
 * Resolve to an absolute path when possible, otherwise the bare command name so
 * the OS can still try to spawn it from PATH at launch time. Mirrors the
 * lenient "always return something" callers (e.g. legacy descriptor factories).
 */
export function resolveAdapterCommand(req: AdapterResolutionRequest): string {
  return resolveAdapterPath(req) ?? req.commandName;
}

/** Inputs for {@link missingAdapterMessage}. */
export interface MissingAdapterMessageInput {
  /** Display name of the adapter, e.g. "lldb-dap". */
  adapterName: string;
  /** The setting key a user can set, e.g. "osprey.debug.lldbDapPath". */
  settingKey: string;
  /** How to obtain it, e.g. "Install LLVM/LLDB". */
  installHint: string;
  /** The configured value that failed, if any (echoed back). */
  configuredValue?: string;
  /** Extra probe sources checked, appended to the "Checked …" list. */
  checkedSources?: string[];
}

/** The "Checked …" list every message ends with. */
function checkedSourcesOf(input: MissingAdapterMessageInput): string {
  return ["launch config", "VS Code setting", "PATH", ...(input.checkedSources ?? [])].join(
    ", ",
  );
}

/** The "Configured …" clause, present only when a value was configured. */
function configuredClauseOf(input: MissingAdapterMessageInput): string {
  return hasText(input.configuredValue)
    ? ` Configured ${input.settingKey}: ${input.configuredValue}.`
    : "";
}

/** A precise, actionable "adapter not found" message. */
export function missingAdapterMessage(input: MissingAdapterMessageInput): string {
  return `${input.adapterName} was not found. ${input.installHint} or set ${input.settingKey} to an existing ${input.adapterName} executable. Checked ${checkedSourcesOf(input)}.${configuredClauseOf(input)}`;
}
