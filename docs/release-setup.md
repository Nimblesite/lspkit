# Release setup

Remaining one-time steps. Trusted Publishers (`Nimblesite/lspkit`, `release.yml`, environment `release`) are already attached to the crates `lsp-profiler`, `lsp-profiler-dotnet`, `lspkit` and `lspkit-server`, and to the npm package `@nimblesite/lsp-profiler-ui`, which also requires 2FA and disallows tokens.

1. **Delete the absorbed crates** [CRATE-RETIRE], in this order: `lspkit-live`, `lspkit-vfs`, `lspkit-config`, `lspkit-mcp`, `lspkit-sidecar`, `lspkit-treesitter`. Open `https://crates.io/crates/<crate>/settings` → Delete crate, then revoke the bootstrap token.
2. **GitHub settings:**
   - Environments → `release`: add yourself as Required reviewer.
   - Rulesets → `main protection`: set it to Active, requiring a PR and the `CI` check.
   - Code security: turn on Dependabot security updates.
3. **After this branch merges:** `git push origin main:dependabot-upgrades` (leave it unprotected).
4. **Release:** `gh release create v0.0.2 --target main --generate-notes`, then approve the `release` gate. Re-run a failed run to retry.
