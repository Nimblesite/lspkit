# Release setup

Remaining one-time steps. The crates.io Trusted Publishers are already attached to `lsp-profiler`, `lsp-profiler-dotnet`, `lspkit` and `lspkit-server`.

1. **npm:** `npm login`, then `scripts/bootstrap-npm.sh v0.0.1`. On `https://www.npmjs.com/package/@nimblesite/lsp-profiler-ui/access`, set **Require 2FA and disallow tokens**.
2. **Delete the absorbed crates** [CRATE-RETIRE], in this order: `lspkit-live`, `lspkit-vfs`, `lspkit-config`, `lspkit-mcp`, `lspkit-sidecar`, `lspkit-treesitter`. Open `https://crates.io/crates/<crate>/settings` → Delete crate, then revoke the bootstrap token.
3. **GitHub settings:**
   - Environments → `release`: add yourself as Required reviewer.
   - Rulesets → `main protection`: set it to Active, requiring a PR and the `CI` check.
   - Code security: turn on Dependabot security updates.
4. **After this branch merges:** `git push origin main:dependabot-upgrades` (leave it unprotected).
5. **Release:** `gh release create v0.0.2 --target main --generate-notes`, then approve the `release` gate. Re-run a failed run to retry.
