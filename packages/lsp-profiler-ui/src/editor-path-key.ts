// Implements [PROF-TESTING] path keying. See docs/specs/LSP-PROFILER-SPEC.md
/**
 * Keying a file path so a runtime's paths and the editor's agree.
 *
 * Every in-editor overlay the library paints — the CPU heat map, the memory track,
 * the leak badges — matches rows produced by a Python runtime against the
 * editors the user has open. Those two paths come from different producers and
 * are only textually identical on POSIX: the runtime reports the interpreter's
 * own filename, while `Uri.fsPath` hands back what VS Code resolved.
 *
 * On Windows they disagree in two ways — the drive letter's case, and 8.3 short
 * components — so a raw string compare NEVER matches there and the overlay
 * silently paints nothing: the data is correct, the editor just stays blank.
 * That is a whole-feature outage that looks like "no results", which is exactly
 * why it survived until win32 ran in CI.
 *
 * Shared rather than copied: the profiler learned this first, and the memory
 * decorations had the identical latent bug.
 */

import path from "node:path";
import { realpath } from "node:fs/promises";

/**
 * Key a path for cross-producer comparison.
 *
 * Windows paths are case-insensitive, so folding case there is sound; POSIX
 * paths are case-SENSITIVE, so it must not fold there.
 */
export async function editorPathKey(file: string): Promise<string> {
  return foldCase(await canonicalPath(file));
}

function foldCase(canonical: string): string {
  return process.platform === "win32" ? canonical.toLowerCase() : canonical;
}

async function keyEntry(file: string): Promise<[string, string]> {
  return [file, await editorPathKey(file)];
}

/**
 * Resolve symlinks on every platform, and expand Windows 8.3 components.
 *
 * Case-folding alone is not enough. The two producers reach the same file by
 * different routes: `os.tmpdir()` yields the short form Windows keeps for
 * legacy callers (`C:\Users\RUNNER~1\…`), while a path the debug adapter or the
 * editor resolved carries the long one (`C:\Users\runneradmin\…`). Those differ
 * in more than case, so `path.resolve` leaves them unequal and the overlay
 * matches nothing.
 *
 * `realpath` is what collapses the two spellings — it asks the
 * filesystem for the name it actually records. It therefore touches disk and
 * throws for a path that no longer exists, so a file that has since been
 * deleted falls back to the resolved-but-unexpanded form rather than taking the
 * whole decoration pass down with it. Callers memoise (see `pathKeyer`) so this
 * costs one lookup per distinct file, not one per hot line.
 */
async function canonicalPath(file: string): Promise<string> {
  try {
    return await realpath(file);
  } catch {
    return path.resolve(file);
  }
}

/**
 * `editorPathKey` memoised for the length of one decoration pass.
 *
 * A profile or snapshot carries many rows across few files, and the expansion
 * above hits the filesystem — so the same handful of paths would otherwise be
 * looked up hundreds of times. The cache is per-pass rather than module-level
 * so a file that moves between runs is never matched against a stale name.
 * Every distinct file in `files` is resolved up front, so the returned lookup
 * is pure; a file outside that set keys exactly as an unresolvable path does.
 */
export async function pathKeyer(files: readonly string[]): Promise<(file: string) => string> {
  const distinct = [...new Set(files)];
  const cache = new Map<string, string>(await Promise.all(distinct.map(async (file) => keyEntry(file))));
  return (file: string): string => cache.get(file) ?? foldCase(path.resolve(file));
}

/** The editor surface a decoration pass paints: only its document's path matters here. */
export interface PathedEditor { readonly document: { readonly uri: { readonly fsPath: string } }; }

/** A `pathKeyer` over a pass's located rows and the editors it will paint. */
export async function rowAndEditorKeyer(
  rows: readonly { readonly file: string }[],
  editors: readonly PathedEditor[],
): Promise<(file: string) => string> {
  return pathKeyer([...rows.map(({ file }) => file), ...editors.map(({ document }) => document.uri.fsPath)]);
}

/** Group items by normalised file path (see `pathKey`) into a map. */
export function groupByFile<Item extends { readonly file: string }>(
  items: readonly Item[],
  keyOf: (file: string) => string,
): Map<string, Item[]> {
  const grouped = new Map<string, Item[]>();
  for (const item of items) {
    const key = keyOf(item.file);
    const existing = grouped.get(key);
    if (existing === undefined) {
      grouped.set(key, [item]);
    } else {
      existing.push(item);
    }
  }
  return grouped;
}
