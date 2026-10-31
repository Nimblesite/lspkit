<!-- agent-pmo:74cf183 -->
# LSP Toolkit — Agent Instructions

Reusable Rust components for building language servers, plus the shared CPU and memory profiler every Nimblesite language server mounts. Consumed by SharpLsp, Deslop, Basilisk and lql-lsp-rust. Spec: [docs/specs/lspkit-spec.md](docs/specs/lspkit-spec.md).

**Languages:** Rust, TypeScript 7 (`packages/`), C# and F# (`dotnet/`). **Build:** `make ci` · **Test:** `make test` · **Lint:** `make lint` · **Format:** `make fmt`. Repo-specific targets sit below the marker in the [Makefile](Makefile). Debug one test with `cargo test <name> -- --nocapture`.

## Workflow

- 🛑 **Never kill a VS Code process**, including browser-hosted instances.
- 🛑 **No git commands** unless the user asks; CI handles git. Never add `Co-Authored-By` or agent attribution.
- **Don't ask the user questions.** Use judgment, record assumptions, keep replies to a status line. Mark the one critical point with an emoji.
- **Subagents are illegal.** Delegate to `pi` agents (Kimi → GLM → DeepSeek, latest models). You own their output.
- **Use Too Many Cooks** for file locks and agent messaging when available.
- **Never weaken, bypass or narrow a CI gate**, threshold or timeout. A red check means the code is wrong.
- **Don't close GitHub issues** unless the user asks. Keep GitHub Actions spend minimal.
- **Release-only regime** ([REL-REGIME](docs/specs/lspkit-spec.md#release--publishing)). CI runs on PRs to `main` only; a push to `main` triggers nothing. Shipping happens only when a `v*` tag (or `gh release create vX.Y.Z`) runs `release.yml`, which publishes every crate and npm package via OIDC and deploys the website. A website-only change ships with `gh workflow run pages.yml` from `main`. Source versions stay `0.0.0-dev`.
- **Auto-memory is off.** Durable rules are reviewed changes to this file.
- **Clean up after yourself**: temporary files, build output, disk space.

## Code (every language)

- **Zero duplication.** Run [Deslop](https://deslop.live/docs/for-ai/) `find-similar` before writing code (fused ≥ 0.85 → reuse) and `top-offenders` after. Move code, never copy it.
- **All lints on, all at error.** Never suppress a warning (`#[allow]`, `oxlint-disable`, `#pragma warning disable`); fix the code. A rule is off only with a written reason in its config.
- **No regex or string matching on code or structured data.** Use tree-sitter or a real parser.
- **Functional style.** `Result`/`Option` over exceptions and null, expressions over statements, pure functions, early returns.
- **No placeholders, silent no-ops or legacy code.** Silently broken code becomes an explicit error plus a failing test, reported to the user.
- **Files < 500 lines, functions < 20 lines.** Named constants, not literals.
- **Models are generated with [typeDiagram](https://typediagram.dev/docs/language-reference.html)**, never hand-written or committed.
- **Global state lives in one `state.rs` per crate** (or the language's equivalent).
- **Dependency versions stay in sync** across manifests, `.github/workflows/` and `.devcontainer/`.
- **Structured logging only**: `tracing` (Rust), the package `logger` module (TypeScript), `Microsoft.Extensions.Logging` (.NET). Never log PII, secrets, file contents or user paths.
- **Bug fixes follow [fix-bug](.agents/skills/fix-bug/SKILL.md)**: failing test first. Skills live in `.agents/skills/`; run `node scripts/link-skills.mjs` after adding one.

## Rust

- No `unwrap()`/`expect()` in production; tests may `expect` with a message.
- No `panic!`/`todo!`/`unimplemented!`/`unreachable!`, no `unsafe`. Public items carry `///` docs.
- `thiserror` in libraries, `anyhow` only in binaries. Pattern matching over casts; iterator chains over loops.

## TypeScript

- 🛑 **TypeScript 7 only** (`typescript@^7`). Never pin an older compiler or add a tool that needs one.
- **Lint with oxlint** (`--type-aware`, tsgolint) using [packages/.oxlintrc.json](packages/.oxlintrc.json), the one config for every package. ESLint and typescript-eslint don't support TypeScript 7.
- `strict` tsconfig. No `any`, no `!` non-null assertions, no `as` casts, no `@ts-ignore`. Narrow with type guards.
- Explicit return types and member accessibility. `async`/`await`, never `.then()`. No floating promises.
- VS Code extension tests drive the editor through `vscode.commands.executeCommand`, never provider methods.

## C# and F#

- Latest .NET, nullable enabled, `TreatWarningsAsErrors`, analyzers at the highest level, all in [dotnet/Directory.Build.props](dotnet/Directory.Build.props).
- No `!` null-forgiving, `as` casts or `dynamic`; use pattern matching. Return results instead of throwing.
- Format C# with CSharpier and F# with Fantomas.

## Testing

- **Coarse E2E, black-box only**, against real binaries. No fake LSP or MCP. Unit tests only to isolate a problem.
- **Many user interactions per test, many assertions per interaction.** No `assert!(true)` and no swallowed errors.
- **Never delete or skip a failing test or weaken an assertion.** Add failing tests for broken functionality.
- **100% coverage target.** `make test` is fail-fast and enforces `coverage-thresholds.json`, which only ratchets up.
- **Deterministic.** No `sleep`, timing dependencies or random state.

## Docs and spec IDs

- Specs live in `docs/specs/`, plans with TODO checklists in `docs/plans/`. Read the spec before starting a feature.
- Spec IDs are hierarchical and non-numeric (`[GROUP-TOPIC]`, `[GROUP-TOPIC-DETAIL]`). Code and tests cite them so `grep [GROUP-` finds spec, code and tests.
- Diagrams are Mermaid; models are typeDiagram. Specs are for humans first; no forced line wraps.

## Profiler

One profiler for every lspkit language server: a language plugs in a runtime crate, and everything else is shared. Read [LSP-PROFILER-SPEC](docs/specs/LSP-PROFILER-SPEC.md) and [LSP-PROFILER-PLAN](docs/plans/LSP-PROFILER-PLAN.md) first.

- `[PROF-INVARIANTS]` are hard rules. Every `[PROF-…]` citation must resolve to one spec heading; `make lint` checks this.
- The wire model is `docs/models/profiler.td`, generated by `crates/lsp-profiler/build.rs`.
- `make test` builds the .NET engine first and needs the `dotnet` SDK.
