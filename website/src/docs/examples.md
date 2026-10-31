---
layout: docs.njk
title: Built with lspkit.
description: SharpLsp and Osprey ship the lspkit profiler in VS Code today.
eleventyNavigation:
  key: Examples
  order: 5
---
<!-- [WEB-EXAMPLES] Real integrations; media is served by each project's own site. -->
Two language servers run on lspkit today. Both mount the same `lsp-profiler` engine and the same `@nimblesite/lsp-profiler-ui` panels; only the runtime crate differs.

## SharpLsp: C# and F#

[SharpLsp](https://sharplsp.dev) is a C# and F# language server for VS Code. Its profiler attaches to any running .NET process through `lsp-profiler-dotnet`, paints hot lines in your code, and ranks leak suspects while the app runs.

{% video "sharplsp-profiler" %}

{% shots "sharplsp-heatmap", "sharplsp-results", "sharplsp-memory", "sharplsp-leaks" %}

{% video "sharplsp-fsharp" %}

[Read the SharpLsp profiler guide ↗](https://sharplsp.dev/docs/profiler/)

## Osprey: a native functional language

[Osprey](https://www.ospreylang.dev) is a functional language with algebraic effects that compiles to native code. Its VS Code extension uses the same profiler to run and profile the open file, attach to a running program, and check for leaks.

{% video "osprey-profiler" %}

{% shots "osprey-heatmap", "osprey-flamegraph" %}

[Watch the Osprey profiler tutorial ↗](https://www.ospreylang.dev/docs/tutorials/profiler/)

## Add your language

A new language needs only a runtime crate that samples its processes. See [Profiler](/docs/profiler/) for how the pieces fit.
