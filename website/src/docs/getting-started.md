---
layout: docs.njk
title: Getting started.
description: Add lspkit to a language server and implement the engine.
eleventyNavigation:
  key: Getting started
  order: 2
---
<!-- [WEB-CONTENT] -->
This is what you are building toward: your language server, profiled inside VS Code.

{% video "osprey-profiler" %}

## Add the crates

```sh
cargo add lspkit lspkit-server
```

Turn on the features you need:

```sh
cargo add lspkit --features treesitter
cargo add lspkit-server --features mcp
```

## Implement the engine

Your analysis implements `EngineApi`. It defines its own report, query and error types, and four operations:

```rust
#[async_trait]
pub trait EngineApi: Send + Sync + 'static {
    type Report: Send + Sync + 'static;
    type Query: Send + Sync;
    type Error: StdError + Send + Sync + 'static;

    fn generation(&self) -> Generation;
    async fn report(&self, query: Self::Query, cancel: CancellationToken)
        -> Result<Snapshot<Self::Report>, Self::Error>;
    async fn rescan(&self, scope: RescanScope, progress: Progress)
        -> Result<RescanTicket, Self::Error>;
    fn subscribe(&self) -> GenerationEventStream;
    async fn shutdown(&self) -> Result<(), Self::Error>;
}
```

- `generation` increases on every successful rescan.
- `report` answers a query from a snapshot and stops promptly when cancelled.
- `rescan` refreshes the analysis and may coalesce concurrent requests.
- `subscribe` streams generation changes to listeners.
- `shutdown` drains in-flight work; every later call returns an error.

## Serve it

Hand one engine instance to the `lspkit-server` scaffolding for the editor and, with the `mcp` feature, to the MCP adapter for agents. Both read the same snapshots.

{% shots "osprey-heatmap", "sharplsp-heatmap" %}

## Learn more

The full behavioural contract is in the [spec](https://github.com/Nimblesite/lspkit/blob/main/docs/specs/lspkit-spec.md), and the API reference is on [docs.rs](https://docs.rs/lspkit).
