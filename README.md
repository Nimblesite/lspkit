# lspkit

A generic Rust workspace of cross-cutting building blocks for LSP and MCP servers, published as `lspkit-*` crates.

## What it is

The headline contribution is the **"one engine, two surfaces"** contract: a single `EngineApi` trait that an LSP backend and an MCP server both consume, so live analysis state is computed once and vended two ways.

| Crate | Responsibility |
|---|---|
| `lspkit` | Façade. `EngineApi` trait + supporting types. Optional `tracing-setup` feature. |
| `lspkit-server` | Hand-rolled JSON-RPC LSP server scaffolding built on `lsp-types`. Capability builder, URI helpers, diagnostics fan-out, progress, cancellation. |
| `lspkit-vfs` | Concurrent open-document store with rope-based incremental edits and position-encoding negotiation. |
| `lspkit-live` | File-watcher → debouncer → single-flight scheduler → session lifecycle. |
| `lspkit-mcp` | `rmcp` adapter that mounts an `EngineApi` impl as MCP tools, resources, and prompts. |
| `lspkit-config` | Layered TOML loader generic over the consumer's config struct. |
| `lspkit-treesitter` | `LanguageParser` trait + content-hash-keyed parse cache. Bundles no grammars. |
| `lspkit-sidecar` | Framed-IPC primitives + lifecycle/health/respawn. Pure transport. |

## Status

v0. Public API surfaces are locked; implementations continue to land in follow-up releases.

## Build

```
make ci      # lint + test + build (full CI simulation)
make test    # fail-fast tests + coverage + threshold
make lint    # cargo clippy -- -D warnings
```

## Spec

Behavior contract: [docs/specs/lspkit-spec.md](https://github.com/Nimblesite/lspkit/blob/main/docs/specs/lspkit-spec.md). Execution plans: [docs/plans/](https://github.com/Nimblesite/lspkit/blob/main/docs/plans/).

## License

MIT — see [LICENSE-MIT](https://github.com/Nimblesite/lspkit/blob/main/LICENSE-MIT). © Nimblesite.
