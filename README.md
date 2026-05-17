# lsp_toolkit

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

v0 — scaffold. Public surfaces are locked; feature implementations land in follow-up PRs. Not yet published.

## Build

```
make ci      # lint + test + doc
make test    # cargo test --workspace
make lint    # cargo clippy --workspace -- -D warnings
```

## Spec

Behavior contract: [spec.md](spec.md). Execution plan: [plan.md](plan.md).

## License

MIT OR Apache-2.0.
