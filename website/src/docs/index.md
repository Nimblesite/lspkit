---
layout: docs.njk
title: A small foundation.
description: The architectural basics of the LSP Toolkit.
eleventyNavigation:
  key: Overview
  order: 1
---
<!-- [WEB-CONTENT] Deliberately brief starter documentation. -->
Reusable Rust components for building language servers, with shared infrastructure for LSP and MCP.

[Explore the prose example →](/docs/prose/)

## One engine

`EngineApi` defines the shared analysis boundary. Reports carry a generation, so consumers can identify the state behind an answer.

## Two surfaces

The language server and MCP adapter consume the same engine. Keep protocol handling at the edges and analysis in one place.

## Shared plumbing

Use the toolkit's file tracking, configuration, transport and profiling components where they fit your server.

[Read the architectural field note →](/blog/one-engine/)

[Explore the source and setup instructions ↗](https://github.com/Nimblesite/lspkit)
