# tcslog
Flight software: a small segmented telemetry log library for storage-bounded systems with error recovery

Summary
=======
Provides functions for reading and writing logs. Logs are segmented,
allowing for:

o   Piece by piece uploads

o   Recovery from errors

Languages
---------
Tcslog supports **Rust**, **C/C++**, and **Python 3**, with the same
operations under the same names in all three:

o   Rust: the `tcslog` crate.

o   C/C++: `tcslog-c`, a C ABI whose header compiles, and is tested, as
    both C and C++. See *C/C++ interface* below.

o   Python 3: `tcslog-py`, a pure-Python package over `ctypes`. See
    *Python 3 interface* below.

A log written from any of them reads in the other two.

Integration with cFS and F´
---------------------------
Work is under way on using Tcslog with NASA's core Flight System (cFS), as a
cFS library whose filled segments go down through CFDP, and with JPL's F´
(F Prime), as a component whose segments go down through FileDownlink. The
plan, shared with tcspecial and tcs-xtce, is in tcspecial's
[`docs/third-party.rst`](https://github.com/vomlehn/tcspecial/blob/main/docs/third-party.rst).

> **Note:** integration with cFS and F´ is a work in progress. None of it is
> built yet, and the plan may change as it is.

Memory
------
Writing a record and reading a record allocate nothing, in any of the
three languages; records are read into a buffer the caller owns. The
few places that do allocate -- opening a log, clearing one, the Rust
iterator, and, in Python, the objects Python makes for every value --
are listed, with why each is unavoidable, in `docs/manual.rst`.

Building
--------
From the repository root:

```sh
make build
```

This compiles the library and builds the user documentation under `docs/`.

Installing
----------
Everything installs under `/opt/tcs`, the tree the TCS packages share
(FHS 3.0's `/opt/<package>`): `tcslog-gen`, which writes test logs, in
`bin/`; `tcslog-links`, which links it into `/usr/local/bin`, in `sbin/`;
the C and C++ headers, `tcslog.h` and `tcslog.hpp`, in `include/`;
`libtcslog.so.1` (with `libtcslog.so` linked to it) and `libtcslog.a`
in `lib/`; a pkg-config file in `lib/pkgconfig/`; the Python 3 package
in `lib/python3/site-packages/`; and the documentation and examples
under `share/`.

As a package, built from a checkout. Installing and packaging use
[tcs-helpers](https://github.com/vomlehn/tcs-helpers), the packaging scripts
and make rules the TCS projects share, from a checkout of it beside this
one, so clone it there first; it is used where it is, never installed, and
`TCS_HELPERS=<path>` names a checkout elsewhere. `make tcs-helpers` checks
that it is there and new enough, which installing and packaging do first.

```sh
git clone https://github.com/vomlehn/tcs-helpers.git ../tcs-helpers
make deb                # build/packages/tcslog_<version>-1_<arch>.deb
make rpm                # build/packages/tcslog-<version>-1.<arch>.rpm
make packages           # both
make packages STRIP=0   # both, unstripped
sudo apt install ./build/packages/tcslog_*.deb
```

Both are made from one install into `build/stage/tcslog` (`make stage`),
so they hold the same files. That install strips what it installs: the
program and shared library with `strip --strip-unneeded`, the static
library with `strip --strip-debug`, which keeps the symbols a program
links against. `STRIP=0` keeps everything as cargo built it, for a
debugger: about 25 MB installed rather than 16. The `-1` is the package's
release, `DEB_REVISION` for the .deb, for a package rebuilt from the same
source. Building them needs, besides cargo and cbindgen, `python3-pip`,
`python3-setuptools`, `python3-docutils` (for `rst2html`) and
`dpkg-dev`. `make rpm` needs `rpmbuild` (`sudo apt install rpm` on
Debian) and says so, without failing, where it is missing. A package
also installs three files outside `/opt/tcs`, each named after it:
`/etc/ld.so.conf.d/tcslog.conf`, so the loader finds the library;
`/etc/profile.d/tcslog.sh`, which adds `/opt/tcs/bin` to `PATH` and
`/opt/tcs/lib/pkgconfig` to `PKG_CONFIG_PATH`; and a `tcslog.pth` in the
system Python's own directory (`/usr/lib/python3/dist-packages` on
Debian, `/usr/lib/python3.X/site-packages` for the Python the RPM was
built with), so `import tcslog` works.

The .deb also carries what Debian expects of every package, outside
`/opt/tcs` as the hooks are: `/usr/share/doc/tcslog/copyright`, in
Debian's machine-readable format, and `changelog.Debian.gz`, both made
as the package is built from `LICENSE-MIT` and the version; lintian's
overrides in `/usr/share/lintian/overrides/tcslog`; and a trigger that
has dpkg run `ldconfig` once the library is installed or removed. The
RPM says the same in its own `License`, `%changelog`, `%post` and
`%postun`. `make lint-packages` checks the packages that `make packages`
built with lintian and rpmlint, and their shell scripts with shellcheck.
It skips, in a line, any of the three that is not installed, and fails
on any finding that is not overridden; each override says why it is
intended, in `packaging/deb/tcslog/lintian-overrides` and
`packaging/rpm/rpmlint.toml`.

Login shells find `/opt/tcs/bin` through `/etc/profile.d/tcslog.sh`, but
sudo, cron and other minimal environments do not read it, and do not
find the programs. To make them available everywhere, link them into
`/usr/local/bin`, which is on every one of those paths, sudo's
`secure_path` included:

```sh
sudo /opt/tcs/sbin/tcslog-links            # link them
sudo /opt/tcs/sbin/tcslog-links --remove   # undo it
```

`--dir` names another directory, such as `/opt/bin`. It links only this
package's programs, leaves anything already in the way alone and says
so, and is never run for you: removing the package removes its links,
but installing it makes none. After `make install`, `sudo make links` and
`sudo make unlinks` do the same, with `LINKDIR` naming the directory.

Or straight from a checkout:

```sh
sudo make install                   # under /opt/tcs, nothing in /etc
sudo make install HOOKS=1           # with the three files above
make install PREFIX=$HOME/tcs       # somewhere of your own
```

`DESTDIR` stages the install, and `make uninstall` removes it again,
honouring the same variables. `make install` strips as the packages do,
and `STRIP=0` keeps it from doing so. `HOOKS=1` needs the default `PREFIX`,
since the files it installs name `/opt/tcs`. Without the hooks, point
the tools at the tree yourself:

```sh
export PKG_CONFIG_PATH=/opt/tcs/lib/pkgconfig
export LD_LIBRARY_PATH=/opt/tcs/lib
export PYTHONPATH=/opt/tcs/lib/python3/site-packages
```

The build bakes in the `TIMER_RESOLUTION` of the machine that runs it
(see `docs/manual.rst`, "The timer resolution"); a library built without
one refuses to open a log for writing.

From C or C++, through pkg-config:

```c
#include "tcslog.h"             /* or "tcslog.hpp" from C++ */
```

```sh
cc  prog.c   $(pkg-config --cflags --libs tcslog)
c++ prog.cpp $(pkg-config --cflags --libs tcslog)
```

From Python 3, the package finds the library installed beside it with
nothing set. In a virtual environment, pip installs the package alone,
which then finds the library as *Python 3 interface* below describes:

```sh
pip install ./tcslog-py
```

From Rust, the library is the `tcslog` crate, taken from crates.io or as
a path dependency, and `tcslog-gen` installs with cargo:

```sh
cargo install --path tcslog-gen --root /opt/tcs
```

Trying it out
-------------
```sh
./bin/run-sample --verbose
```

Writes a sample log into a temporary directory, lists the segment files, and
removes the directory again. The example's source is
`tcslog/examples/sample.rs`.

Tools
-----
`tcslog-dump` and `tcslog-dumphdr` are the
[tcslog-tools](https://github.com/vomlehn/tcslog-tools) crate, in its own
repository, because they only read a log and so need neither the `write`
feature nor the build-time timer resolution it requires:

```sh
cargo install tcslog-tools
```

The error-recovery suite under `test/` drives both, so running `make test`
needs that checkout. `bin/tcslog-tool` finds it, expecting
`../tcslog-tools` beside this repository; set `TCSLOG_TOOLS` to look
elsewhere.

`make test` answers whether a scenario still produces what it produced
before, which is not the same as whether what it produces is right.
`bin/verify-helper` is for the second question: name any of the scenarios
under `test/` and it shows each one's segment files as the reader will
find them, then the records the reader made of them, pausing after each
until ENTER.

```sh
./bin/verify-helper combined_12-10 corrupt_12-10
```

`make fuzz` goes past the scenarios anyone thought of: two cargo-fuzz
targets under `fuzz/` damage real logs and build logs from arbitrary
bytes, and require that the reader never panics, always reaches the end,
and -- when the damage only removes bytes -- never returns a record that
was not written. It needs a nightly toolchain and `cargo install
cargo-fuzz`; `FUZZ_TIME` sets the seconds per target, 60 by default, and
CI runs it on every push.

C/C++ interface
---------------
The code producing telemetry on a flight system is often C or C++, and a
library that code cannot call is one that does not get used where the
data is, so `tcslog-c` presents `LogWrite` and `LogRead` to a C or C++
caller. The header wraps its declarations in `extern "C"` for C++, so
the same header and libraries serve both. C++ has `tcslog.hpp` besides:
`tcslog::LogWrite`, `tcslog::LogRead` and `tcslog::WriteCallbacks`,
named as in Rust, closing their handles themselves, and needing no
exceptions, RTTI, or heap. `make capi-test` runs its test,
`tcslog-c/examples/wrapper.cpp`. It is a
separate crate because a C interface has to be built as a `cdylib` and a
`staticlib`, which a crate cannot be conditionally, and because the
`unsafe` an ABI needs is then confined to it: the library crate itself
contains none.

`make install` installs the headers and both libraries, as `libtcslog`;
see *Installing* above. In this repository the libraries cargo builds
keep the crate's name, `libtcslog_c`, which is what the tests and
examples here link.

The `1` in `libtcslog.so.1` is the ABI's version, not the package's. It
is the whole number in `tcslog-c/abi-version`, from which
`tcslog-c/build.rs` sets the soname and the Makefile the installed
names, and it is bumped only when the ABI changes incompatibly: when a
function's signature or behaviour changes incompatibly, or one is
removed, or a struct or enum the header exposes changes its layout or
values. A program built against one ABI version is then never started
against another. Adding a function, or a status after the last, does
not bump it.

`tcslog-c/include/tcslog.h` is generated from `tcslog-c/src/lib.rs` by
`cbindgen` and is not written by hand: `make header` regenerates it and
`make check` fails if it is out of date, the same arrangement
`tcslog/README.md` has with the library's doc comment. That needs a tool
`cargo` does not ship:

```sh
cargo install cbindgen
```

Every function returns a `TcslogStatus` and writes what the caller wanted
through an out-parameter, so no status can be confused with data. The
status numbers are ABI, and `docs/design.rst` specifies them along
with the rest of the interface.

A writer is given its callbacks when it is opened, in a
`TcslogCallbacks` that also carries the `void *ctx` handed back to each
of them, so two logs in one process can have different callbacks and
different contexts. On the Rust side that is the `WriteCallbacks` trait,
which a caller implements on whatever its callbacks need to reach.

```sh
./bin/run-capi-example          # as C
./bin/run-capi-example --cxx    # the same file as C++
```

Builds `tcslog-c/examples/downlink.c` against the header, runs it
against a temporary directory, and shows what is left. The example
writes telemetry twice: once with a `send` callback that renames each
filled segment file out of the log's namespace, which is what a vehicle
does with telemetry it has downlinked, and once with no callback at all,
which is what a log looks like before a ground station comes into view.
It then reads back what remains.

`make capi-test` is the other half of that: it compiles
`tcslog-c/examples/smoke.c` against the header, once as C and once as
C++, links the staticlib, and drives ten scenarios through the ABI -- a
round trip, the fixed format, a refused format tag, a record too large
for the buffer, a segment file deleted from the middle of the log, a
`send` callback that refuses, a cleared log, the resolution callback,
two writers with two contexts, and null arguments. `make test`
runs it, because it is the only test that would catch a header
describing the wrong argument order -- the Rust unit tests in `tcslog-c`
call the same functions as Rust.

`make capi-memcheck` runs the same program, as C and as C++, under the address and
undefined-behaviour sanitizers, which is what checks the handles for
leaks and double frees: they are boxed in Rust and released from C, and
nothing else here would notice a close that leaked one. It is not part
of `make test`, needing a compiler that has the sanitizers.

Python 3 interface
------------------
`tcslog-py` is the `tcslog` Python 3 package: `LogWrite`, `LogRead`,
`WriteCallbacks` and the rest, named as in Rust, with each `LogError`
variant an exception class. It is pure Python over `ctypes`, calling the
C ABI's shared library, so it needs no compiler and nothing outside the
standard library:

`make install` installs it along with the C/C++ binding, into
`$(PREFIX)/lib/python3/site-packages`, a path with no Python minor
version in it, using `pip --target` on a copy of `tcslog-py`; `PYDIR`
overrides where it goes. Python searches there once told to, by
`PYTHONPATH` or by the `.pth` file a package installs. In a virtual
environment, `pip install ./tcslog-py` works too.

The package finds the library through `TCSLOG_LIB`, if set, then beside
the package, then as `libtcslog.so.1` in the `lib/` of the tree it was
installed into, then on the loader's path (`LD_LIBRARY_PATH`), then in
this repository's `target/` directory.

```sh
./bin/run-python-example
```

Runs `tcslog-py/examples/downlink.py`, the Python counterpart of
`downlink.c`. `make python-test` runs the package's tests, which drive
it through the scenarios `smoke.c` drives the C/C++ binding through;
`make test` includes them.

The package is fully typed and ships `py.typed`, so a type checker reads
its annotations. `make check` holds it to `ruff check`, `ruff format`,
and `mypy --strict`, as it holds the crates to clippy and rustfmt. Neither
tool ships with Python; on a system whose Python refuses `pip install`,
put them in a virtual environment and point `make` at them:

```sh
python3 -m venv ~/.venvs/tcslog-lint
~/.venvs/tcslog-lint/bin/pip install ruff mypy
make check RUFF=~/.venvs/tcslog-lint/bin/ruff MYPY=~/.venvs/tcslog-lint/bin/mypy
```

Documentation
-------------
`tcslog/README.md` is not written by hand. It is generated from the
library's own documentation in `tcslog/src/lib.rs`, so the two cannot
drift: `make readme` regenerates it and `make check` fails if it is out of
date. Edit the doc comment, not the README. The whole of it is generated,
so what the README says is also what docs.rs shows. That needs a tool
`cargo` does not ship:

```sh
cargo install cargo-rdme
```

There are also two documents under `docs/`, and they are easy to confuse.

`docs/manual.rst` is the user guide: what the library does, how to call it,
and how it behaves. `make -C docs` renders it to `docs/manual.html`.

`docs/design.rst` is the specification. The sources were originally
generated from it as a Claude Code prompt, which is where its old name,
`docs/tcslog-prompt.rst`, came from, but it no longer generates anything;
it is read rather than run. Both documents are now maintained by hand, and
a change in behaviour belongs in both.

`docs/tcslog-prompt.rst` is a deprecated stub that records the rename and
nothing else.

Making Changes
--------------
Changes were originally made from a single "master" Claude prompt. They
are now made directly, whether by hand or by using Claude.

License
-------
Licensed under either of

o   Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
    <http://www.apache.org/licenses/LICENSE-2.0>)

o   MIT license ([LICENSE-MIT](LICENSE-MIT) or
    <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
