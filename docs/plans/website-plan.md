# Website sketch

## Responsive prose follow-up [WEB-ACCESS-PROSE]

- [x] Inspect techdoc layouts and identify missing shared prose coverage and off-canvas mobile docs navigation.
- [x] Add and run a failing generated-site regression for prose examples and navigable docs/blog pages: missing `/docs/prose/` link reproduced before the fix.
- [x] Apply shared `.prose` styling, in-flow mobile docs navigation, header illustrations and linked docs/standalone specimens from one Markdown source.
- [x] Run regression, lint, build and repository tests; record environment limits below.
- [x] Review duplication and remove temporary website build output; disk check reports 17 GiB available.

Website checks: both generated-site regressions pass; Oxlint and `git diff --check` pass. The final Deslop scan found no website duplication clusters. CSS is 172 lines, below the 2,000-line budget. Browser viewport verification remains unavailable: the computer-use host reports no browsers or native apps. Kimi's read-only review identified overlapping mobile menus, long-token overflow, table containment and landscape menu scrolling; the changes address these without adding runtime JavaScript.

Branch: `website` created directly from fetched `origin/main`; all pending changes are website-related. The previous `updates` tree matches `origin/main` exactly.

Repository validation: `make fmt CHECK=1`, profiler UI build and all three path regressions, `make lint`, `make build`, `make website-test`, Ruff and whitespace checks pass. `make test` is **not green**: 125 tests passed before `memory_profiling_sees_the_leak_its_root_and_the_collection` failed because the runtime dropped 1,713 heap events after its 1,024 MB capture buffer filled; fail-fast left 352 tests unrun. No profiler source, test, coverage threshold or timeout was changed. Browser viewport/interaction checks remain unverified because the browser host is unavailable.

Added `make website-test` to PR CI so the static website checks stay enforced. Updated local `main` to `origin/main` and removed the obsolete local `updates` branch only after verifying its tree was identical to the already-squashed main tree; the working tree has one feature branch, `website`.

- [x] Remove the Atom subscription footer link [WEB-CONTENT]: the plugin generates URL-encoded article markup instead of HTML content and omits feed/entry authors. Feed generation remains upstream; the footer no longer promotes it.

- [x] Add root `website-build` and `website-run` Make targets with shared locked dependency setup [WEB-CONTENT].
- [x] Verify the Make build, Oxlint, and HTTP response from the Make development server; stop the verification server afterward.

- [x] Read the repository spec and techdoc package contract.
- [x] Define the design system [WEB-DESIGN].
- [x] Build the Eleventy shell and one illustrated example post [WEB-CONTENT].
- [x] Verify the build and generated links [WEB-ACCESS].
- [ ] Visually verify responsive layout and interactions: blocked because the computer-use browser host is unavailable.
- [x] Check duplication: baseline and final CLI scans found no clusters involving `website/`; existing repository findings remain outside this sketch.
- [x] Clean temporary output and check disk space: stopped the owned preview server, removed generated `_site/`; about 13 GiB remains available. Dependencies remain for local development.

Final checks: Oxlint passed with warnings treated as errors; the final Eleventy build passed.

Scope: a local starter in `website/`, using the existing lspkit identity. Publishing is deferred to the repository's release workflow; repository instructions prohibit git operations without an explicit request. No changes to crates or existing CI gates.

Validation: Eleventy generated twelve files, including eight HTML pages. Parsed every HTML page to check local links, asset references, fragments, image alternatives, a single H1 and JSON-LD validity. HTTP checks passed for homepage, docs, blog index, article and illustration. A read-only Kimi k3.1-pro review informed theme and mobile CSS; its recommendations were reviewed against the installed plugin. The plugin's menu lacks initial `aria-expanded` and `aria-controls` metadata, although toggling sets the expanded state; this upstream limitation is retained rather than copying its templates into the sketch.
