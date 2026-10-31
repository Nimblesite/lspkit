# Release setup

One-time steps, in order. Every Trusted Publisher form takes: Owner `Nimblesite`, Repository `lspkit`, Workflow `release.yml`, Environment `release`.

1. **crates.io:** `cargo login` with a token scoped `publish-new` + `trusted-publishing`, then `scripts/bootstrap-crates.sh v0.0.1`. It publishes `lsp-profiler` and `lsp-profiler-dotnet` once, and attaches the Trusted Publisher to all 4 published crates through the crates.io API.
2. **npm:** `npm install -g npm@latest && npm login`, then `scripts/bootstrap-npm.sh v0.0.1`. It publishes `@nimblesite/lsp-profiler-ui` once and attaches its Trusted Publisher with `npm trust`.
3. **Delete the absorbed crates** [CRATE-RETIRE]: `lspkit-config`, `lspkit-live`, `lspkit-mcp`, `lspkit-sidecar`, `lspkit-vfs`, `lspkit-treesitter`. Open `https://crates.io/crates/<crate>/settings` → Delete crate. Delete `lspkit-live` before `lspkit-vfs`, because a crate with a dependent can't be deleted. If crates.io refuses, yank instead: `cargo yank --version 0.0.1 <crate>`. Then revoke the bootstrap token.
4. **npm:** on `https://www.npmjs.com/package/@nimblesite/lsp-profiler-ui/access`, set **Require 2FA and disallow tokens**.
5. **GitHub settings:**
   - Environments → `release`: add yourself as Required reviewer.
   - Branches → `main`: require a PR and the `CI` check.
   - Code security: Dependabot alerts and security updates on; CodeQL **Advanced**.
   - Secrets: delete `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN`.
6. **After this branch merges:** `git push origin main:dependabot-upgrades` (leave it unprotected).
7. **Release:** `gh release create v0.0.2 --target main --generate-notes`, then approve the `release` gate. Re-run a failed run to retry.
