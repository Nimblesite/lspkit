# agent-pmo:74cf183
# =============================================================================
# Standard Makefile — lsp_toolkit
# Cross-platform: Linux, macOS, Windows (via GNU Make)
# =============================================================================

.PHONY: build test lint fmt clean ci setup help doc

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
	cargo build --release --workspace --locked

## test: Fail-fast tests + coverage + threshold enforcement.
##       See REPO-STANDARDS-SPEC [TEST-RULES] and [COVERAGE-THRESHOLDS-JSON].
##       Uses cargo-nextest (stable-compatible fail-fast; default behaviour).
test:
	@echo "==> Testing (fail-fast + coverage + threshold)..."
	rustup component add llvm-tools-preview 2>/dev/null || true
	cargo llvm-cov nextest --workspace --all-targets --locked --lcov --output-path lcov.info
	$(MAKE) _coverage_check

## lint: Run all linters/analyzers (read-only). Does NOT format.
lint:
	@echo "==> Linting..."
	cargo clippy --release --workspace --all-targets --locked -- -D warnings

## fmt: Format all code in-place. Pass CHECK=1 for read-only check (CI use).
fmt:
	@echo "==> Formatting$(if $(CHECK), (check mode),)..."
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

# ---------------------------------------------------------------------------
# Private sub-recipes (NOT in .PHONY public list; underscore-prefixed)
# ---------------------------------------------------------------------------

_coverage_check:
	@if [ ! -f "$(COVERAGE_THRESHOLDS_FILE)" ]; then echo "FAIL: $(COVERAGE_THRESHOLDS_FILE) not found"; exit 1; fi; \
	if [ ! -f lcov.info ]; then echo "FAIL: lcov.info not found"; exit 1; fi; \
	FAILED=0; \
	for CRATE in $$(jq -r '.per_crate | keys[]' "$(COVERAGE_THRESHOLDS_FILE)"); do \
	  THRESHOLD=$$(jq -r --arg c "$$CRATE" '.per_crate[$$c]' "$(COVERAGE_THRESHOLDS_FILE)"); \
	  COV=$$(awk -v crate="$$CRATE" 'BEGIN{lh=0;lf=0;in_block=0} /^SF:/{in_block=(index($$0,"/crates/" crate "/src/")>0)?1:0} in_block&&/^LH:/{sub(/^LH:/,""); lh+=$$0} in_block&&/^LF:/{sub(/^LF:/,""); lf+=$$0} /^end_of_record/{in_block=0} END{printf "%d %d", lh, lf}' lcov.info); \
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
