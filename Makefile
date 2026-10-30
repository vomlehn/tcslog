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

# The version, from the workspace's Cargo.toml, which every crate here
# and the packages take.
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
# The C ABI's version, which is not the package's: a whole number in
# tcslog-c/abi-version, from which tcslog-c/build.rs sets the shared
# library's soname, so that the soname and the installed names cannot
# disagree. Bump it only when the ABI changes incompatibly -- a
# function's signature or behaviour changed incompatibly or a function
# removed, or a struct or enum the header exposes changed in layout or
# values -- as build.rs says at more length.
ABI     := $(strip $(file < tcslog-c/abi-version))
SONAME  := libtcslog.so.$(ABI)

# Where `make install` puts everything: /opt/tcs, the tree the TCS
# packages share under FHS 3.0's /opt/<package>, so that one PATH entry
# and one loader directory serve them all. DESTDIR stages the install
# for packaging. A developer installing somewhere of their own sets
# PREFIX, such as PREFIX=$HOME.
PREFIX  ?= /opt/tcs
DESTDIR ?=
# Whether install also writes the hooks outside $(PREFIX) that make an
# installed /opt/tcs usable without being told where it is: the loader's
# directory in /etc/ld.so.conf.d, PATH and PKG_CONFIG_PATH in
# /etc/profile.d, and Python's path in a .pth file. Off unless asked
# for, so that an install touches nothing in /etc or /usr that its
# installer did not mean it to; `make stage` asks, the packages being
# where these belong. Each is named after this package, so no two TCS
# packages own the same file.
HOOKS   ?= 0
# Whether install strips what it installs; see install below.
STRIP   ?= 1
BINDIR  := $(DESTDIR)$(PREFIX)/bin
SBINDIR := $(DESTDIR)$(PREFIX)/sbin
INCDIR  := $(DESTDIR)$(PREFIX)/include
LIBDIR  := $(DESTDIR)$(PREFIX)/lib
PCDIR   := $(LIBDIR)/pkgconfig
DOCDIR  := $(DESTDIR)$(PREFIX)/share/doc/tcslog
EXDIR   := $(DESTDIR)$(PREFIX)/share/examples/tcslog
# What install puts there: the generated header and the C++ one written
# over it by hand, both libraries, so a consumer can link either
# statically or dynamically, and the documents a reader of the installed
# tree needs. Cargo names the libraries after the crate, libtcslog_c;
# they are installed as libtcslog, the name tcslog-c/build.rs gives the
# shared one's soname.
CAPI_HEADERS := tcslog-c/include/tcslog.h tcslog-c/include/tcslog.hpp
# The programs a person runs, in bin. They are also what tcslog-links
# links into /usr/local/bin, written into it as it is installed, so that
# it never touches another TCS package's programs in the same bin.
PROGRAMS := tcslog-gen
DOCS     := README.md LICENSE-APACHE LICENSE-MIT CHANGELOG.md \
	docs/manual.html docs/design.html
EXAMPLES := tcslog-c/examples/downlink.c tcslog-c/examples/wrapper.cpp \
	tcslog-py/examples/downlink.py tcslog/examples/sample.rs
# The hooks HOOKS=1 installs name /opt/tcs, so they are refused with any
# other PREFIX rather than installed pointing at the wrong tree.
ifeq ($(HOOKS),1)
ifneq ($(PREFIX),/opt/tcs)
$(error HOOKS=1 installs files that name /opt/tcs; give PREFIX=/opt/tcs \
	or leave HOOKS unset)
endif
endif

# The Python 3 interpreter, for the package's tests and its install,
# and the linter and type checker `make check` holds the package to.
# Neither tool ships with Python; see README.md for installing them.
PYTHON := python3
RUFF   := ruff
MYPY   := mypy
# Where `make install` puts the Python 3 package: the same directory
# for every Python 3, so the path has no minor version in it to go
# stale when Python is upgraded. Python does not search it unless told
# to, by PYTHONPATH or by the .pth file HOOKS=1 installs.
PYDIR ?= $(PREFIX)/lib/python3/site-packages

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
# cbindgen copies the doc comments as rustdoc wrote them, so
# rustdoc_to_doxygen.py rewrites their Arguments and Returns sections as
# the @param and @return commands a C reader's tools know.
.PHONY: header
header:			## Regenerate the C header from tcslog-c/src/lib.rs
	cd tcslog-c && cbindgen --config cbindgen.toml --crate tcslog-c \
		--output include/tcslog.h.tmp \
		&& unexpand --first-only -t 8 include/tcslog.h.tmp \
		| $(PYTHON) -I rustdoc_to_doxygen.py > include/tcslog.h \
		&& rm include/tcslog.h.tmp
	@echo "[OK] tcslog-c/include/tcslog.h regenerated"

