# Website sketch

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
