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
After checking out, use:

    make install

User documentation in;

    docs/tcslog.rst

and

    docs/tcslog.html

Making Changes
--------------
In the future, it's possible that changes will be made by changing the Rust
file directly or by incremental AI work. For now, changes are made by
modifying the Claude Code prompt file in the docs directory, tcslog-prompt.rst.
To run claude to rebuild everything, start in the root directory and type:

    make distclean

to discard the previous Rust files in tcslog and the user documentation in
docs. Then generate new ones with:

    make generate

This will generate the tcslog subcrate and the user documentation.
