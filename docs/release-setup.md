# Release setup

One-time steps, in order. Every Trusted Publisher form takes: Owner `Nimblesite`, Repository `lspkit`, Workflow `release.yml`, Environment `release`.

1. **crates.io:** `cargo login` with a token scoped `publish-new` + `trusted-publishing`, then `scripts/bootstrap-crates.sh v0.0.1`. It publishes `lsp-profiler` and `lsp-profiler-dotnet` once, and attaches the Trusted Publisher to all 4 published crates through the crates.io API.
2. **npm:** `npm login` (npm >= 11.5.1), then `scripts/bootstrap-npm.sh v0.0.1` to publish `@nimblesite/lsp-profiler-ui` once.
3. **Yank the absorbed crates:** `for c in lspkit-config lspkit-live lspkit-mcp lspkit-sidecar lspkit-vfs lspkit-treesitter; do cargo yank --version 0.0.1 "$c"; done`, then revoke the token.
4. **npm Trusted Publisher:** open `https://www.npmjs.com/package/@nimblesite/lsp-profiler-ui/access` → Trusted Publisher → GitHub Actions, and set **Require 2FA and disallow tokens**.
5. **GitHub settings:**
   - Environments → `release`: add yourself as Required reviewer.
   - Branches → `main`: require a PR and the `CI` check.
   - Code security: Dependabot alerts and security updates on; CodeQL **Advanced**.
   - Secrets: delete `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN`.
6. **After this branch merges:** `git push origin main:dependabot-upgrades` (leave it unprotected).
7. **Release:** `gh release create v0.0.2 --target main --generate-notes`, then approve the `release` gate. Re-run a failed run to retry.
