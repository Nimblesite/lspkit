---
layout: docs.njk
title: Crates.
description: What each published crate and package provides.
eleventyNavigation:
  key: Crates
  order: 3
---
<!-- [WEB-CONTENT] -->
| Crate | What it gives you |
|---|---|
| [`lspkit`](https://crates.io/crates/lspkit) | `EngineApi` and its types, plus the `config`, `vfs` and `sidecar` modules. |
| [`lspkit-server`](https://crates.io/crates/lspkit-server) | JSON-RPC LSP server scaffolding on `lsp-types`: capabilities, dispatch, URIs, diagnostics, progress, cancellation and live file watching. |
| [`lsp-profiler`](https://crates.io/crates/lsp-profiler) | Language-neutral CPU and memory profiler for language servers. |
| [`lsp-profiler-dotnet`](https://crates.io/crates/lsp-profiler-dotnet) | The profiler's .NET runtime, using EventPipe. |

## Features

| Crate | Feature | Adds |
|---|---|---|
| `lspkit` | `treesitter` | Tree-sitter parsing helpers. |
| `lspkit` | `tracing-setup` | A ready-made `tracing` subscriber. |
| `lspkit-server` | `mcp` | The MCP adapter over the same engine. |
| `lspkit-server` | `pull` | Pull-based incremental analysis with salsa. |

## Editor package

[`@nimblesite/lsp-profiler-ui`](https://www.npmjs.com/package/@nimblesite/lsp-profiler-ui) is the VS Code UI for the profiler: processes, CPU heat map, flame graph, memory dashboard and retention graph. It also carries Debug Adapter Protocol glue for extension tests.

```sh
npm install @nimblesite/lsp-profiler-ui
```
