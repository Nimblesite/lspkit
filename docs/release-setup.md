# Release setup — crates.io + npm

**No GitHub secrets are needed.** Both registries authenticate with OIDC. Do these steps once, in order.

Values used in every Trusted Publisher form below:

| Field | Value |
|---|---|
| Owner | `Nimblesite` |
| Repository | `lspkit` |
| Workflow | `release.yml` |
| Environment | `release` |

## 1. Bootstrap the 3 new packages (one command)

Trusted Publishing can only be set on a package that already exists. These 3 don't exist yet: `lsp-profiler`, `lsp-profiler-dotnet` and `@nimblesite/lsp-profiler-ui`.

```bash
cargo login            # token from crates.io/settings/tokens (scope: publish-new); your current one is rejected
npm login              # needs npm >= 11.5.1: npm i -g npm@latest
scripts/bootstrap-registries.sh v0.0.1
```

## 2. crates.io: add the Trusted Publisher to all 4 crates

For each crate, go to `https://crates.io/crates/<crate>/settings`, open **Trusted Publishing**, choose **Add**, then **GitHub**, and enter the table values.

`lsp-profiler` `lsp-profiler-dotnet` `lspkit` `lspkit-server`

Then revoke the token from step 1.

## 3. npm: add the Trusted Publisher to the package

Go to `https://www.npmjs.com/package/<pkg>/access`, open **Trusted Publisher**, choose **GitHub Actions**, and enter the table values. On the same page, set **Require 2FA and disallow tokens**.

`@nimblesite/lsp-profiler-ui`

## 4. GitHub (Settings)

- **Environments → `release`:** add yourself as a **Required reviewer**. Tag-only deployment (`v*.*.*`) is already set.
- **Branches → `main`:** require a PR and the `CI` check.
- **Code security:** turn on Dependabot alerts and security updates. Set CodeQL to **Advanced** (turn off default setup).
- **Secrets:** delete `CARGO_REGISTRY_TOKEN` and `NPM_TOKEN` if they exist.
- **After this branch merges to `main`:** `git push origin main:dependabot-upgrades`, and leave that branch unprotected.

## 5. Release

```bash
gh release create v0.0.2 --target main --generate-notes
```

Then approve the `release` gate on the workflow run. To retry, re-run the failed run; publishing skips anything already uploaded. `0.0.1` is taken, so start at `v0.0.2`.
