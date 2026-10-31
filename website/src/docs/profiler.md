---
layout: docs.njk
title: Profiler.
description: One CPU and memory profiler for every language server.
eleventyNavigation:
  key: Profiler
  order: 4
---
<!-- [WEB-CONTENT] -->
`lsp-profiler` profiles a running language server without leaving the editor. A language plugs in a runtime crate; sessions, reports and the protocol are shared.

{% video "sharplsp-cpu" %}

## What it measures

- **CPU:** sampling sessions, hotspots and flame graphs, exported as speedscope or V8 profiles.
- **Memory:** heap snapshots, retention paths and leak scoring.

{% shots "sharplsp-heatmap", "sharplsp-results", "sharplsp-memory", "sharplsp-leaks" %}

{% shots "sharplsp-flamegraph" %}

{% video "sharplsp-memory" %}

## How it fits together

1. Your server mounts `lsp-profiler` and a runtime crate, such as `lsp-profiler-dotnet` for .NET.
2. The editor talks to it over one JSON-RPC protocol.
3. `@nimblesite/lsp-profiler-ui` renders the results in VS Code.

{% shots "osprey-heatmap", "osprey-flamegraph" %}

## Add it

```sh
cargo add lsp-profiler
cargo add lsp-profiler-dotnet   # for .NET language servers
```

See it running in [SharpLsp and Osprey](/docs/examples/). The full design is in the [profiler spec](https://github.com/Nimblesite/lspkit/blob/main/docs/specs/LSP-PROFILER-SPEC.md).