# The hooks HOOKS=1 installs, outside $(PREFIX). The .pth file goes
# where Debian's python3 looks; `make rpm` moves it to where the RPM's
# Python looks.
LDCONF  := $(DESTDIR)/etc/ld.so.conf.d/tcslog.conf
PROFILE := $(DESTDIR)/etc/profile.d/tcslog.sh
PTH     := $(DESTDIR)/usr/lib/python3/dist-packages/tcslog.pth

# Install everything under $(PREFIX), built for release: tcslog-gen;
# the headers in include; the shared library under its soname, with its
# development name linked to it, and the static one; a pkg-config file
# naming the real prefix; the Python 3 package; and the documents and
# examples. The Rust library is not installed, a Rust program taking it
# through cargo.
#
# Under STRIP=1, the default, what is installed is stripped as it is
# installed: the program and the shared library of every symbol nothing
# links against, and the static library of its debugging information
# alone, since a program linking it still needs its symbols. That takes
# the packages, which are cut from an install, from tens of megabytes to
# a few. STRIP=0 keeps everything as cargo built it, for a debugger.
#
# The Python package goes in with pip, so that it has the metadata pip
# and importlib.metadata read, but from a copy of tcslog-py made for
# the purpose: pip builds in the directory it is given, and a build left
# in the source tree by `sudo make install` would be root's. --no-index
# keeps it off the network, and --target puts it in $(PYDIR) whatever
# Python the build machine has. The direct_url.json pip writes would name
# that copy, which is gone by then, so it is removed.
.PHONY: install
install: header docs	## Build, strip (STRIP=0: don't), install under PREFIX
	@echo "Installing tcslog $(VERSION) to $(DESTDIR)$(PREFIX)..."
	cargo build --release -p tcslog-c -p tcslog-gen
	install -D -m 755 -t $(BINDIR) $(addprefix target/release/,$(PROGRAMS))
	install -d $(SBINDIR)
	sed -e 's|@PACKAGE@|tcslog|g' -e 's|@PREFIX@|$(PREFIX)|g' \
		-e 's|@PROGRAMS@|$(PROGRAMS)|g' packaging/hooks/links.sh.in \
		> $(SBINDIR)/tcslog-links
	chmod 755 $(SBINDIR)/tcslog-links
	install -D -m 644 -t $(INCDIR) $(CAPI_HEADERS)
	install -D -m 644 target/release/libtcslog_c.so $(LIBDIR)/$(SONAME)
	ln -sf $(SONAME) $(LIBDIR)/libtcslog.so
	install -D -m 644 target/release/libtcslog_c.a $(LIBDIR)/libtcslog.a
	if [ "$(STRIP)" = 1 ]; then \
		strip --strip-unneeded $(addprefix $(BINDIR)/,$(PROGRAMS)) \
			$(LIBDIR)/$(SONAME); \
		strip --strip-debug $(LIBDIR)/libtcslog.a; \
	fi
	install -d $(PCDIR)
	sed -e 's|@PREFIX@|$(PREFIX)|' -e 's|@VERSION@|$(VERSION)|' \
		packaging/tcslog.pc.in > $(PCDIR)/tcslog.pc
	chmod 644 $(PCDIR)/tcslog.pc
	tmp=$$(mktemp -d -t tcslog-py.XXXXXXXXXX); \
	trap 'rm -rf "$$tmp"' EXIT; \
	cp -R tcslog-py/pyproject.toml tcslog-py/tcslog $$tmp/; \
	$(RMDIR) $$tmp/tcslog/__pycache__; \
	$(PYTHON) -m pip install --quiet --no-deps --no-build-isolation \
		--no-index --no-cache-dir --no-compile --upgrade \
		--root-user-action=ignore \
		--target $(DESTDIR)$(PYDIR) $$tmp
	dist=$(DESTDIR)$(PYDIR)/tcslog-$(VERSION).dist-info; \
	$(RM) $$dist/direct_url.json; \
	sed -i '/^tcslog-[^/]*\.dist-info\/direct_url\.json,/d' $$dist/RECORD
	install -D -m 644 -t $(DOCDIR) $(DOCS)
	install -D -m 644 -t $(EXDIR) $(EXAMPLES)
	if [ "$(HOOKS)" = 1 ]; then \
		install -D -m 644 packaging/hooks/tcslog.conf $(LDCONF); \
		install -D -m 644 packaging/hooks/tcslog.sh $(PROFILE); \
		install -D -m 644 packaging/hooks/tcslog.pth $(PTH); \
	fi
	@echo "[OK] Installed under $(DESTDIR)$(PREFIX)"
	@# Staged with DESTDIR, the path is not this machine's to check.
	@if [ -z "$(DESTDIR)" ] && ! $(PYTHON) -c \
		'import sys; sys.exit("$(PYDIR)" not in sys.path)'; then \
		echo "     $(PYDIR) is not on Python's path;" \
			"add it to PYTHONPATH"; \
	fi
	@echo "     Compile against it with:"
	@echo "       cc  prog.c   \$$(pkg-config --cflags --libs tcslog)"
	@echo "       c++ prog.cpp \$$(pkg-config --cflags --libs tcslog)"
	@echo "     with PKG_CONFIG_PATH=$(PREFIX)/lib/pkgconfig, and from"
	@echo "     Python 3 with:"
	@echo "       import tcslog"

