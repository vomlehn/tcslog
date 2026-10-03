# Makefile for automated Rust project creation with Claude Code

# Run every recipe under bash with pipefail. Without it, the exit
# status of a pipeline is the status of its last stage, so a failing
# `cargo build ... | tee build.out` would report success and `make
# install` would happily install a stale binary.
SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c

# Portable command abstractions (override per-OS as needed)
RM      := rm -f
RMDIR   := rm -rf

# Extra flags for `cargo build`; `make release` sets it to --release.
RELEASE =

# Default target
.PHONY: all
all: build			## Build the project (default target)

# Display help. Every entry is the `##` comment on the target's own
# rule, so the list cannot drift from the set of targets that exist.
.PHONY: help
help:				## Show this help
	@echo "Makefile for Rust Project with Claude Code"
	@echo ""
	@echo "Usage:"
	@grep -hE '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) \
		| sed 's/:.*## /|/' \
		| awk -F'|' '{printf "  make %-14s - %s\n", $$1, $$2}'

# Build the project. The subshell groups the whole build so that one
# `tee` captures all of it.
.PHONY: build
build:				## Build the Rust project
	( \
		echo "Building the project..."; \
		cargo build $(RELEASE); \
		$(MAKE) -C docs; \
		echo "[OK] Build complete"; \
	) 2>&1 | tee build.out

# Run tests
.PHONY: test
test:				## Run all tests
	@echo "Running tests..."
	cargo test
	$(MAKE) -C test
	@echo "[OK] Tests complete"

# Clean build artifacts
.PHONY: clean
clean:				## Remove build artifacts
	@echo "Cleaning build artifacts..."
	-cargo clean
	$(RM) build.out
	$(MAKE) -C docs clean
	@echo "[OK] Clean complete"

# Clean everything including generated source
.PHONY: distclean
distclean: clean		## Remove build artifacts and all generated files
	@echo "Removing all generated files..."
	$(RM) Cargo.lock
	$(RMDIR) target
	$(MAKE) -C docs distclean
	@echo "[OK] Project reset"

# There is no install target here. The only binaries worth installing,
# tcslog-dump and tcslog-dumphdr, are the tcslog-tools crate in its own
# repository, and `make -C ../tcslog-tools install` installs them.
# tcslog-gen stays uninstalled on purpose: it exists to give the tests
# segment files to read, and it writes deliberately damaged ones.

# Check code quality
.PHONY: check
check:				## Run cargo check, clippy, and fmt --check
	@echo "Running cargo check..."
	cargo check --all-targets
	cargo clippy --all-targets -- -D warnings
	cargo fmt -- --check

# Format code
.PHONY: format
format:				## Format the code with cargo fmt
	cargo fmt

# Create release build. Reuses `build` so there is one build path.
.PHONY: release
release: test			## Build with optimizations into target/release
	@echo "Creating release build..."
	$(MAKE) build RELEASE=--release
	@echo "[OK] Release binaries are in target/release/"
