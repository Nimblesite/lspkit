# agent-pmo:74cf183
# =============================================================================
# Standard Makefile — lspkit
# Cross-platform: Linux, macOS, Windows (via GNU Make)
# =============================================================================

.PHONY: build test lint fmt clean ci setup help doc profiler-ui profiler-ui-test

# ---------------------------------------------------------------------------
# OS Detection
# ---------------------------------------------------------------------------
ifeq ($(OS),Windows_NT)
  SHELL := powershell.exe
  .SHELLFLAGS := -NoProfile -Command
  RM = Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
  MKDIR = New-Item -ItemType Directory -Force
  HOME ?= $(USERPROFILE)
else
  RM = rm -rf
  MKDIR = mkdir -p
endif

# The _coverage_check recipe is POSIX sh. On Windows GNU make defaults SHELL
# to powershell, so the coverage-gate regression tests must drive it through
# Git Bash; CI uses /bin/sh.
ifeq ($(OS),Windows_NT)
  POSIX_SHELL ?= C:/Program Files/Git/bin/bash.exe
else
  POSIX_SHELL ?= /bin/sh
endif

# ---------------------------------------------------------------------------
# Coverage — single source of truth is coverage-thresholds.json
# See REPO-STANDARDS-SPEC [COVERAGE-THRESHOLDS-JSON].
# ---------------------------------------------------------------------------
COVERAGE_THRESHOLDS_FILE := coverage-thresholds.json

# =============================================================================
# Standard Targets (7 — portfolio-wide, identical across every repo)
# =============================================================================

## build: Compile/assemble all artifacts
build:
	@echo "==> Building..."
	cargo build --release --workspace --features lspkit-server/pull --locked

## test: Fail-fast tests + coverage + threshold enforcement.
##       See REPO-STANDARDS-SPEC [TEST-RULES] and [COVERAGE-THRESHOLDS-JSON].
##       Uses cargo-nextest (stable-compatible fail-fast; default behaviour).
test:
	@echo "==> Testing (fail-fast + coverage + threshold)..."
	rustup component add llvm-tools-preview 2>/dev/null || true
	$(MAKE) _profiler_dotnet
	cargo llvm-cov nextest --workspace --all-targets --features lspkit-server/pull --locked --lcov --output-path lcov.info
	$(MAKE) _coverage_check
# The gate itself must not fail open (missing jq, corrupt JSON, invalid
# thresholds all have to fail closed). This regression cannot be dropped.
	$(MAKE) _coverage_check_test
	$(MAKE) _typediagram_test

## lint: Run all linters/analyzers (read-only). Does NOT format.
lint:
	@echo "==> Linting..."
	cargo clippy --release --workspace --all-targets --features lspkit-server/pull --locked -- -D warnings
	$(MAKE) _profiler_spec_check

## fmt: Format all code in-place. Pass CHECK=1 for read-only check (CI use).
fmt:
	@echo "==> Formatting$(if $(CHECK), (check mode),)..."
	$(MAKE) _profiler_codegen
	cargo fmt --all$(if $(CHECK), -- --check,)

## clean: Remove all build artifacts
clean:
	@echo "==> Cleaning..."
	cargo clean
	$(RM) lcov.info

## ci: lint + test + build (full CI simulation)
ci: lint test build

## setup: Post-create dev environment setup (used by devcontainer)
setup:
	@echo "==> Setting up development environment..."
	rustup component add rustfmt clippy llvm-tools-preview
	cargo install cargo-llvm-cov cargo-nextest cargo-watch
	@echo "==> Setup complete. Run 'make ci' to validate."

## help: List all available targets
help:
	@echo "Standard targets:"
	@echo "  build  - Compile/assemble all artifacts"
	@echo "  test   - Fail-fast tests + coverage + threshold enforcement"
	@echo "  lint   - All linters/analyzers (read-only, no formatting)"
	@echo "  fmt    - Format all code in-place (CHECK=1 for read-only CI check)"
	@echo "  clean  - Remove build artifacts"
	@echo "  ci     - lint + test + build (full CI simulation)"
	@echo "  setup  - Post-create dev environment setup"
	@echo ""
	@echo "Repo-specific targets:"
	@echo "  doc    - Build cargo workspace documentation (no deps)"
	@echo "  profiler-ui      - Build the profiler's VS Code UI package"
	@echo "  profiler-ui-test - Test the profiler UI package against a mock server"

# ---------------------------------------------------------------------------
# Private sub-recipes (NOT in .PHONY public list; underscore-prefixed)
# ---------------------------------------------------------------------------

## The profiler's .NET engine and the fixture its e2e tests profile (Release).
## lsp-profiler-dotnet's tests run both. [PROF-TESTING]
_profiler_dotnet:
	dotnet build -c Release dotnet/LspProfiler.DotNet.Engine
	dotnet build -c Release dotnet/fixtures/HotLeakApp
	dotnet build -c Release dotnet/fixtures/HotLeakAppFs

