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
# The generated C header, and the C++ one written over it by hand.
CAPI_HEADERS := tcslog-c/include/tcslog.h tcslog-c/include/tcslog.hpp
CAPI_LIBS   := libtcslog_c.a libtcslog_c.so

# The Python 3 interpreter, for the package's tests and its install,
# and the linter and type checker `make check` holds the package to.
# Neither tool ships with Python; see README.md for installing them.
PYTHON := python3
RUFF   := ruff
MYPY   := mypy
# Where `make install` puts the Python 3 package. Under the default
# PREFIX that is the user site directory, which Python searches without
# being told and which needs no pip -- an externally managed Python
# (PEP 668) refuses `pip install` outside a virtual environment, but
# not a package placed where it already looks. Under any other PREFIX
# it is that prefix's site-packages, which may need adding to
# PYTHONPATH; install says so when it does. Evaluated only when used,
# so a build without Python never runs it.
PYDIR ?= $(shell $(PYTHON) -c 'import os, site, sysconfig; \
	print(site.getusersitepackages() \
	if os.path.realpath("$(PREFIX)") \
	== os.path.realpath(os.path.expanduser("~")) \
	else sysconfig.get_path("purelib", "posix_prefix", \
	vars={"base": "$(PREFIX)", "platbase": "$(PREFIX)"}))')

# The C and C++ compilers for the binding's smoke test, and the warnings
# it is held to. -Werror because a warning in a 180-line test is a
# mistake in the test, not noise to scroll past. The test is one file in
# the common subset of the two languages; -x c++ is what makes a C++
# driver compile a file named .c as C++.
CC            := cc
CXX           := c++
CAPI_CFLAGS   := -Wall -Wextra -Werror -std=c11
CAPI_CXXFLAGS := -x c++ -Wall -Wextra -Werror -std=c++11
# The C++ interface's own test, held to -pedantic and built without
# exceptions or RTTI, which is what shows tcslog.hpp needs neither.
WRAPPER_CXXFLAGS := -Wall -Wextra -Werror -pedantic -std=c++11 \
	-fno-exceptions -fno-rtti
# What `make capi-memcheck` adds. -g so a report names lines rather
# than addresses, and no optimization so the frames are the real ones.
CAPI_SAN_FLAGS := -fsanitize=address,undefined -fno-omit-frame-pointer -g -O0
# Where cargo puts the staticlib the smoke test links. `make release`
# does not move it, because RELEASE reaches cargo and not this.
CARGO_TARGET_DIR ?= target/debug

# Default target
.PHONY: all
all: build		## Build the project (default target)

# Display help. Every entry is the `##` comment on the target's own
# rule, so the list cannot drift from the set of targets that exist.
.PHONY: help
help:			## Show this help
	@echo "Makefile for Rust Project with Claude Code"
	@echo ""
	@echo "Usage:"
	@grep -hE '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) \
		| sed 's/:.*## /|/' \
		| awk -F'|' '{printf "  make %-14s - %s\n", $$1, $$2}'

# Build the project. The subshell groups the whole build so that one
# `tee` captures all of it.
.PHONY: build
build:	docs		## Build the Rust project
	echo "Building the project..."
	cargo build $(RELEASE)
	echo "[OK] Build complete"

.PHONY: docs
docs:
	$(MAKE) -C docs

# Run tests
.PHONY: test
test:			## Run all tests
	@echo "Running tests..."
	cargo test
	$(MAKE) -C test
	$(MAKE) capi-test
	$(MAKE) python-test
	@echo "[OK] Tests complete"

