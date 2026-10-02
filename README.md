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

Installs `tcslog-dump` into `$HOME/bin/`. Make sure `$HOME/bin` is on your
`PATH`.

Uninstalling
------------
```sh
make uninstall
```

Removes `tcslog-dump` from `$HOME/bin/`.

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
