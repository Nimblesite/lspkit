# Writing about lspkit

**The foundation for any language's tooling.** This is the brief for the README, website, journal, release notes and videos. Pair it with the [design system](specs/website-design-system.md) before writing or rendering.

lspkit is a set of Rust crates for building language servers. Lead with what a language server author gets: analysis written once, served to the editor over LSP and to agents over MCP, and profiled with the same CPU and memory profiler that real language servers already ship. Show the profiler running before explaining the crates.

## The three pillars

Say all three together on first mention, in this order.

- **One engine.** `EngineApi` is the single boundary between analysis and protocols. State is computed once; every report carries a generation number, so a client knows which workspace state produced an answer.
- **Two surfaces.** The LSP server and the MCP adapter consume the same engine. Editors and AI agents see the same answers, with no second analysis pipeline to drift.
- **A profiler that ships.** `lsp-profiler` and `@nimblesite/lsp-profiler-ui` give any language server hot lines in the source, flame graphs, a live heap timeline and ranked leak suspects inside VS Code. A language plugs in a runtime crate; everything else is shared.

Short form: **The foundation for any language's tooling.** Longer form: **lspkit is the language server toolkit: write your language analysis once in Rust, serve it over LSP and MCP, and profile it with a CPU and memory profiler built into VS Code.**

## Proof first

Every claim points at something running. The proof today is two shipping language servers:

- **[SharpLsp](https://sharplsp.dev)**, C# and F#: [profiler guide and video](https://sharplsp.dev/docs/profiler/). Attach to any running .NET process; hot lines light up in the code; leak suspects are ranked live.
- **[Osprey](https://www.ospreylang.dev)**, a native functional language: [profiler tutorial video](https://www.ospreylang.dev/docs/tutorials/profiler/). Run and profile the open file, read the flame graph, attach to a running program, check for leaks.

Use their videos and screenshots, credited and linked to their own sites. On the website, embed the videos on [Examples](../website/src/docs/examples.md); in the README, link the poster images to each project's profiler page.

## Message order

1. **The profiler, running.** A video or screenshot of hot lines in real source code is the hook.
2. **One engine.** One Rust implementation, two surfaces. Name `EngineApi` only in the docs.
3. **The crates.** What each crate gives you and how to add it (`cargo add lspkit lspkit-server`).
4. **Bring your language.** A runtime crate is all a new language needs to get the profiler.

## Audience

Language server authors and tool builders: people writing an LSP server in Rust, adding an MCP surface for agents, or wanting profiling in their editor extension. Assume they know LSP; do not assume they know MCP or the profiler.

## Keywords [WEB-SEO-KEYWORDS]

Every blog post MUST target search keywords taken from Google Trends (`pytrends`, 12-month window), recorded in an HTML comment at the top of the post with the date range. Use them in the title, description, first paragraph and headings, where they read naturally.

Google Trends, 12 months to October 2026, relative interest:

| Keyword | Interest | Use |
|---|---|---|
| mcp server | 46 | Lead keyword. Related: "what is mcp server", "github mcp server", "claude mcp server". |
| claude code, cursor, codex | rising | Name the agents that consume an MCP server. |
| vscode extension | 7 | The editor surface. |
| memory leak | 6.9 | The profiler's hook. |
| language server protocol | 0.7 | Top "language server" query; also "rust language server". |

"flame graph", "cpu profiler", "memory profiler" and "vscode profiler" are near zero: use them in body copy, never as the lead.

## Wording

- Use **lspkit** in lowercase, always. The crates are `lspkit`, `lspkit-server`, `lsp-profiler`, `lsp-profiler-dotnet`; the npm package is `@nimblesite/lsp-profiler-ui`.
- Concrete verbs and visible results: "hot lines light up in your code", not "powerful insights".
- Name SharpLsp and Osprey as examples of what lspkit enables, never as part of lspkit. The crates themselves stay generic and never mention a consumer.
- No invented adoption numbers, benchmarks or stars. Attach any performance figure to a published measurement.
- Banned: "blazing fast", "revolutionary", "seamless", "next-generation", "zero-cost".

## Qualifications to preserve

- lspkit is at 0.x. Public APIs may change between minor versions; say so where stability matters.
- The profiler runtimes that ship today are .NET (`lsp-profiler-dotnet`) and Osprey's native runtime. `lsp-profiler-python` is parked and unpublished; do not advertise Python profiling.
- The profiler UI targets VS Code. Other editors speak the same JSON-RPC protocol but have no ready-made UI.
