# tcslog

Onboard telemetry logging for vehicles that cannot send their telemetry home as
it is produced — spacecraft, autonomous underwater vehicles, buoys, balloons —
where storage is budgeted long before launch and contact is intermittent.

A log is a directory of segment files, each at most a size you choose. Records
are appended to the file being written; when it fills, the library hands it to
a callback of yours and opens the next one. That shape is what makes a log
downlinkable a piece at a time, and what bounds the damage any single bad
sector can do.

Every segment file but the one being written is *exactly* the chosen size, so a
log's footprint is the file count times that size. Nothing is padded and
nothing falls short. Once running, writing and reading a record allocate
nothing.

## Two things to set up first

**The real-time clock must hold the correct time before you open a log.** A
writer reads it once, pairs it with the monotonic clock, and measures every
segment identifier and record timestamp from that pairing — which is what keeps
identifiers in creation order when NTP or an operator steps the real-time clock
backwards. A correction arriving later cannot mend identifiers already minted.
`LogWrite::new` refuses a clock that does not read later than the UNIX epoch,
which is what an unset clock reads on most systems, and reports
`LogError::ClockError`.

**`TIMER_RESOLUTION` must be set when building the `write` feature.** It is how
finely your machine's clock advances, in nanoseconds, and nothing guesses it
for you. Leave it unset and the crate still builds, but `LogWrite::new` reports
`LogError::TimerResolutionZero` and opens no log. Set it in
`.cargo/config.toml`:

```toml
[env]
TIMER_RESOLUTION = "1"
```

or on the command line, as `TIMER_RESOLUTION=1 cargo build`.

It need not be exact. A writer that finds the value too small doubles it, goes
on doubling until a segment file name is free, and keeps what it arrived at.
`LogWrite::timer_resolution()` reports the figure in force, which is the one to
build with next time — so starting at `1` and reading it back is a fine way to
find it.

## Writing

```rust
use tcslog::{Format, LogWrite, WriteCallbacks};

let mut log = LogWrite::new(
    "/var/telemetry",   // an existing directory
    "seg-",             // file name prefix
    ".tcslog",          // file name suffix
    65_536,             // bytes per segment file, header included
    Format::VariableTsRc,
    WriteCallbacks::default(),
)?;

log.write_str("attitude nominal")?;
```

`WriteCallbacks` is where a filled segment file leaves the library's care:
`send` is called with its path, and must leave no file of that name behind —
compress it, downlink it, or rename it out of the log's naming pattern. The
default `send` does nothing, which suits development and lets segment files
accumulate.

## Reading

Reading needs no timer resolution, so a read-only program can take the crate
without its default features:

```toml
[dependencies]
tcslog = { version = "0.2", default-features = false }
```

```rust
use tcslog::{LogError, LogRead};

let mut log = LogRead::new("/var/telemetry", "seg-", ".tcslog")?;
let mut buf = [0u8; 4096];

loop {
    match log.read(&mut buf) {
        Ok(result) => handle(&buf[..result.n as usize], result.meta),
        Err(LogError::Eof) => break,
        // Writing was interrupted here; record numbering restarts.
        Err(LogError::SessionEnd) => continue,
        // Telemetry was lost. `lost` files are missing, and the first
        // `n` bytes are real telemetry from a record cut short.
        Err(LogError::ReadTruncated { lost, n }) => note_loss(lost, n),
        Err(e) => return Err(e.into()),
    }
}
```

The rule is: read again until `Eof`. Every other outcome is news about the
telemetry, not a failure of the reader.

## Recovery

A log is read back as far as it survives. On a segment file that is missing,
unreadable, or cut short, the reader abandons the record it was in, finds the
next whole record start in the first file that opens cleanly, and carries on.

- Losses are reported rather than hidden, including how many files went
  missing, and including a loss falling exactly on a record boundary — the case
  a naive reader misses because both sides look ordinary.
- Nothing is invented: bytes from either side of a gap are never stitched into
  one record.
- A record cut short is handed over, marked so it cannot be mistaken for a
  whole one.
- Each segment file carries its own identity, so a file renamed or copied out
  of its directory can still be identified and read.

## Record formats

| `Format` | Per-record overhead | What it stores |
| --- | --- | --- |
| `Fixed(n)` | none | payloads of exactly `n` bytes |
| `VariableSimple` | 4 bytes | a length |
| `VariableTsRc` | 20 bytes | a length, a timestamp, a record number |

## Tools

[`tcslog-tools`](https://github.com/vomlehn/tcslog-tools) provides `tcslog-dump`,
which prints a log, and `tcslog-dumphdr`, which prints one segment file's
header without consulting its name — for files renamed or copied out of their
directory.

## Documentation

The full manual, including the theory of operation, is `docs/tcslog.rst` in
[the repository](https://github.com/vomlehn/tcslog).

## License

Licensed under either of Apache License, Version 2.0 or the MIT license, at
your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