# Build and run the C binding's smoke test: tcslog-c/examples/smoke.c
# compiled against the generated header and linked against the
# staticlib, which is what a C or C++ consumer does. The Rust unit tests
# in tcslog-c call the same functions as Rust, so they would pass a
# header that described the wrong argument order; only this catches
# that.
#
# It is compiled and run twice, as C and as C++: a header whose
# `extern "C"` block or enum declarations a C++ compiler mishandles is
# as broken for a C++ caller as a wrong one is for everyone.
#
# Each log is written into a temporary directory that is removed
# afterwards whether the test passed or not.
.PHONY: capi-test
capi-test:		## Run the C binding's smoke test as C and as C++
	@echo "Running the C binding smoke test..."
	cargo build $(RELEASE) -p tcslog-c
	tmp=$$(mktemp -d -t tcslog-capi.XXXXXXXXXX); \
	trap 'rm -rf "$$tmp"' EXIT; \
	mkdir $$tmp/c $$tmp/cxx; \
	$(CC) $(CAPI_CFLAGS) -o $$tmp/smoke-c tcslog-c/examples/smoke.c \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	$(CXX) $(CAPI_CXXFLAGS) -o $$tmp/smoke-cxx tcslog-c/examples/smoke.c \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	$(CXX) $(WRAPPER_CXXFLAGS) -o $$tmp/wrapper \
		tcslog-c/examples/wrapper.cpp \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	mkdir $$tmp/wrap; \
	echo "-- as C"; $$tmp/smoke-c $$tmp/c; \
	echo "-- as C++"; $$tmp/smoke-cxx $$tmp/cxx; \
	echo "-- through tcslog.hpp"; $$tmp/wrapper $$tmp/wrap
	@echo "[OK] C/C++ binding smoke test passed"

# Run the Python 3 interface's tests, which drive tcslog-py through the
# scenarios smoke.c drives the C/C++ binding through. TCSLOG_LIB names
# the library cargo just built, so an installed copy elsewhere on the
# loader's path cannot stand in for it.
.PHONY: python-test
python-test:		## Run the Python 3 interface's tests
	@echo "Running the Python 3 interface tests..."
	cargo build $(RELEASE) -p tcslog-c
	TCSLOG_LIB=$(abspath $(CARGO_TARGET_DIR))/libtcslog_c.so \
		PYTHONPATH=$(abspath tcslog-py) \
		$(PYTHON) -m unittest discover -s tcslog-py/tests
	@echo "[OK] Python 3 interface tests passed"

# Run the smoke test, as C and as C++, under the address and
# undefined-behaviour sanitizers, which is what checks the handles for
# leaks and double frees: they are boxed on the Rust side and released
# from C, and nothing else here would notice if a close leaked one.
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
capi-memcheck:		## Run the C/C++ smoke test under ASan and UBSan
	@echo "Running the C binding smoke test under sanitizers..."
	cargo build $(RELEASE) -p tcslog-c
	tmp=$$(mktemp -d -t tcslog-capi-san.XXXXXXXXXX); \
	trap 'rm -rf "$$tmp"' EXIT; \
	mkdir $$tmp/c $$tmp/cxx; \
	$(CC) $(CAPI_CFLAGS) $(CAPI_SAN_FLAGS) -o $$tmp/smoke-c \
		tcslog-c/examples/smoke.c \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	$(CXX) $(CAPI_CXXFLAGS) $(CAPI_SAN_FLAGS) -o $$tmp/smoke-cxx \
		tcslog-c/examples/smoke.c \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	$(CXX) $(WRAPPER_CXXFLAGS) $(CAPI_SAN_FLAGS) -o $$tmp/wrapper \
		tcslog-c/examples/wrapper.cpp \
		-I tcslog-c/include -L $(CARGO_TARGET_DIR) -l:libtcslog_c.a; \
	mkdir $$tmp/wrap; \
	export ASAN_OPTIONS=detect_leaks=1 UBSAN_OPTIONS=halt_on_error=1; \
	echo "-- as C"; $$tmp/smoke-c $$tmp/c; \
	echo "-- as C++"; $$tmp/smoke-cxx $$tmp/cxx; \
	echo "-- through tcslog.hpp"; $$tmp/wrapper $$tmp/wrap
	@echo "[OK] No leak or undefined behaviour reported"

