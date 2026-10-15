<!-- agent-pmo:74cf183 -->
# LSP Toolkit

**Repo:** https://github.com/Nimblesite/lspkit

Reusable Rust components for building LSPs and related tooling, including the shared CPU and memory profiler that every Nimblesite language server mounts. Consumed by:

- [lql-lsp-rust](/Users/christianfindlay/Documents/Code/DataProvider/Lql/lql-lsp-rust)
- [Basilisk](/Users/christianfindlay/Documents/Code/Basilisk)
- [Deslop](/Users/christianfindlay/Documents/Code/Deslop)
- [SharpLsp](/Users/christianfindlay/Documents/Code/SharpLsp)

**Language:** Rust · **Build:** `make ci` · **Test:** `make test` · **Lint:** `make lint`

To debug a single test: `cargo test <name> -- --nocapture` (not a make target).

> ⚠️ Killing a VSCode process — even in the browser — is forbidden.

## Quality Bar

Code must pass review at Google / Meta / Microsoft. No exceptions, not even for one line.

## Token Discipline

Check file size first. `Grep` over `Read`. Use `offset`/`limit`. Smallest diff that solves the problem. Delete dead code, unused imports, stale comments. Call out irrelevant context before proceeding.

## IPC

Processes communicate via IPC. Generate IPC model code with [typeDiagram](https://typediagram.dev/docs/language-reference.html). Generated model code is gitignored, never committed.

## Spec IDs

Hierarchical, non-numeric: `[GROUP-TOPIC]` / `[GROUP-TOPIC-DETAIL]` (e.g. `[PARSE-CSHARP-NORMALIZE]`, `[RANK-SCORE]`). Same-group sections sit adjacent in the spec. Code and tests reference the ID in a comment so `grep [PARSE-` walks spec → code → tests.

## Hard Rules — Universal

- **Files < 500 lines, functions < 20 lines.** Refactor when over.
- **DRY.** Search before writing. Move code, don't copy.
- **No git commands.** CI handles git.
- **Regex on source code = illegal.** Use tree-sitter.
- **No placeholders, no silent no-ops.** Return `Result<T, E>`. Panics are bugs.
- **No legacy code.** Legacy = deleted. Copying files = illegal; move them.
- **Centralize global state** in a single `state.rs` (or its equivalent in any other language) per crate.
- **Dependency versions in `Cargo.toml`, CI workflows, and `.devcontainer/` stay in sync.**
- **Mandatory bug fix process:** [.claude/skills/fix-bug/SKILL.md](.claude/skills/fix-bug/SKILL.md).

## Hard Rules — Rust

- No `unwrap()` / `expect()` in production (tests may `expect` with a message).
- No `panic!` / `todo!` / `unimplemented!` / `unreachable!`.
- No `unsafe {}`. Workspace lint: `unsafe_code = "deny"`.
- All public items have `///` doc comments. Workspace lint: `missing_docs = "deny"`.
- `thiserror` for library errors; `anyhow` only in binaries.
- Pattern matching over casting. Expressions over statements. Iterator chains over imperative loops.
- Early return with `?`.
- Descriptive variable names — no single letters except in closures.
- No `#[allow(clippy::...)]` suppressions. Fix the underlying code.

## Logging

- `tracing` + `tracing-subscriber` only. Never `println!` / `eprintln!` for diagnostics.
- Log entry/exit of significant operations. Levels: `error | warn | info | debug | trace`.
- Structured fields, not string interpolation: `tracing::info!(file_count = 42, lang = "csharp")`.
- Never log file contents, user paths, or secrets. Log counts and hashes.
- The CLI's user-facing output is not a log — it goes to stdout via the renderer; diagnostics go to `tracing`.

## Testing

- **Coarse E2E only.** Drive the CLI end-to-end against fixtures; assert against rendered output. Never reach into internals.
- **Real binaries only.** Testing UI/extensions against a fake LSP or MCP is illegal — build and install the real binaries first.
- **`make test` is fail-fast.** Never `--no-fail-fast`.
- **`make test` enforces coverage** from `coverage-thresholds.json` at the repo root. Threshold ratchets up only (-1% rounding tolerance). Not env vars, not CI YAML.
- **Never delete a failing test. Never weaken an assertion.** Add more failing tests for broken functionality.
- **Meaningful assertions only.** `assert!(true)` is illegal. No try/catch that swallows errors and asserts success.
- **Deterministic.** No `sleep`, no timing dependencies, no random state.

## Profiler

`lsp-profiler` is one CPU and memory profiler for every language server built on lspkit. A language plugs in a runtime crate; the service, the wire, the LSP surface and the VS Code UI library are shared and never change per language.

**Spec:** [docs/specs/LSP-PROFILER-SPEC.md](docs/specs/LSP-PROFILER-SPEC.md). **Plan:** [docs/plans/LSP-PROFILER-PLAN.md](docs/plans/LSP-PROFILER-PLAN.md). Every `[PROF-…]` citation must resolve to exactly one spec heading (`make lint` runs `scripts/profiler-spec-citations.mjs`).

- The `[PROF-INVARIANTS]` are hard: nothing above `CpuBackend` / `MemoryBackend` names a tool, a language or a runtime; the Rust layer computes everything the UI shows; standard LSP first; no scraping of console output.
- The profiler crates carry their own stricter `[lints]` (no `unwrap`, `expect`, `panic`, indexing, `as` casts or `print!` on any path).
- Errors are `thiserror` types mapped to `[PROF-ERRORS]` codes; a failing tool is a classified error, never a crash.
- The wire model is `docs/models/profiler.td`; `crates/lsp-profiler/build.rs` generates `wire_generated.rs` and the UI's `wire-generated.ts` with typeDiagram (gitignored).
- `make test` builds the .NET engine and its fixture first (`dotnet` SDK required).

| Path | What |
|---|---|
| `crates/lsp-profiler` | The core: sessions, aggregation, exports, memory scoring, `ProfilerService`, `commands` |
| `crates/lsp-profiler-dotnet` | The .NET runtime: process classification, engine driver, CPU and memory backends |
| `dotnet/LspProfiler.DotNet.Engine` | The C# engine: EventPipe CPU sampling and heap snapshots, NDJSON out |
| `dotnet/fixtures/HotLeakApp` | The .NET test target: a hot loop and a static-rooted leak |
| `packages/lsp-profiler-ui` | The VS Code UI library (`make profiler-ui`, `make profiler-ui-test`) |
| `crates/lsp-profiler-python`, `crates/lsp-profiler-helper` | Parked runtimes: excluded from the workspace |

## Multi-Agent Coordination (Too Many Cooks)

If the TMC server is available: register on start (name, intent, files), lock files before editing, broadcast your plan, check messages periodically, release locks when done. Never edit a locked file — wait or take another approach.

## Build Commands

Cross-platform GNU Make. Exactly 7 standard targets:

```bash
make build   # cargo build --release --workspace
make test    # FAIL-FAST tests + coverage + threshold (ONLY test entry point)
make lint    # cargo clippy -- -D warnings (no formatting)
make fmt     # cargo fmt --all  (pass CHECK=1 for read-only check)
make clean   # cargo clean
make ci      # lint + test + build (full CI simulation)
make setup   # post-create dev environment setup (devcontainer hook)
```

Repo-specific helpers live in the `Repo-Specific Targets` section of the Makefile (e.g. `make doc`). `make test` runs `cargo llvm-cov` with `--fail-fast`, asserts measured ≥ threshold from `coverage-thresholds.json`, and exits non-zero on any failure. To debug a single test, invoke `cargo test <name> -- --nocapture` directly — that is not a Makefile target.

`make fmt`, `make lint`, `make test` are three separate non-overlapping targets.

## Repo Structure

```
lspkit/
├── CLAUDE.md                       # canonical agent instructions (this file)
├── AGENTS.md                       # pointer → CLAUDE.md
├── Makefile                        # 7 standard targets + Repo-Specific section
├── rustfmt.toml
├── coverage-thresholds.json        # single source of truth for thresholds
├── opencode.json                   # OpenCode pointer
├── .cursorrules / .windsurfrules   # pointers
├── .claude/skills/                 # ci-prep, code-dedup, fix-bug, spec-check,
│                                   # submit-pr, upgrade-packages
├── .clinerules/                    # Cline pointer
├── .devcontainer/devcontainer.json
├── .github/
│   ├── copilot-instructions.md     # pointer
│   ├── pull_request_template.md
│   └── workflows/ci.yml
├── .vscode/extensions.json
├── docs/
│   ├── specs/                      # behaviour & requirements (spec IDs live here)
│   └── plans/                      # how-to-achieve-goals + TODO checklists
├── crates/                         # workspace crates (lspkit, lspkit-server,
│                                   # lspkit-vfs, lspkit-live, lspkit-mcp,
│                                   # lspkit-config, lspkit-treesitter,
│                                   # lspkit-sidecar, lsp-profiler,
│                                   # lsp-profiler-dotnet — see docs/specs/)
├── dotnet/                         # the profiler's .NET engine + test fixture
└── packages/                       # editor packages (lspkit-debug, lsp-profiler-ui)
```

The canonical spec for the eight crates and the "one engine, two surfaces" pattern lives in [docs/specs/lspkit-spec.md](docs/specs/lspkit-spec.md).

### Cargo workspace lints (when Cargo.toml is created)

The Rust hard rules above are enforced via `[workspace.lints.rust]` and `[workspace.lints.clippy]` in `Cargo.toml`. The required deny-list is in [REPO-STANDARDS-SPEC](../AgentPMOWorkflow/docs/specs/REPO-STANDARDS-SPEC.md) under `[LINT-RUST]` and mirrored in `agent-pmo-skill/templates/linting/cargo-workspace-lints.toml`. Copy that block into the workspace `Cargo.toml` when scaffolding crates.
