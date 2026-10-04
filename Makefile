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

# Where `make install` puts the C binding. Defaults to $HOME, as the
# tcslog-tools Makefile does for its binaries; override PREFIX for
# somewhere else and DESTDIR to stage the install for packaging.
PREFIX  ?= $(HOME)
DESTDIR ?=
INCDIR  := $(DESTDIR)$(PREFIX)/include
LIBDIR  := $(DESTDIR)$(PREFIX)/lib
# What install puts there: the generated header, and both libraries, so
# a consumer can link either statically or dynamically.
CAPI_HEADER := tcslog-c/include/tcslog.h
CAPI_LIBS   := libtcslog_c.a libtcslog_c.so

# The C compiler for the binding's smoke test, and the warnings it is
# held to. -Werror because a warning in a 180-line test is a mistake in
# the test, not noise to scroll past.
CC          := cc
CAPI_CFLAGS := -Wall -Wextra -Werror -std=c11
# What `make capi-memcheck` adds. -g so a report names lines rather
# than addresses, and no optimization so the frames are the real ones.
CAPI_SAN_FLAGS := -fsanitize=address,undefined -fno-omit-frame-pointer -g -O0
# Where cargo puts the staticlib the smoke test links. `make release`
# does not move it, because RELEASE reaches cargo and not this.
CARGO_TARGET_DIR ?= target/debug

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
build:	docs			## Build the Rust project
	echo "Building the project..."
	cargo build $(RELEASE)
	echo "[OK] Build complete"

.PHONY: docs
docs:
	$(MAKE) -C docs

# Run tests
.PHONY: test
test:				## Run all tests
	@echo "Running tests..."
	cargo test
	$(MAKE) -C test
	$(MAKE) capi-test
	@echo "[OK] Tests complete"

# Build and run the C binding's smoke test: tcslog-c/examples/smoke.c
# compiled against the generated header and linked against the
# staticlib, which is what a C consumer does. The Rust unit tests in
# tcslog-c call the same functions as Rust, so they would pass a header
# that described the wrong argument order; only this catches that.
#
# The log is written into a temporary directory that is removed
# afterwards whether the test passed or not.
.PHONY: capi-test
capi-test:			## Compile and run the C binding's smoke test
	@echo "Running the C binding smoke test..."
	cargo build $(RELEASE) -p tcslog-c
	tmp=$$(mktemp -d -t tcslog-capi.XXXXXXXXXX); \
	bin=$$tmp/smoke; \
	trap 'rm -rf "$$tmp"' EXIT; \
	$(CC) $(CAPI_CFLAGS) -o $$bin tcslog-c/examples/smoke.c \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	$$bin $$tmp
	@echo "[OK] C binding smoke test passed"

# Run the smoke test under the address and undefined-behaviour
# sanitizers, which is what checks the handles for leaks and double
# frees: they are boxed on the Rust side and released from C, and
# nothing else here would notice if a close leaked one.
#
# The Rust staticlib is built without sanitizers, which is enough:
# LeakSanitizer intercepts the process's allocator, and Rust's default
# allocator on this platform is the system one, so a box leaked across
# the boundary is still reported. ASAN_OPTIONS asks for the leak check
# at exit, which is on by default on Linux and off elsewhere.
#
# Not part of `make test`: it needs a compiler with the sanitizers,
# which not every toolchain this builds on has.
.PHONY: capi-memcheck
capi-memcheck:			## Run the C smoke test under ASan and UBSan
	@echo "Running the C binding smoke test under sanitizers..."
	cargo build $(RELEASE) -p tcslog-c
	tmp=$$(mktemp -d -t tcslog-capi-san.XXXXXXXXXX); \
	bin=$$tmp/smoke; \
	trap 'rm -rf "$$tmp"' EXIT; \
	$(CC) $(CAPI_CFLAGS) $(CAPI_SAN_FLAGS) -o $$bin \
		tcslog-c/examples/smoke.c \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1 $$bin $$tmp
	@echo "[OK] No leak or undefined behaviour reported"

# Regenerate tcslog-c/include/tcslog.h from tcslog-c/src/lib.rs. Needs
# `cargo install cbindgen`, as `make check` does.
.PHONY: header
header:				## Regenerate the C header from tcslog-c/src/lib.rs
	cd tcslog-c && cbindgen --config cbindgen.toml --crate tcslog-c \
		--output include/tcslog.h
	@echo "[OK] tcslog-c/include/tcslog.h regenerated"

# Install the C binding: the header and both libraries. The Rust
# library is not installed, having no use outside cargo, and neither is
# a pkg-config file -- a consumer needs -ltcslog_c and nothing else, so
# there is nothing for one to say that -L and -l do not.
#
# Built with $(RELEASE) honoured, so `make release install` installs the
# optimized libraries; CARGO_TARGET_DIR says which directory those
# landed in.
.PHONY: install
install: header		## Install the C header and libraries under PREFIX
	@echo "Installing the C binding to $(DESTDIR)$(PREFIX)..."
	cargo build $(RELEASE) -p tcslog-c
	install -d $(INCDIR) $(LIBDIR)
	install -m 644 $(CAPI_HEADER) $(INCDIR)/
	for lib in $(CAPI_LIBS); do \
		install -m 644 $(CARGO_TARGET_DIR)/$$lib $(LIBDIR)/; \
	done
	@echo "[OK] Installed $(CAPI_HEADER) to $(INCDIR)/"
	@echo "[OK] Installed $(CAPI_LIBS) to $(LIBDIR)/"
	@echo "     Compile against it with:"
	@echo "       cc prog.c -I$(PREFIX)/include -L$(PREFIX)/lib -ltcslog_c"

# Removes what install put there, honouring the same two variables.
# Nothing else in those directories is touched.
.PHONY: uninstall
uninstall:			## Remove the installed C header and libraries
	@echo "Removing the C binding from $(DESTDIR)$(PREFIX)..."
	$(RM) $(INCDIR)/tcslog.h
	$(RM) $(addprefix $(LIBDIR)/,$(CAPI_LIBS))
	@echo "[OK] Uninstalled from $(DESTDIR)$(PREFIX)"

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
# repository. tcslog-gen stays uninstalled on purpose: it exists to give
# the tests segment files to read

# Check code quality
# The library's README is generated from its own documentation, so
# checking it is checking that the two have not drifted. Intralinks are
# stripped rather than resolved: resolving them wants a pinned nightly,
# and `SegId` reads as well as a link to it in a README.
.PHONY: check
check:				## Run cargo check, clippy, fmt, and the README check
	@echo "Running cargo check..."
	cargo check --all-targets
	# The read-only configuration, which nothing else here builds: the
	# scenario suite reaches it only through tcslog-tools, so a `write`
	# item exported without its cfg compiled fine until that suite ran.
	cargo check -p tcslog --no-default-features
	cargo clippy --all-targets -- -D warnings
	cargo fmt -- --check
	cargo rdme --check -w tcslog --intralinks-strip-links
	@echo "Checking the C header is current..."
	cd tcslog-c && cbindgen --config cbindgen.toml --crate tcslog-c \
		--output /dev/stdout 2>/dev/null \
		| diff -u include/tcslog.h - \
		|| { echo "tcslog-c/include/tcslog.h is out of date; run 'make header'" 1>&2; \
		     exit 1; }

# Regenerate the library's README from tcslog/src/lib.rs. Needs
# `cargo install cargo-rdme`, as `make check` does.
.PHONY: readme
readme:				## Regenerate tcslog/README.md from the library docs
	cargo rdme -w tcslog --intralinks-strip-links
	@echo "[OK] tcslog/README.md regenerated"

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
