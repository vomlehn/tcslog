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

Documentation
-------------
`tcslog/README.md` is not written by hand. It is generated from the
library's own documentation in `tcslog/src/lib.rs`, so the two cannot
drift: `make readme` regenerates it and `make check` fails if it is out of
date. Edit the doc comment, not the README. Its last few sections, from
"Record formats" on, are outside the generated region and are hand-written.
That needs a tool `cargo` does not ship:

```sh
cargo install cargo-rdme
```

There are also two documents under `docs/`, and they are easy to confuse.

`docs/tcslog.rst` is the user guide: what the library does, how to call it,
and how it behaves. `make -C docs` renders it to `docs/tcslog.html`.

`docs/tcslog-prompt.rst` is the specification. The sources were originally
generated from it as a Claude Code prompt, but it no longer generates
anything; it is read rather than run. Both documents are now maintained by
hand, and a change in behaviour belongs in both.

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
