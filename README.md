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

This compiles the library and binaries and builds the user documentation
under `docs/`.

Installing
----------
```sh
make install
```

Installs two binaries — `tcslog-sample` and `tcslog-dump` — into
`$HOME/bin/`. Make sure `$HOME/bin` is on your `PATH`.

Uninstalling
------------
```sh
make uninstall
```

Removes `tcslog-sample` and `tcslog-dump` from `$HOME/bin/`.

Documentation
-------------
User documentation is generated to:

```
docs/tcslog.rst
docs/tcslog.html
```

The Claude Code prompt that generates the sources and user docs lives at
`docs/tcslog-prompt.rst` — do not confuse it with the generated user guide
at `docs/tcslog.rst`.

Making Changes
--------------
In the future, it's possible that changes will be made by changing the Rust
file directly or by incremental AI work. For now, changes are made by
modifying the Claude Code prompt file in the docs directory, tcslog-prompt.rst.
To run claude to rebuild everything, start in the root directory and type:

```sh
make distclean
```

to discard the previous Rust files in tcslog and the user documentation in
docs. Then generate new ones with:

```sh
make generate
```

This will generate the tcslog subcrate and the user documentation.
