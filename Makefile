# ============================================================================
# Zaguán Blade — Makefile
# ============================================================================
# Common targets:
#   make install-deps   — install frontend dependencies (bun install)
#   make dev            — run the desktop app with live reload
#   make build          — full release build (bun run tauri build)
#   make build-frontend — frontend build only (tsc + vite build)
#   make test           — run frontend + backend tests
#   make test-frontend  — run frontend tests only
#   make test-backend   — run backend (Rust) tests only
#   make lint           — run frontend (eslint) + backend (clippy) linters
#   make clippy         — run cargo clippy on the backend
#   make check          — TypeScript type-check (tsc --noEmit)
#   make cargo-check    — cargo check with optional CARGO_CHECK_ARGS
#   make fmt            — format Rust source (cargo fmt)
#   make fmt-check      — check Rust formatting without modifying files
#   make install        — install the release binary to /usr/local/bin
#   make uninstall      — remove the binary from /usr/local/bin
#   make clean          — remove frontend and backend build artifacts
#   make ci             — full CI pipeline: check + lint + test + build
#   make ci-check       — CI checks only: check + lint + test (no build)
#   make ci-build       — CI build with optional TARGET and BUNDLES overrides
#
# CI overrides (optional):
#   make ci-build TARGET=aarch64-apple-darwin BUNDLES="app dmg"
# ============================================================================

# --- Configurable variables -------------------------------------------------
BUN          ?= bun
CARGO        ?= cargo
RUST_TARGET  ?=
BUNDLES      ?=
CARGO_CHECK_ARGS ?=
INSTALL_DIR  ?= /usr/local/bin
BIN_NAME     ?= zblade
RELEASE_BIN  := src-tauri/target/$(if $(RUST_TARGET),$(RUST_TARGET)/,)release/$(BIN_NAME)

# Pass-through args for tauri build (empty by default)
TAURI_BUILD_ARGS := $(if $(RUST_TARGET),--target $(RUST_TARGET),) $(if $(BUNDLES),--bundles $(BUNDLES),)

# --- Default target ---------------------------------------------------------
.DEFAULT_GOAL := build

# --- Phony targets ----------------------------------------------------------
.PHONY: help install-deps dev build build-frontend release test test-frontend test-backend \
        lint lint-frontend lint-backend clippy check cargo-check fmt fmt-check \
        install uninstall clean clean-frontend clean-backend \
        ci ci-check ci-build

# --- Help -------------------------------------------------------------------
help: ## Show this help
	@echo "Zaguán Blade — available targets:"
	@echo ""
	@echo "  make install-deps   Install frontend dependencies (bun install)"
	@echo "  make dev            Run the desktop app with live reload"
	@echo "  make build          Full release build (bun run tauri build)"
	@echo "  make build-frontend Frontend build only (tsc + vite build)"
	@echo "  make test           Run frontend + backend tests"
	@echo "  make test-frontend  Run frontend tests only"
	@echo "  make test-backend   Run backend (Rust) tests only"
	@echo "  make lint           Run frontend (eslint) + backend (clippy) linters"
	@echo "  make clippy         Run cargo clippy on the backend"
	@echo "  make check          TypeScript type-check (tsc --noEmit)"
	@echo "  make cargo-check    cargo check (override with CARGO_CHECK_ARGS=...)"
	@echo "  make fmt            Format Rust source (cargo fmt)"
	@echo "  make fmt-check      Check Rust formatting without modifying files"
	@echo "  make install        Install the release binary to $(INSTALL_DIR)"
	@echo "  make uninstall      Remove the binary from $(INSTALL_DIR)"
	@echo "  make clean          Remove frontend + backend build artifacts"
	@echo "  make ci             Full CI pipeline: check + lint + test + build"
	@echo "  make ci-check       CI checks only: check + lint + test (no build)"
	@echo "  make ci-build       CI build with TARGET/BUNDLES overrides"
	@echo ""
	@echo "CI overrides:"
	@echo "  make ci-build TARGET=aarch64-apple-darwin BUNDLES=\"app dmg\""
	@echo "  make cargo-check CARGO_CHECK_ARGS=\"--features devtools\""

# --- Dependencies -----------------------------------------------------------
install-deps: ## Install frontend dependencies
	$(BUN) install

# --- Development ------------------------------------------------------------
dev: install-deps ## Run the desktop app with live reload
	$(BUN) run tauri dev

# --- Build ------------------------------------------------------------------
build: install-deps ## Full release build
	$(BUN) run tauri build $(TAURI_BUILD_ARGS)

release: build ## Alias for 'build'

build-frontend: install-deps ## Frontend build only (tsc + vite build)
	$(BUN) run build

# --- Tests ------------------------------------------------------------------
test: test-frontend test-backend ## Run all tests

test-frontend: ## Run frontend tests
	$(BUN) test src/**/*.test.ts

test-backend: ## Run backend (Rust) tests
	cd src-tauri && $(CARGO) test --workspace

# --- Linting ----------------------------------------------------------------
lint: lint-frontend lint-backend ## Run all linters

lint-frontend: ## Run frontend linter (eslint)
	$(BUN) run lint

lint-backend: clippy ## Run backend linter (cargo clippy)

clippy: ## Run cargo clippy on the backend
	cd src-tauri && $(CARGO) clippy --all-targets --all-features -- -D warnings

# --- Type checking ----------------------------------------------------------
check: ## TypeScript type-check
	$(BUN) x tsc --noEmit

# --- Cargo check (pass-through args via CARGO_CHECK_ARGS) --------------------
cargo-check: ## Run cargo check on the backend (override with CARGO_CHECK_ARGS=...)
	cd src-tauri && $(CARGO) check --workspace $(CARGO_CHECK_ARGS)

# --- Formatting -------------------------------------------------------------
fmt: ## Format Rust source
	cd src-tauri && $(CARGO) fmt

fmt-check: ## Check Rust formatting without modifying
	cd src-tauri && $(CARGO) fmt --check

# --- Installation (no sudo — run 'sudo make install' if needed) -------------
install: $(RELEASE_BIN) ## Install the release binary to $(INSTALL_DIR)
	@echo "Installing $(BIN_NAME) to $(INSTALL_DIR)/"
	@install -Dm755 $(RELEASE_BIN) $(INSTALL_DIR)/$(BIN_NAME)
	@echo "Installed: $$(which $(BIN_NAME) 2>/dev/null || echo '$(INSTALL_DIR)/$(BIN_NAME)')"

uninstall: ## Remove the binary from $(INSTALL_DIR)
	@echo "Removing $(INSTALL_DIR)/$(BIN_NAME)"
	@rm -f $(INSTALL_DIR)/$(BIN_NAME)

# --- Clean ------------------------------------------------------------------
clean: clean-frontend clean-backend ## Remove all build artifacts

clean-frontend: ## Remove frontend build artifacts
	rm -rf dist node_modules/.vite coverage
	@echo "Frontend build artifacts cleaned"

clean-backend: ## Remove backend (Rust) build artifacts
	cd src-tauri && $(CARGO) clean
	@echo "Backend build artifacts cleaned"

# --- CI targets -------------------------------------------------------------
ci-check: check lint test ## CI checks: type-check + lint + test (no build)

ci: ci-check build ## Full CI pipeline: check + lint + test + build

ci-build: install-deps ## CI build with optional TARGET and BUNDLES overrides
	$(BUN) run tauri build $(TAURI_BUILD_ARGS)
