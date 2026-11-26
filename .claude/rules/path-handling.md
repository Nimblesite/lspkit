# 🔥 Path handling is centralized: `lspkit::path` only

Spec: [PATH-CENTRAL](../../docs/specs/lspkit-spec.md#path-central-path-handling). This rule is not optional and has no exceptions.

Every file path operation in this workspace lives in `crates/lspkit/src/path.rs`, and nowhere else: not in any other crate, `test-support/` file or example.

`lspkit::path` owns:

- path ↔ `file://` URI conversion: `path_to_uri`, `uri_to_path`, `document_uri`;
- resolving a relative path against a base: `resolve_against`;
- canonicalization and normalization: `canonical`, `normalize_path`;
- containment and equality between paths: `is_within`, `same_path`, `path_in_roots`;
- rebasing from one root to another: `rebase`;
- walking up a tree to find a file: `find_ancestor`.

## Never

- Never write a path or URI helper outside `lspkit::path`, however small. This covers production code, tests, examples and `test-support/`. A `strip_prefix` rebase, a `canonicalize` call, a `starts_with(root)` or `==` comparison of two paths, an `ancestors()`/`parent()` walk, a `format!("file://…")` built from a real path, and a hand-rolled percent-encoder in a caller are all defects.
- Never add a path or URL crate (`url`, `path-clean`, `dunce`, `normpath`, …). `lspkit::path` implements [URI-AUTHORITY] and [URI-WINDOWS] itself.
- Never wrap a central function in another crate. Re-export it instead, as `lspkit_server::uri` does.

## Always

- When you need a path operation `lspkit::path` lacks, add it there first, with tests. Then call it.
- Spell every file's URI with `lspkit::path::path_to_uri`, so the profiler, the diagnostics bus and the VFS agree byte for byte ([PATH-CENTRAL-ONE-URI]).
- Review every diff for loose path handling, and move any you find into `lspkit::path`.

## TypeScript (`packages/lsp-profiler-ui`)

The same rule, one module: `src/paths.ts` ([PROF-UI-PATHS]). It owns `editorPathKey`, `pathKeyer`, `comparablePath`, `pathRules` and `looksLikePath`. Never write `replaceAll("\\", "/")`, `.toLowerCase()` on a path, `path.win32`/`path.posix` selection or `realpath` keying anywhere else. Build editor URIs only with `vscode.Uri.file(path)`, never from a string. Keep `paths.ts` free of `vscode` so `npm run test:paths` runs it under plain mocha.

## The only exceptions

- Std accessors that read one already-resolved path's own parts (`file_name`, `file_stem`, `extension`, a single `parent`), and `join` with a plain relative name.
- The tests of `lspkit::path` and `src/paths.ts` themselves, which use the OS (`std::fs::canonicalize`, `realpath`) as the oracle they check against.
- A literal `file:///…` URI for a synthetic document that exists on no disk. In a test or audit fixture it is wire data, not path handling.
