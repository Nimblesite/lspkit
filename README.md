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
| `lsp-profiler` | Language-neutral CPU and memory profiler for language servers: sessions, hotspots, flame graphs, speedscope/V8 exports, heap snapshots, leak scoring, one JSON-RPC protocol. [Spec](docs/specs/LSP-PROFILER-SPEC.md). |
| `lsp-profiler-dotnet` | The profiler's .NET runtime: EventPipe CPU sampling and heap snapshots through the engine in `dotnet/`. |

## Editor packages

Reusable editor-side building blocks that are not Rust crates live under `packages/`:

| Package | Responsibility |
|---|---|
| [`@nimblesite/lspkit-debug`](packages/lspkit-debug) | Language-neutral VS Code **Debug Adapter Protocol** glue + integration-test harness (adapter resolution, zero-config launch defaults, save-or-reject, profiling-launch hooks, and a `customRequest`-driven DAP test client/assertions). Written once for every Nimblesite language extension (Osprey · `lldb-dap`, SharpLsp · `netcoredbg`, Basilisk · `debugpy`) instead of per-language. Fully dependency-injected (no static `import "vscode"`), 100% unit-test coverage. |
| [`@nimblesite/lsp-profiler-ui`](packages/lsp-profiler-ui) | Editor-neutral VS Code UI for the profiler protocol: processes view, CPU heat map, flame graph, memory dashboard, retention graph, autopilot. |

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