# Regenerate tcslog-c/include/tcslog.h from tcslog-c/src/lib.rs. Needs
# `cargo install cbindgen`, as `make check` does. cbindgen indents only
# with spaces, so unexpand turns its eight-column indents into tabs.
.PHONY: header
header:			## Regenerate the C header from tcslog-c/src/lib.rs
	cd tcslog-c && cbindgen --config cbindgen.toml --crate tcslog-c \
		--output include/tcslog.h.tmp \
		&& unexpand --first-only -t 8 include/tcslog.h.tmp \
			> include/tcslog.h \
		&& rm include/tcslog.h.tmp
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
install: header		## Install the header, libraries, and Python 3 package
	@echo "Installing the C binding to $(DESTDIR)$(PREFIX)..."
	cargo build $(RELEASE) -p tcslog-c
	install -d $(INCDIR) $(LIBDIR)
	install -m 644 $(CAPI_HEADERS) $(INCDIR)/
	for lib in $(CAPI_LIBS); do \
		install -m 644 $(CARGO_TARGET_DIR)/$$lib $(LIBDIR)/; \
	done
	@echo "[OK] Installed $(CAPI_HEADERS) to $(INCDIR)/"
	@echo "[OK] Installed $(CAPI_LIBS) to $(LIBDIR)/"
	# The package, and beside it a link to the shared library just
	# installed, which is the first place the package looks after
	# TCSLOG_LIB -- so it finds the library with no LD_LIBRARY_PATH.
	# The link names the library's final path, not the DESTDIR one.
	install -d $(DESTDIR)$(PYDIR)/tcslog
	install -m 644 tcslog-py/tcslog/*.py tcslog-py/tcslog/py.typed \
		$(DESTDIR)$(PYDIR)/tcslog/
	ln -sf $(PREFIX)/lib/libtcslog_c.so \
		$(DESTDIR)$(PYDIR)/tcslog/libtcslog_c.so
	@echo "[OK] Installed the Python 3 package to" \
		"$(DESTDIR)$(PYDIR)/tcslog/"
	@# Staged with DESTDIR, the path is not this machine's to check.
	@[ -n "$(DESTDIR)" ] \
		|| $(PYTHON) -c 'import sys; sys.exit("$(PYDIR)" in sys.path)' \
		&& echo "     $(PYDIR) is not on Python's path;" \
			"add it to PYTHONPATH" \
		|| true
	@echo "     Compile against it with:"
	@echo "       cc  prog.c   -I$(PREFIX)/include" \
		"-L$(PREFIX)/lib -ltcslog_c"
	@echo "       c++ prog.cpp -I$(PREFIX)/include" \
		"-L$(PREFIX)/lib -ltcslog_c"
	@echo "     and from Python 3 with:"
	@echo "       import tcslog"

# Removes what install put there, honouring the same two variables.
# Nothing else in those directories is touched.
.PHONY: uninstall
uninstall:		## Remove the header, libraries, and Python 3 package
	@echo "Removing the C binding from $(DESTDIR)$(PREFIX)..."
	$(RM) $(INCDIR)/tcslog.h $(INCDIR)/tcslog.hpp
	$(RM) $(addprefix $(LIBDIR)/,$(CAPI_LIBS))
	$(RMDIR) $(DESTDIR)$(PYDIR)/tcslog
	@echo "[OK] Uninstalled from $(DESTDIR)$(PREFIX)"

# Clean build artifacts
.PHONY: clean
clean:			## Remove build artifacts
	@echo "Cleaning build artifacts..."
	-cargo clean
	$(RM) build.out
	$(RMDIR) tcslog-py/tcslog/__pycache__ tcslog-py/tests/__pycache__
	$(MAKE) -C docs clean
	@echo "[OK] Clean complete"

# Clean everything including generated source
.PHONY: distclean
distclean: clean	## Remove build artifacts and all generated files
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
check:			## Run cargo check, clippy, fmt, ruff, mypy, and more
	@echo "Running cargo check..."
	cargo check --all-targets
	# The read-only configuration, which nothing else here builds: the
	# scenario suite reaches it only through tcslog-tools, so a `write`
	# item exported without its cfg compiled fine until that suite ran.
	cargo check -p tcslog --no-default-features
	cargo clippy --all-targets -- -D warnings
	cargo fmt -- --check
	# The fuzz crate is outside the workspace, so the two lines above do
	# not reach it. Checked on stable; only running it needs nightly.
	cargo fmt --manifest-path fuzz/Cargo.toml -- --check
	cargo clippy --manifest-path fuzz/Cargo.toml --all-targets \
		-- -D warnings
	cargo rdme --check -w tcslog --intralinks-strip-links
	@echo "Checking the C header is current..."
	cd tcslog-c && cbindgen --config cbindgen.toml --crate tcslog-c \
		--output /dev/stdout 2>/dev/null \
		| unexpand --first-only -t 8 \
		| diff -u include/tcslog.h - \
		|| { echo "tcslog-c/include/tcslog.h is out of date;" \
			"run 'make header'" 1>&2; \
		     exit 1; }
	@echo "Checking the Python 3 package..."
	cd tcslog-py && $(RUFF) check . && $(RUFF) format --check . && $(MYPY) .
	@echo "Checking the Python package version matches the crates..."
	crate=$$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml); \
	py=$$(sed -n 's/^version = "\(.*\)"/\1/p' tcslog-py/pyproject.toml); \
	[ "$$crate" = "$$py" ] \
		|| { echo "tcslog-py/pyproject.toml is $$py," \
			"the crates $$crate" 1>&2; \
		     exit 1; }

# Fuzz the reader: each target in fuzz/ for FUZZ_TIME seconds, starting
# from and adding to the corpus under fuzz/corpus. Needs a nightly
# toolchain and `cargo install cargo-fuzz`. Not part of `make test`,
# being open-ended: a failure leaves the input that caused it under
# fuzz/artifacts, which `cargo +nightly fuzz run <target> <file>`
# replays. See docs/design.rst, "Fuzzing". FUZZ_FLAGS goes to
# `cargo fuzz run`: CI names the target there, since the prebuilt
# cargo-fuzz it installs is a musl binary and defaults to building for
# musl, where the sanitizer cannot run.
FUZZ_TIME    := 60
FUZZ_TARGETS := damaged_log arbitrary_files
FUZZ_FLAGS   :=
.PHONY: fuzz
fuzz:			## Fuzz each target for FUZZ_TIME seconds (nightly)
	cd fuzz && for t in $(FUZZ_TARGETS); do \
		cargo +nightly fuzz run $(FUZZ_FLAGS) $$t -- \
			-max_total_time=$(FUZZ_TIME) -max_len=8192 \
			|| exit 1; \
	done
	@echo "[OK] No fuzz target failed in $(FUZZ_TIME)s each"

# Type-check the whole workspace for a platform other than the one
# building it, which is what keeps a Unix-only construct from creeping
# back in: design.rst asks for code that works beyond Linux. Not part
# of `make check`, needing `rustup target add $(CROSS_TARGET)` first.
CROSS_TARGET := x86_64-pc-windows-gnu
.PHONY: cross-check
cross-check:		## Type-check the workspace for CROSS_TARGET (Windows)
	cargo check --workspace --all-targets --target $(CROSS_TARGET)
	@echo "[OK] The workspace type-checks for $(CROSS_TARGET)"

# Regenerate the library's README from tcslog/src/lib.rs. Needs
# `cargo install cargo-rdme`, as `make check` does.
.PHONY: readme
readme:			## Regenerate tcslog/README.md from the library docs
	cargo rdme -w tcslog --intralinks-strip-links
	@echo "[OK] tcslog/README.md regenerated"

# Format code
.PHONY: format
format:			## Format the code with cargo fmt and ruff
	cargo fmt
	cd tcslog-py && $(RUFF) format .

# Create release build. Reuses `build` so there is one build path.
.PHONY: release
release: test		## Build with optimizations into target/release
	@echo "Creating release build..."
	$(MAKE) build RELEASE=--release
	@echo "[OK] Release binaries are in target/release/"