# Removes what install put there, honouring the same variables, HOOKS
# included. Nothing else in those directories is touched.
#
# The links `make links` made in $(LINKDIR) go first, while the script
# that knows which they are is still there, so that nothing is left
# pointing at programs that are gone. A failure there, such as a
# LINKDIR that does not exist, stops nothing.
.PHONY: uninstall
uninstall:		## Remove what install put under PREFIX
	@echo "Removing tcslog from $(DESTDIR)$(PREFIX)..."
	-$(SBINDIR)/tcslog-links --remove --dir $(LINKDIR)
	$(RM) $(addprefix $(BINDIR)/,$(PROGRAMS)) $(SBINDIR)/tcslog-links
	$(RM) $(INCDIR)/tcslog.h $(INCDIR)/tcslog.hpp
	$(RM) $(LIBDIR)/$(SONAME) $(LIBDIR)/libtcslog.so \
		$(LIBDIR)/libtcslog.a $(PCDIR)/tcslog.pc
	$(RMDIR) $(DESTDIR)$(PYDIR)/tcslog \
		$(DESTDIR)$(PYDIR)/tcslog-$(VERSION).dist-info
	$(RMDIR) $(DOCDIR) $(EXDIR)
	if [ "$(HOOKS)" = 1 ]; then \
		$(RM) $(LDCONF) $(PROFILE) $(PTH); \
	fi
	@echo "[OK] Uninstalled from $(DESTDIR)$(PREFIX)"

# Link the installed programs into $(LINKDIR), or take those links away
# again, with the tcslog-links install put in sbin: what an administrator
# does to make them available beyond login shells, to sudo and cron
# among others. Needs root for /usr/local/bin, being root's; LINKDIR
# names another directory, such as /opt/bin.
LINKDIR ?= /usr/local/bin
.PHONY: links
links:			## Link the installed programs into LINKDIR
	$(DESTDIR)$(PREFIX)/sbin/tcslog-links --dir $(LINKDIR)

.PHONY: unlinks
unlinks:		## Remove the links `make links` made in LINKDIR
	$(DESTDIR)$(PREFIX)/sbin/tcslog-links --remove --dir $(LINKDIR)

# Where the packages are made from and where they land, both under
# build/, which .gitignore keeps out of the repository. The stage is one
# directory per package, as tcspecial's is, though this repository has
# only the one package.
STAGE   := $(CURDIR)/build/stage
PKGSTAGE := $(STAGE)/tcslog
PKGOUT  := $(CURDIR)/build/packages

# Install into $(PKGSTAGE) as the package installs onto a machine: under
# /opt/tcs, with the hooks, and stripped unless STRIP=0 says otherwise,
# which the sub-make is passed as every variable on the command line is.
# The stage is emptied first, so that nothing a previous install left
# behind gets into a package. Files are then made writable by their
# owner alone, whatever the umask of whoever ran this, which is what
# both package formats expect.
.PHONY: stage
stage:			## Install into build/stage/tcslog for packaging
	$(RMDIR) $(STAGE)
	$(MAKE) install DESTDIR=$(PKGSTAGE) PREFIX=/opt/tcs HOOKS=1
	chmod -R u+rwX,go=rX $(PKGSTAGE)
	@echo "[OK] Staged under $(PKGSTAGE)"

