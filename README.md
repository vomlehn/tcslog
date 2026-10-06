# tcslog
Small segmented telemetry log library for storage-bounded systems with error recovery

Summary
=======
Provides functions for reading and writing logs. Logs are segmented,
allowing for:

o   Piece by piece uploads

o   Recovery from errors

Building
--------
From the repository root:

```sh
make build
```

This compiles the library and builds the user documentation under `docs/`.

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

C interface
-----------
The code producing telemetry on a flight system is often C, and a library
that code cannot call is one that does not get used where the data is, so
`tcslog-c` presents `LogWrite` and `LogRead` to a C caller. It is a
separate crate because a C interface has to be built as a `cdylib` and a
`staticlib`, which a crate cannot be conditionally, and because the
`unsafe` an ABI needs is then confined to it: the library crate itself
contains none.

```sh
make install                       # header and libraries under $HOME
make install PREFIX=/usr/local     # somewhere else; DESTDIR stages it
cc prog.c -I$HOME/include -L$HOME/lib -ltcslog_c
```

`make uninstall` removes them again, honouring the same two variables.

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
./bin/run-capi-example
```

Builds `tcslog-c/examples/downlink.c` against the header, runs it
against a temporary directory, and shows what is left. The example
writes telemetry twice: once with a `send` callback that renames each
filled segment file out of the log's namespace, which is what a vehicle
does with telemetry it has downlinked, and once with no callback at all,
which is what a log looks like before a ground station comes into view.
It then reads back what remains.

`make capi-test` is the other half of that: it compiles
`tcslog-c/examples/smoke.c` against the header, links the staticlib, and
drives nine scenarios through the ABI -- a round trip, the fixed format,
a refused format tag, a record too large for the buffer, a segment file
deleted from the middle of the log, a `send` callback that refuses, a
cleared log, the resolution callback, and null arguments. `make test`
runs it, because it is the only test that would catch a header
describing the wrong argument order -- the Rust unit tests in `tcslog-c`
call the same functions as Rust.

`make capi-memcheck` runs the same program under the address and
undefined-behaviour sanitizers, which is what checks the handles for
leaks and double frees: they are boxed in Rust and released from C, and
nothing else here would notice a close that leaked one. It is not part
of `make test`, needing a compiler that has the sanitizers.

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

`docs/tcslog.rst` is the user guide: what the library does, how to call it,
and how it behaves. `make -C docs` renders it to `docs/tcslog.html`.

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
