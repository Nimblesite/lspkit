# PLAN — building `lspkit` v0

> Behavior contract lives in [spec.md](spec.md). This file is the **execution** plan.
> The canonical in-flight task list is the **TODO** section at the bottom.

## Approach

v0 is **scaffold**: every crate compiles, the public surface is locked in, but feature implementations land in follow-up PRs. Trait/type skeletons carry `///` doc comments and `todo!()`-free placeholders (return `Err`, default values, or empty streams). No demo crate, no published version, no tests beyond a compile-only smoke test per crate.

## Phases

1. **Workspace bootstrap.** Create `Cargo.toml` (workspace), `rustfmt.toml`, `.gitignore`, `README.md`, `Makefile`. All generic — no consumer names, no language ecosystems.
2. **Trait + types.** Build `crates/lspkit/src/engine.rs` with `EngineApi`, `Generation`, `Snapshot<R>`, `GenerationEvent`, `Cause`, `RescanScope`, `RescanTicket`. This is the headline; everything depends on it.
3. **VFS scaffold.** `lspkit-vfs` with the document store API and `PositionEncoding` skeleton.
4. **Server scaffold.** `lspkit-server` with our hand-rolled JSON-RPC loop signature, capability builder shape, URI helpers, diagnostics-bus type, workspace-folders type.
5. **MCP scaffold.** `lspkit-mcp` with the `rmcp` adapter trait surface and the newtype wall hiding `rmcp::*`.
6. **Live, config, treesitter, sidecar scaffolds.** Same treatment: public surface + placeholder bodies.
7. **Tracing helper.** Feature-gated module in `lspkit` (≈50 LOC).
8. **Hygiene pass.** Apply `#[non_exhaustive]`, workspace lints, deny `missing_docs`, confirm acceptance grep returns zero hits.
9. **Migration notes in source repos.** Add a "Migration to `lspkit`" section to each of the four source repos' AI-guidance file. No code edits in those repos.

## Critical files to create

```
lspkit/
├── Cargo.toml
├── rustfmt.toml
├── .gitignore
├── README.md
├── Makefile
├── spec.md
├── plan.md
└── crates/
    ├── lspkit/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, engine.rs, tracing.rs, prelude.rs}
    ├── lspkit-server/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, dispatch.rs, jsonrpc.rs, capabilities.rs, uri.rs,
    │             diagnostics.rs, progress.rs, workspace.rs}
    ├── lspkit-vfs/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, document.rs, encoding.rs, edit.rs}
    ├── lspkit-live/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, watcher.rs, scheduler.rs, session.rs}
    ├── lspkit-mcp/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, adapter.rs, tools.rs}
    ├── lspkit-config/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, layered.rs}
    ├── lspkit-treesitter/
    │   ├── Cargo.toml
    │   └── src/{lib.rs, parser.rs, cache.rs}
    └── lspkit-sidecar/
        ├── Cargo.toml
        └── src/{lib.rs, transport.rs, lifecycle.rs, correlator.rs}
```

## Source-repo migration notes (no code edits)

Add the same section to each repo's AI-guidance file, parameterized with concrete paths from Phase-1 exploration:

> ## Migration to `lspkit`
>
> The cross-cutting LSP/MCP scaffolding in this repo (document store, watcher+scheduler, diagnostics pipeline, capability builder, MCP↔engine bridge, sidecar IPC) is being distilled into the generic `lspkit-*` workspace at `/Users/christianfindlay/Documents/Code/lspkit`.
>
> **For new LSP infrastructure work:** prefer `lspkit-*` crates over reinventing it here.
> **For changes to existing scaffolding in this repo:** flag in the PR description if the patch duplicates `lspkit` functionality, and link the upstream issue/crate.
>
> Mapping (current → toolkit crate): *(per-repo paths filled in)*
>
> Code in this repo is **not** being removed — it stays canonical until the toolkit matures. This note exists so future agents reuse `lspkit` for new servers and avoid widening this repo's scaffolding.

Target files:

- `/Users/christianfindlay/Documents/Code/DataProvider/Lql/lql-lsp-rust/` — add to `README.md` (no Claude.md/AGENTS.md present per phase-1).
- `/Users/christianfindlay/Documents/Code/Basilisk/Claude.md` and `Agents.md`.
- `/Users/christianfindlay/Documents/Code/Deslop/Claude.md`.
- `/Users/christianfindlay/Documents/Code/SharpLsp/Claude.md` and `AGENTS.md`.

## Risks

1. **`rmcp` is pre-1.0.** Pin a minor version; document the version contract in `lspkit-mcp`'s README; revisit when 1.0 lands.
2. **`DashMap` 6 deadlock pattern** across `await`. `lspkit-vfs` MUST scope all guards inside sync blocks; lint-enforce via a `cargo check` test that no method returns a guard type.
3. **`async_trait` macro overhead** in public traits. Acceptable for v0; revisit when native AFIT `Send`-bound futures stabilize across crate boundaries.
4. **JSON-RPC framing correctness.** Rolling our own server loop means we own the bug surface. Mitigation: a small fuzz/conformance test corpus against a real LSP client, scheduled for v0.1.

## TODO

- [ ] Create workspace root files: `Cargo.toml`, `rustfmt.toml`, `.gitignore`, `README.md`, `Makefile` (generic).
- [ ] Scaffold `crates/lspkit/` with `engine.rs` (full `EngineApi` trait + supporting types), `tracing.rs` (feature-gated), `prelude.rs`, `lib.rs`.
- [ ] Scaffold `crates/lspkit-vfs/` with `document.rs`, `encoding.rs`, `edit.rs` — public surface only, placeholder bodies.
- [ ] Scaffold `crates/lspkit-server/` with hand-rolled JSON-RPC loop signature in `jsonrpc.rs`, plus `dispatch.rs`, `capabilities.rs`, `uri.rs`, `diagnostics.rs`, `progress.rs`, `workspace.rs`.
- [ ] Scaffold `crates/lspkit-live/` with `watcher.rs`, `scheduler.rs`, `session.rs`.
- [ ] Scaffold `crates/lspkit-mcp/` with `adapter.rs`, `tools.rs` — `rmcp` types wrapped behind newtypes.
- [ ] Scaffold `crates/lspkit-config/` with `layered.rs`.
- [ ] Scaffold `crates/lspkit-treesitter/` with `parser.rs`, `cache.rs`.
- [ ] Scaffold `crates/lspkit-sidecar/` with `transport.rs`, `lifecycle.rs`, `correlator.rs` — zero references to any specific backend.
- [ ] Add workspace lints (`missing_docs = "deny"`, `unsafe_code = "deny"`) and apply `#[non_exhaustive]` everywhere required by the spec.
- [ ] Run acceptance checks: `cargo build --workspace`, `cargo test --workspace`, `cargo doc --workspace --no-deps`, `cargo clippy --workspace -- -D warnings`, generic-only grep returns zero hits, `tower_lsp` / `rmcp::` not in any `pub` signature.
- [ ] Write the "Migration to `lspkit`" section into `lql-lsp-rust/README.md`.
- [ ] Write the "Migration to `lspkit`" section into `Basilisk/Claude.md` and `Basilisk/Agents.md`.
- [ ] Write the "Migration to `lspkit`" section into `Deslop/Claude.md`.
- [ ] Write the "Migration to `lspkit`" section into `SharpLsp/Claude.md` and `SharpLsp/AGENTS.md`.
- [ ] Final review: every TODO above checked; spec.md and plan.md exist as separate files.