# Build the .deb from the stage, with the control files in
# packaging/deb/tcslog. dpkg-shlibdeps works out what the shared library
# and tcslog-gen need from the libraries they link, but wants a
# debian/control to read, so it is given a minimal one in a directory
# of its own. The shlibs file tells dpkg-shlibdeps, building a package
# that links libtcslog, to depend on this one at this version or later.
#
# DEB_REVISION is the Debian revision, for a package rebuilt from the
# same source, as in tcspecial.
DEB_DIR := packaging/deb/tcslog
DEB_REVISION ?= 1
DEB_VERSION  := $(VERSION)-$(DEB_REVISION)
.PHONY: deb
deb: stage		## Build the .deb in build/packages
	root=$(CURDIR)/build/deb/tcslog; \
	arch=$$(dpkg --print-architecture); \
	$(RMDIR) $$root; \
	mkdir -p $(CURDIR)/build/deb $(PKGOUT); \
	cp -a $(PKGSTAGE) $$root; \
	install -d -m 755 $$root/DEBIAN; \
	shl=$(CURDIR)/build/deb/shlibdeps; \
	$(RMDIR) $$shl; \
	mkdir -p $$shl/debian; \
	printf 'Source: tcslog\n\nPackage: tcslog\nArchitecture: any\n' \
		> $$shl/debian/control; \
	depends=$$(cd $$shl && dpkg-shlibdeps -O -l$$root/opt/tcs/lib \
		-e$$root/opt/tcs/lib/$(SONAME) \
		-e$$root/opt/tcs/bin/tcslog-gen 2>/dev/null \
		| sed -n 's/^shlibs:Depends=//p'); \
	size=$$(du -sk $$root | cut -f1); \
	sed -e 's|@VERSION@|$(DEB_VERSION)|' -e "s|@ARCH@|$$arch|" \
		-e "s|@SIZE@|$$size|" -e "s|@DEPENDS@|$$depends|" \
		$(DEB_DIR)/control > $$root/DEBIAN/control; \
	install -m 644 $(DEB_DIR)/conffiles $$root/DEBIAN/; \
	install -m 755 $(DEB_DIR)/postinst $(DEB_DIR)/prerm $(DEB_DIR)/postrm \
		$$root/DEBIAN/; \
	echo "libtcslog $(ABI) tcslog (>= $(VERSION))" > $$root/DEBIAN/shlibs; \
	chmod 644 $$root/DEBIAN/shlibs; \
	dpkg-deb --root-owner-group --build $$root \
		$(PKGOUT)/tcslog_$(DEB_VERSION)_$$arch.deb

# Build the .rpm from the same stage, with packaging/rpm/tcslog.spec,
# whose %install copies the stage rather than building anything. A
# machine without rpmbuild says so and carries on, so that `make
# packages` still makes the .deb; that is checked before staging, so as
# not to build for nothing. rpmbuild is pointed at a database of its own
# under build/, because a Debian host has no RPM database in /var/lib/rpm
# to read, and rpmbuild complains at length when it finds none.
RPM_TOP := $(CURDIR)/build/rpm
.PHONY: rpm
rpm:			## Build the .rpm in build/packages
	@if ! command -v rpmbuild > /dev/null; then \
		echo "rpmbuild is not installed, so no RPM was built;" \
			"install it with 'sudo apt install rpm'"; \
		exit 0; \
	fi; \
	$(MAKE) stage; \
	$(RMDIR) $(RPM_TOP); \
	mkdir -p $(PKGOUT) $(RPM_TOP)/db; \
	rpmbuild -bb --quiet \
		--define "_topdir $(RPM_TOP)" \
		--define "_dbpath $(RPM_TOP)/db" \
		--define "tcs_version $(VERSION)" \
		--define "tcs_abi $(ABI)" \
		--define "tcs_stage $(PKGSTAGE)" \
		--define "tcs_pysite $$($(PYTHON) -c 'import sysconfig; \
			print(sysconfig.get_path("purelib", "posix_prefix", \
			vars={"base": "/usr"}))')" \
		--define "tcs_pyversion $$($(PYTHON) -c 'import sysconfig; \
			print(sysconfig.get_python_version())')" \
		packaging/rpm/tcslog.spec; \
	cp $(RPM_TOP)/RPMS/*/tcslog-$(VERSION)-*.rpm $(PKGOUT)/; \
	echo "[OK] Built the RPM in $(PKGOUT)"

.PHONY: packages
packages: deb rpm	## Build both; STRIP=0 leaves them unstripped

# Clean build artifacts
.PHONY: clean
clean:			## Remove build artifacts
	@echo "Cleaning build artifacts..."
	-cargo clean
	$(RM) build.out
	$(RMDIR) build
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
		| $(PYTHON) -I rustdoc_to_doxygen.py \
		| diff -u include/tcslog.h - \
		|| { echo "tcslog-c/include/tcslog.h is out of date;" \
			"run 'make header'" 1>&2; \
		     exit 1; }
	@echo "Checking the Python 3 package..."
	cd tcslog-py && $(RUFF) check . && $(RUFF) format --check . && $(MYPY) .
	@echo "Checking the Python package version matches the crates..."
	py=$$(sed -n 's/^version = "\(.*\)"/\1/p' tcslog-py/pyproject.toml); \
	[ "$(VERSION)" = "$$py" ] \
		|| { echo "tcslog-py/pyproject.toml is $$py," \
			"the crates $(VERSION)" 1>&2; \
		     exit 1; }
	@echo "Checking the Python package loads the installed soname..."
	grep -q '^_INSTALLED_NAME = "$(SONAME)"$$' \
		tcslog-py/tcslog/__init__.py \
		|| { echo "tcslog-py/tcslog/__init__.py: _INSTALLED_NAME" \
			"is not \"$(SONAME)\"" 1>&2; \
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