## The profiler's gitignored wire model ([PROF-WIRE-GEN]). build.rs regenerates
## it on every build; rustfmt needs it before anything has been built.
_profiler_codegen:
	node scripts/typediagram/generate.mjs

## The wire-model generator end to end, and the CLI capture it relies on ([PROF-WIRE-GEN]).
_typediagram_test:
	node --test scripts/typediagram/generate.test.mjs scripts/typediagram/run-cli.test.mjs

## Every [PROF-*] citation in the profiler resolves to one spec heading.
_profiler_spec_check:
	node scripts/profiler-spec-citations.mjs

## Coverage-gate regression: coarse CLI tests driving the real
## _coverage_check target against fixture LCOV/JSON files (missing jq, missing
## inputs, corrupt JSON, empty/nonnumeric/fractional/out-of-range thresholds
## must all fail closed). $(MAKE)/$(POSIX_SHELL) keep it cross-platform.
_coverage_check_test:
	node scripts/coverage-gate.test.mjs --make "$(MAKE)" --shell "$(POSIX_SHELL)"

_coverage_check:
	@if [ ! -f "$(COVERAGE_THRESHOLDS_FILE)" ]; then echo "FAIL: $(COVERAGE_THRESHOLDS_FILE) not found"; exit 1; fi; \
	if [ ! -f lcov.info ]; then echo "FAIL: lcov.info not found"; exit 1; fi; \
	command -v jq >/dev/null 2>&1 || { echo "FAIL: jq not found — install jq so coverage thresholds cannot be skipped"; exit 1; }; \
	jq -e '(.per_crate | type) == "object" and (.per_crate | length) > 0 and ([.per_crate[] | type == "number" and . == floor and . >= 0 and . <= 100] | all)' "$(COVERAGE_THRESHOLDS_FILE)" >/dev/null || { echo "FAIL: $(COVERAGE_THRESHOLDS_FILE) per_crate must be a nonempty map of integer thresholds 0..100"; exit 1; }; \
	set -e; \
	FAILED=0; \
	CRATES=$$(jq -r '.per_crate | keys[]' "$(COVERAGE_THRESHOLDS_FILE)" | tr -d '\r'); \
	if [ -z "$$CRATES" ]; then echo "FAIL: could not list crates from $(COVERAGE_THRESHOLDS_FILE)"; exit 1; fi; \
	for CRATE in $$CRATES; do \
	  THRESHOLD=$$(jq -r --arg c "$$CRATE" '.per_crate[$$c]' "$(COVERAGE_THRESHOLDS_FILE)" | tr -d '\r') || exit 1; \
	  if [ -z "$$THRESHOLD" ] || [ "$$THRESHOLD" = "null" ]; then echo "FAIL: $$CRATE — threshold missing in $(COVERAGE_THRESHOLDS_FILE)"; exit 1; fi; \
	  COV=$$(awk -v crate="$$CRATE" 'BEGIN{lh=0;lf=0;in_block=0} /^SF:/{in_block=(index($$0,"/crates/" crate "/src/")>0)?1:0} in_block&&/^LH:/{sub(/^LH:/,""); lh+=$$0} in_block&&/^LF:/{sub(/^LF:/,""); lf+=$$0} /^end_of_record/{in_block=0} END{printf "%d %d", lh, lf}' lcov.info) || exit 1; \
	  LH=$${COV% *}; LF=$${COV#* }; \
	  if [ "$$LF" -eq 0 ]; then echo "FAIL: $$CRATE — no lines in lcov.info"; FAILED=1; continue; fi; \
	  PCT=$$(awk "BEGIN{printf \"%.2f\", $$LH/$$LF*100}"); \
	  PCT_INT=$$(awk "BEGIN{printf \"%d\", $$LH/$$LF*100}"); \
	  if [ "$$PCT_INT" -lt "$$THRESHOLD" ]; then \
	    printf "FAIL: %-20s %s%% < %s%%\n" "$$CRATE" "$$PCT" "$$THRESHOLD"; FAILED=1; \
	  else \
	    printf "OK:   %-20s %s%% >= %s%%\n" "$$CRATE" "$$PCT" "$$THRESHOLD"; \
	  fi; \
	done; \
	if [ "$$FAILED" -ne 0 ]; then echo "FAIL: one or more crates below per-crate threshold"; exit 1; fi

# =============================================================================
# Repo-Specific Targets
#
# Targets below this line are specific to this repo and are NOT part of the
# standard 7-target interface.
#
# Rules:
#   - MUST NOT duplicate or shadow any of the 7 standard targets above.
# =============================================================================

## doc: Build cargo workspace documentation (no deps)
doc:
	@echo "==> Building docs..."
	cargo doc --workspace --no-deps --locked

## profiler-ui: Build the profiler's VS Code UI library (needs the wire model
## that `cargo build` generates).
profiler-ui:
	cargo build -p lsp-profiler --locked
	cd packages/lsp-profiler-ui && npm ci && npm run build

## profiler-ui-test: Test the UI library against a mock language server.
profiler-ui-test: profiler-ui
	cd packages/lsp-profiler-ui && npm test
