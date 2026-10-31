# lspkit

A generic Rust workspace of cross-cutting building blocks for LSP and MCP servers, published as four crates: `lspkit`, `lspkit-server`, `lsp-profiler` and `lsp-profiler-dotnet`.

## What it is

The headline contribution is the **"one engine, two surfaces"** contract: a single `EngineApi` trait that an LSP backend and an MCP server both consume, so live analysis state is computed once and vended two ways.

| Crate | Responsibility |
|---|---|
| `lspkit` | Façade. `EngineApi` trait + supporting types, plus the `config`, `vfs` and `sidecar` modules and the `treesitter` module behind the `treesitter` feature. Optional `tracing-setup` feature. |
| `lspkit-server` | Hand-rolled JSON-RPC LSP server scaffolding built on `lsp-types`, plus the `live` module and the `mcp` adapter behind the `mcp` feature. Capability builder, URI helpers, diagnostics fan-out, progress, cancellation. |
| `lsp-profiler` | Language-neutral CPU and memory profiler for language servers: sessions, hotspots, flame graphs, speedscope/V8 exports, heap snapshots, leak scoring, one JSON-RPC protocol. [Spec](docs/specs/LSP-PROFILER-SPEC.md). |
| `lsp-profiler-dotnet` | The profiler's .NET runtime: EventPipe CPU sampling and heap snapshots through the engine in `dotnet/`. |

`lsp-profiler-python` is parked and unpublished; it hosts the `lsp-profiler-helper` binary and depends on `lsp-profiler`.

## Editor packages

Reusable editor-side building blocks that are not Rust crates live under `packages/`:

| Package | Responsibility |
|---|---|
| [`@nimblesite/lsp-profiler-ui`](packages/lsp-profiler-ui) | Editor-neutral VS Code UI for the profiler protocol: processes view, CPU heat map, flame graph, memory dashboard, retention graph, autopilot. Its [`debug`](packages/lsp-profiler-ui/src/debug) module is the language-neutral **Debug Adapter Protocol** glue and integration-test harness (adapter resolution, zero-config launch defaults, save-or-reject, profiling-launch hooks, a `customRequest`-driven DAP test client and assertions), dependency-injected with no static `import "vscode"`. |

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
