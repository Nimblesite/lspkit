# lspkit website

Prose examples: `/docs/prose/` shows the documentation layout; `/prose/` shows a standalone reading page. Both share a Markdown specimen and the same `.prose` stylesheet as `/blog/one-engine/`. All reading pages include responsive header illustrations. Run `make website-test` (Python 3 required) for lint and generated-site regression checks; PR CI runs the same target.

From the repository root, `make website-build` installs locked dependencies and builds the static site; `make website-run` installs them and starts the live-reloading development server. Stop the server with Ctrl+C.

Small Eleventy 3 site built on [eleventy-plugin-techdoc](https://www.npmjs.com/package/eleventy-plugin-techdoc). Design: [Fieldwork](../docs/specs/website-design-system.md). One original SVG header illustration, one example journal post, and minimal docs. No runtime service or remote fonts.

```sh
make website-run
```

Open `http://localhost:8080`. `npm run lint` runs Oxlint with repository rules and warnings treated as errors; `npm run build` renders into ignored `_site/`. Set `SITE_URL` to the production origin before a production build. This sketch is local; deployment remains governed by the repository's release workflow.

Edit `src/blog/one-engine.md` for the example post, `src/index.njk` for the homepage, and `src/assets/css/styles.css` for all visual tokens. The plugin owns layouts, navigation behaviour, indexes and feeds. [WEB-CONTENT] [WEB-DESIGN] [WEB-ACCESS]

Lint configuration inherits the repository rules. Two narrowly scoped integration exceptions are documented here: Eleventy loads its config and JavaScript data through default exports, so `import/no-default-export` is off only for those two entrypoints; `node/no-process-env` is off only in `site.js` because that build-time data boundary reads the deployment origin. All remaining configured diagnostics fail the lint command.
