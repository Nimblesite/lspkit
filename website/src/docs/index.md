---
layout: docs.njk
title: A small foundation.
description: One engine behind your language server and MCP server, with a profiler built into VS Code.
eleventyNavigation:
  key: Overview
  order: 1
---
<!-- [WEB-CONTENT] Starter documentation: overview, setup, crates, profiler. -->
lspkit is a set of Rust crates for building language servers. Your analysis runs once, in one engine, and both an LSP server and an MCP server read from it.

{% video "sharplsp-profiler" %}

## One engine

`EngineApi` is the boundary between your analysis and the protocols. Every report carries a generation number, so a client always knows which state of the workspace produced an answer.

## Two surfaces

The LSP server and the MCP adapter consume the same engine. Protocol handling stays at the edges; analysis lives in one place.

## Shared plumbing

File tracking, configuration, sidecar processes, tree-sitter parsing, transport and profiling are ready-made. Use the parts that fit your server.

{% shots "sharplsp-heatmap", "osprey-heatmap" %}

## Next steps

- [Getting started](/docs/getting-started/): add the crates and implement `EngineApi`.
- [Crates](/docs/crates/): what each crate and feature provides.
- [Profiler](/docs/profiler/): CPU and memory profiling for any language server.
- [Examples](/docs/examples/): SharpLsp and Osprey running the profiler in VS Code.
- [The architectural field note](/blog/one-engine/): why one engine serves two surfaces.
