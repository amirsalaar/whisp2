CARGO = ~/.cargo/bin/cargo
NPM   = npm --prefix src-ui

# ── App ──────────────────────────────────────────────────────────────────────

.PHONY: dev build clean

# Start the full Tauri dev session (frontend + Rust hot-reload)
dev:
	RUST_LOG=whisp_rs=debug $(CARGO) tauri dev

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

# ── iOS ──────────────────────────────────────────────────────────────────────

.PHONY: ios-dev ios-build ios-typecheck ios-regen

# iOS dev session on a connected, unlocked device
ios-dev:
	$(CARGO) tauri ios dev

# iOS release build
ios-build:
	$(CARGO) tauri ios build

# Swift typecheck of iOS sources (host app + Live Activity shared files)
ios-typecheck:
	cd src-tauri/gen/apple && \
	xcrun -sdk iphonesimulator swiftc -typecheck \
	  -target arm64-apple-ios17.0-simulator \
	  -sdk "$$(xcrun --sdk iphonesimulator --show-sdk-path)" \
	  Shared/WhispActivityAttributes.swift \
	  Shared/WhispStopIntent.swift \
	  Shared/WhispLogger.swift \
	  Sources/whisp-rs/WhispIntent.swift

# Clean-regen Xcode project from project.yml (in-place merges have stale-state bugs)
ios-regen:
	rm -rf src-tauri/gen/apple/whisp-rs.xcodeproj
	cd src-tauri/gen/apple && xcodegen generate
