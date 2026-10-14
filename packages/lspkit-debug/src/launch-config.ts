// Launch-configuration synthesis and normalization, language-neutral.
//
// Pressing F5 with no .vscode/launch.json hands the debug provider an EMPTY
// configuration; every LspKit editor must fill it from the active document so a
// zero-config debug works. This file owns that pure transform (Osprey's
// applyDefaultOspreyDebugConfig, Basilisk's applyDebugConfigDefaults, and
// SharpLsp's resolveDebugConfiguration defaulting are all instances of it).

import * as path from "path";

/** The fields of the active editor's document the synthesizer reads. */
export interface DebugDocumentLike {
  languageId: string;
  fileName: string;
}

/** A minimal active-editor stand-in. */
export interface ActiveEditorLike {
  document: DebugDocumentLike;
}

/** A debug configuration is an open bag of fields (DAP-defined + custom). */
export type DebugConfigLike = Record<string, unknown>;

/** Policy describing how to fill an empty config for one language. */
export interface LaunchConfigPolicy {
  /** Debug type id, e.g. "osprey". */
  type: string;
  /** Human-facing config name, e.g. "Debug Osprey File". */
  name: string;
  /** Request kind; defaults to "launch". */
  request?: string;
  /** True when the document belongs to this language. */
  isLanguageDocument: (document: DebugDocumentLike) => boolean;
  /**
   * Extra fields derived from the resolved program path, e.g. `{ cwd }`.
   * Merged into the synthesized config. Defaults to `{ cwd: dirname(program) }`.
   */
  deriveFields?: (program: string) => DebugConfigLike;
}

function defaultDerivedFields(program: string): DebugConfigLike {
  return { cwd: path.dirname(program) };
}

/** True when a config has none of type/request/name (a blank F5 config). */
export function isBlankConfig(config: DebugConfigLike): boolean {
  return !config.type && !config.request && !config.name;
}

/**
 * Fill a blank config from the active editor when it holds a document of this
 * language; an already-populated config is returned untouched. Pure: it mutates
 * and returns `config` but reads nothing global.
 */
export function applyLaunchConfigDefaults(
  config: DebugConfigLike,
  activeEditor: ActiveEditorLike | undefined,
  policy: LaunchConfigPolicy,
): DebugConfigLike {
  if (!isBlankConfig(config)) {
    return config;
  }
  if (!activeEditor || !policy.isLanguageDocument(activeEditor.document)) {
    return config;
  }
  const program = activeEditor.document.fileName;
  const derive = policy.deriveFields ?? defaultDerivedFields;
  config.type = policy.type;
  config.name = policy.name;
  config.request = policy.request ?? "launch";
  config.program = program;
  return Object.assign(config, derive(program));
}

/** Build a complete default config for a given document (or `{}` if foreign). */
export function defaultLaunchConfigForEditor(
  activeEditor: ActiveEditorLike | undefined,
  policy: LaunchConfigPolicy,
): DebugConfigLike {
  return applyLaunchConfigDefaults({}, activeEditor, policy);
}

/**
 * A stable, per-source output path for a compiled debug artifact, e.g.
 * `<dir>/.lspkit-debug/<basename><exe>`. `subdir` defaults to ".lspkit-debug".
 */
export function defaultDebugOutputPath(
  program: string,
  platform: NodeJS.Platform,
  subdir = ".lspkit-debug",
): string {
  const exe = platform === "win32" ? ".exe" : "";
  const base = path.basename(program, path.extname(program));
  return path.join(path.dirname(program), subdir, `${base}${exe}`);
}
