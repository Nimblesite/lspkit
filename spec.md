# Plan: `lspkit` — a generic Rust framework for building LSP+MCP servers

## Context

You maintain four real LSP servers — each one re-implements the same scaffolding from scratch (tower-lsp setup, document store, file watcher + debouncer, diagnostics pipeline, config loader, tracing setup, capability advertisement, and in two cases an MCP surface that exposes the LSP's live state to AI agents). Every new server repeats the same 1–3k LOC of boilerplate before it can ship its first feature.

This repo, `/Users/christianfindlay/Documents/Code/lspkit`, will distill those cross-cutting patterns into a public, fully generic Rust workspace published as `lspkit-*` crates. The headline contribution is the **"one engine, two surfaces"** pattern: an LSP server holds live analysis state, an MCP server vends that same state to AI agents — both reading from a single `EngineApi` trait.

Constraints (decided with you upstream):

- **Fully generic.** No mentions of any specific consumer (none of the four source repos) in toolkit code, docs, READMEs, or examples. Toolkit crates must stand on their own when published.
- **LSP backbone:** `tower-lsp` 0.20 (matches three of your four servers). See *Risks* below — this crate is unmaintained as of 2026 and the toolkit may pivot to `async-lsp` at v0.2.
- **MCP backbone:** `rmcp` (official Rust SDK).
- **Wire models:** the toolkit ships **no** typeDiagram-generated types. Wire schemas are a consumer concern (case-by-case).
- **Source repos:** code is **not modified**. Only their AI-guidance docs (Claude.md / AGENTS.md / README) get a migration pointer.
- **Scope:** lean v0 — crate skeletons, traits, and the few utilities every consumer rebuilds. No demo/example crate, no published version yet.

## Goals

1. Eight crates with clean dependency directions and a single façade entry point.
2. A stable `EngineApi` trait that both `lspkit-server` (LSP) and `lspkit-mcp` (MCP) consume — proving the "one engine, two surfaces" pattern.
3. Generic helpers for the things every LSP gets wrong: position encoding, URI ↔ path, cancellation, progress, diagnostics fan-out, debounced re-analysis, workspace folders.
4. Public API hygiene that lets us evolve internals without breaking downstreams (`#[non_exhaustive]`, wrapper types over third-party LSP/MCP types, sealed extension traits where appropriate).
5. Migration notes added to each of the four source repos pointing future Claude sessions at this toolkit.

## Non-goals (v0)

- No demo LSP. No published crates.io release. No tree-sitter grammars bundled.
- No Roslyn/FCS/Python-specific or any other language-specific glue. `lspkit-sidecar` is **pure transport**.
- No backwards-compat shim with the existing implementations in source repos. The toolkit re-implements distilled patterns from scratch — those repos keep working unchanged.

## Crate layout — 8 crates

```
lspkit/
├── Cargo.toml                      # workspace
├── rustfmt.toml
├── README.md                       # generic; no consumer names
├── .gitignore
└── crates/
    ├── lspkit/                     # façade + EngineApi trait + tracing setup (feature-gated)
    ├── lspkit-server/              # tower-lsp builder, capability negotiation, dispatch
    ├── lspkit-vfs/                 # document store, rope edits, position encoding
    ├── lspkit-live/                # watcher + debouncer + single-flight scheduler + session
    ├── lspkit-mcp/                 # rmcp adapter mounting EngineApi as tools/resources
    ├── lspkit-config/              # layered TOML loader generic over consumer config type
    ├── lspkit-treesitter/          # LanguageParser trait + content-hash parse cache
    └── lspkit-sidecar/             # framed IPC + lifecycle/health/respawn (pure transport)
```

Dependency direction (acyclic):

```
                   lspkit (façade, hosts EngineApi)
                  /   |   \      \         \
   lspkit-server  lspkit-mcp  lspkit-live  lspkit-config
        |               |          |
   lspkit-vfs    (rmcp adapter) (notify + scheduler)
        |
   lspkit-treesitter (optional, separate dep cone)

   lspkit-sidecar    (standalone; depends only on tokio + serde)
```

### Per-crate responsibilities

| Crate | Responsibility | Why it's a crate |
|---|---|---|
| **lspkit** | Façade. Hosts `EngineApi` trait + `Generation` / `Snapshot` / `GenerationEvent` types. Feature `tracing-setup` provides a `tracing-subscriber` builder tuned for stdio servers (stderr-only by default, optional rolling JSON file). Prelude module. | Discoverability — `use lspkit::EngineApi` is the headline import. Tracing helper is too small for its own crate (≈50 LOC). |
| **lspkit-server** | tower-lsp `Server::new` wrapper, capability builder, dispatcher that routes LSP requests to an `EngineApi` + VFS pair. Cancellation token plumbed through every request. URI ↔ `PathBuf` helper. Workspace-folders helper. Wraps `tower_lsp::Client` behind a `LspClient` newtype so consumers never see tower-lsp's types in their public APIs. | This is the bulk of every LSP server's boilerplate. |
| **lspkit-vfs** | Concurrent open-document store (`DashMap`-backed). LSP version tracking. `didOpen/didChange/didClose` helpers. Incremental edits via `ropey`. **Position encoding negotiation** (UTF-8 ↔ UTF-16 ↔ UTF-32) — every consumer gets this wrong. `ropey::Rope` is hidden from public signatures. | Independently useful for headless analyzers and CLI re-parsers, not just LSP servers. |
| **lspkit-live** | `notify` watcher → debouncer → single-flight scheduler → session lifecycle. Produces values that implement `EngineApi`. Configurable debounce + quiet windows. Generation counter. Subscriber stream of `GenerationEvent`. | Marked `0.0.x` — most experimental crate. The watcher/scheduler interplay is where v1 will refine. |
| **lspkit-mcp** | `rmcp` adapter. Mounts an `EngineApi` impl as MCP tools, resources, and prompts. Helpers for registering tools whose descriptions encourage **prevention-first** agent behavior (i.e., call the tool *before* generating code) — pattern hooks, not policy. Wraps `rmcp` server types behind our own. | Closes the "two surfaces" loop. Pure adapter; no domain logic. |
| **lspkit-config** | Layered TOML loader: walk up from workspace root, merge LSP `initializationOptions`, optional hot-reload signal. Generic over the consumer's `Deserialize` config struct. | Every consumer rebuilds this; common bugs (precedence order, missing-file handling, hot-reload races) are worth solving once. |
| **lspkit-treesitter** | `LanguageParser` trait, content-hash-keyed parse cache, incremental parse helper. **Does not bundle grammars.** | Optional dependency cone — consumers who don't use tree-sitter don't pay for it. |
| **lspkit-sidecar** | Length-prefixed framed transport over Unix socket / Windows named pipe. Pluggable wire format (MessagePack, JSON, CBOR — consumer's choice). Lifecycle manager: spawn, health-ping with timeout, exponential-backoff respawn, request correlation via `AtomicU64` ID. **Pure transport — no Roslyn, FCS, or any specific backend.** | Lets a Rust LSP delegate semantic work to a non-Rust process (C#, F#, Python, Go) without re-inventing the wire. |

## The headline: `EngineApi` trait

```rust
#[async_trait::async_trait]
pub trait EngineApi: Send + Sync + 'static {
    type Report: Send + Sync + 'static;
    type Query: Send + Sync;
    type Error: std::error::Error + Send + Sync + 'static;

    fn generation(&self) -> Generation;

    async fn report(
        &self,
        query: Self::Query,
        cancel: CancellationToken,
    ) -> Result<Snapshot<Self::Report>, Self::Error>;

    async fn rescan(
        &self,
        scope: RescanScope,
        progress: Progress,
    ) -> Result<RescanTicket, Self::Error>;

    fn subscribe(&self) -> BoxStream<'static, GenerationEvent>;

    async fn shutdown(&self) -> Result<(), Self::Error>;
}

#[non_exhaustive]
pub struct Snapshot<R> { pub generation: Generation, pub data: R }

#[non_exhaustive]
pub struct GenerationEvent { pub generation: Generation, pub cause: Cause }

#[non_exhaustive]
pub enum RescanScope { All, Paths(Vec<PathBuf>) }
```

Design notes baked in: associated types (one report type per impl, clean call sites); `Snapshot<R>` forces callers to confront staleness; `BoxStream` keeps channel choice an impl detail; `CancellationToken` and `Progress` make every long-running call cooperative; `shutdown` drains before either surface returns.

## Cross-cutting helpers (in `lspkit-server` unless noted)

These are the things the Plan-mode review flagged as universally broken across LSPs. v0 ships them:

- **Position encoding** (`lspkit-vfs::PositionEncoding`) — negotiates UTF-16/UTF-8/UTF-32 at `initialize`, converts on every read.
- **URI ↔ `PathBuf`** (`lspkit-server::uri`) — cross-platform-correct, handles `file://`, percent-decoding, Windows drive letters.
- **Cancellation** — `tokio_util::sync::CancellationToken` plumbed through dispatcher.
- **Progress** — `Progress` sink wrapping LSP `window/workDoneProgress`.
- **Diagnostics bus** (`lspkit-server::diagnostics`) — fan-out from one source to LSP `publishDiagnostics` and MCP resource-updated notifications.
- **Workspace folders** — explicit `Workspace` type, multi-root aware.

## Tech choices (locked for v0 unless flagged)

- `tokio` 1.x, `tracing` + `tracing-subscriber`, `serde` + `serde_derive`, `async-trait`.
- `tower-lsp` 0.20 (see *Risks*).
- `rmcp` (pin a minor version; verify its tokio requirement aligns with tower-lsp's).
- `notify` 7.x, `ropey` 1.x, `dashmap` 6.x.
- `rmp-serde` available as a default wire format for `lspkit-sidecar`, but the transport is generic over `serde`.

## Migration notes added to source repos (no code edits)

Add a short "Migration to lspkit" section to the **AI-guidance file** of each repo (Claude.md / AGENTS.md, whichever exists; create one only if neither does — but in practice every repo has one). Each note follows the same template, parameterized per repo:

> ## Migration to `lspkit`
>
> The cross-cutting LSP/MCP scaffolding in this repo (document store, watcher+scheduler, diagnostics pipeline, capability builder, MCP↔engine bridge, sidecar IPC) is being distilled into the generic `lspkit-*` workspace at `/Users/christianfindlay/Documents/Code/lspkit`.
>
> **For new LSP infrastructure work:** prefer `lspkit-*` crates over reinventing it here.
> **For changes to existing scaffolding in this repo:** flag in the PR description if the patch duplicates `lspkit` functionality, and link the upstream issue/crate.
>
> Mapping (current → toolkit crate):
> - `<repo's document store path>` → `lspkit-vfs`
> - `<repo's tower-lsp setup path>` → `lspkit-server`
> - `<repo's watcher/scheduler path, if any>` → `lspkit-live`
> - `<repo's config loader path>` → `lspkit-config`
> - `<repo's MCP path, if any>` → `lspkit-mcp`
> - `<repo's sidecar IPC path, if any>` → `lspkit-sidecar`
>
> Code in this repo is **not** being removed — it stays canonical until the toolkit matures. This note exists so future agents reuse `lspkit` for new servers and avoid widening this repo's scaffolding.

Files to add the section to:

- `/Users/christianfindlay/Documents/Code/DataProvider/Lql/lql-lsp-rust/<Claude.md or README.md, whichever is canonical>`
- `/Users/christianfindlay/Documents/Code/Basilisk/Claude.md` and `Agents.md`
- `/Users/christianfindlay/Documents/Code/Deslop/Claude.md`
- `/Users/christianfindlay/Documents/Code/SharpLsp/Claude.md` and `AGENTS.md`

The per-repo mapping table is filled with concrete paths from the Phase-1 exploration findings (already gathered).

## Risks & open questions

1. **`tower-lsp` 0.20 is unmaintained.** Last release 2023; pins an older `lsp-types`. The community fork `tower-lsp-server` and the alternative `async-lsp` are both active. **Decision:** ship v0 on `tower-lsp` 0.20 to match your existing repos; re-evaluate at v0.2 with a concrete `async-lsp` PoC. Capture this as a tracked issue in the toolkit repo.
2. **`rmcp` is pre-1.0.** Pin a minor version. Document the rmcp version contract clearly in `lspkit-mcp`'s README.
3. **`DashMap` 6 deadlock pattern** when holding a reference across `await`. `lspkit-vfs` must scope locks tightly inside methods and never return guards.
4. **`async_trait` vs native AFIT.** Use `async_trait` for the public `EngineApi` — native `async fn in trait` still has rough edges for `Send`-bound futures across crate boundaries. Revisit at v0.2.
5. **Public API surface.** Wrap every `tower_lsp::*` and `rmcp::*` type before it appears in a `pub` signature. This is non-negotiable for backwards compatibility once we publish.

## Critical files to create

- `/Users/christianfindlay/Documents/Code/lspkit/Cargo.toml` — workspace manifest (members + workspace.dependencies pinning shared deps).
- `/Users/christianfindlay/Documents/Code/lspkit/rustfmt.toml`, `.gitignore`, `README.md` (generic).
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit/{Cargo.toml, src/lib.rs, src/engine.rs, src/tracing.rs}` — façade + `EngineApi` trait + feature-gated tracing helper.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-server/{Cargo.toml, src/lib.rs, src/dispatch.rs, src/capabilities.rs, src/uri.rs, src/diagnostics.rs, src/workspace.rs}`.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-vfs/{Cargo.toml, src/lib.rs, src/document.rs, src/encoding.rs, src/edit.rs}`.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-live/{Cargo.toml, src/lib.rs, src/watcher.rs, src/scheduler.rs, src/session.rs}`.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-mcp/{Cargo.toml, src/lib.rs, src/adapter.rs, src/tools.rs}`.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-config/{Cargo.toml, src/lib.rs, src/layered.rs}`.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-treesitter/{Cargo.toml, src/lib.rs, src/parser.rs, src/cache.rs}`.
- `/Users/christianfindlay/Documents/Code/lspkit/crates/lspkit-sidecar/{Cargo.toml, src/lib.rs, src/transport.rs, src/lifecycle.rs, src/correlator.rs}`.

Per-crate files contain trait/type skeletons with `///` doc comments and `todo!()`-free placeholders. v0 is *scaffold*: the crates compile, the public surface is locked in, but feature implementations land in follow-up PRs.

## Verification

- `cargo build --workspace` succeeds from a clean checkout.
- `cargo test --workspace` runs (no tests yet at scaffold stage — confirm test harness wiring works for each crate).
- `cargo doc --workspace --no-deps` produces docs with no `missing_docs` warnings (lint enforced workspace-wide).
- `cargo clippy --workspace -- -D warnings` clean.
- `rg -i 'basilisk|deslop|sharplsp|lql' crates/ README.md` returns **zero hits** — generic-toolkit guarantee.
- Each of the four source repos has a "Migration to `lspkit`" section in its AI-guidance file, with the per-repo path mapping filled in from the Phase-1 findings.
- Manual sanity: load `crates/lspkit/src/engine.rs` in your editor — the `EngineApi` trait reads as obviously usable from both an LSP server and an MCP server with no domain-specific concessions.
