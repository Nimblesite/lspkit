// DAP adapter binary resolution, language-neutral.
//
// Every LspKit editor needs the same resolution chain to find its debug
// adapter executable (lldb-dap for Osprey, netcoredbg for SharpLsp, debugpy for
// Basilisk): a launch-config override, then a user setting, then PATH, then an
// optional platform probe (e.g. `xcrun -f`), then a list of common install
// locations. This file owns that chain so no extension re-implements it.

import * as path from "path";

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

/** First directory on PATH that contains `command`, or undefined. */
export function findExecutableOnPath(
  command: string,
  env: NodeJS.ProcessEnv,
  existsSync: (filePath: string) => boolean,
): string | undefined {
  for (const dir of (env.PATH ?? "").split(path.delimiter)) {
    if (!dir) {
      continue;
    }
    const candidate = path.join(dir, command);
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

function resolveConfigured(
  configured: string,
  req: AdapterResolutionRequest,
): string | undefined {
  if (looksLikePath(configured)) {
    return req.existsSync(configured) ? configured : undefined;
  }
  return findExecutableOnPath(configured, req.env, req.existsSync);
}

function probePlatform(req: AdapterResolutionRequest): string | undefined {
  if (!req.probe) {
    return undefined;
  }
  try {
    const resolved = req.probe("xcrun", ["-f", req.commandName]);
    return resolved && req.existsSync(resolved) ? resolved : undefined;
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
  const configured = req.configValue || req.settingValue;
  if (configured) {
    return resolveConfigured(configured, req);
  }

  const names = [req.commandName, ...(req.legacyNames ?? [])];
  for (const name of names) {
    const onPath = findExecutableOnPath(name, req.env, req.existsSync);
    if (onPath) {
      return onPath;
    }
  }

  const probed = probePlatform(req);
  if (probed) {
    return probed;
  }

  return req.candidates.find(req.existsSync);
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

/** A precise, actionable "adapter not found" message. */
export function missingAdapterMessage(input: MissingAdapterMessageInput): string {
  const checked = ["launch config", "VS Code setting", "PATH", ...(input.checkedSources ?? [])];
  const configured = input.configuredValue
    ? ` Configured ${input.settingKey}: ${input.configuredValue}.`
    : "";
  return (
    `${input.adapterName} was not found. ${input.installHint} or set ${input.settingKey} ` +
    `to an existing ${input.adapterName} executable. Checked ${checked.join(", ")}.${configured}`
  );
}
