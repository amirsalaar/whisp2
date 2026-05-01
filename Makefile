CARGO = ~/.cargo/bin/cargo
NPM   = npm --prefix src-ui

# ── App ──────────────────────────────────────────────────────────────────────

.PHONY: dev build clean

# Start the full Tauri dev session (frontend + Rust hot-reload)
dev:
	$(NPM) run dev &
	$(CARGO) tauri dev

# Production build (.app bundle in src-tauri/target/release/bundle/)
build:
	$(CARGO) tauri build

# Remove all compiled artefacts
clean:
	$(CARGO) clean
	rm -rf src-ui/dist

# ── Frontend ─────────────────────────────────────────────────────────────────

.PHONY: ui-dev ui-build ui-lint ui-install

# Vite dev server only (no Rust)
ui-dev:
	$(NPM) run dev

# TypeScript compile + Vite bundle
ui-build:
	$(NPM) run build

# ESLint
ui-lint:
	$(NPM) run lint

# Install node_modules
ui-install:
	$(NPM) install

# ── Rust ─────────────────────────────────────────────────────────────────────

.PHONY: check test fmt lint-rs

# Type-check without linking (fast)
check:
	$(CARGO) check

# Run all unit tests
test:
	$(CARGO) test

# Format with rustfmt
fmt:
	$(CARGO) fmt

# Clippy lints
lint-rs:
	$(CARGO) clippy -- -D warnings
