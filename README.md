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

Documentation
-------------
User documentation is maintained by hand at `docs/tcslog.rst` and rendered
to `docs/tcslog.html` by `make -C docs`. It is no longer generated from the
prompt; edit the `.rst` file directly.

The Claude Code prompt that generates the sources lives at
`docs/tcslog-prompt.rst` — do not confuse it with the user guide at
`docs/tcslog.rst`.

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
