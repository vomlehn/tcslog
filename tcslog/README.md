# tcslog

<!-- cargo-rdme start -->

Onboard logging of telemetry for vehicles that cannot send it home as
it is produced: spacecraft, autonomous underwater vehicles, buoys,
balloons, and anything else whose contact is intermittent, where
storage is budgeted long before launch.

A log is a directory of *segment files*, each at most
`seg_size_max` bytes, whose names are a caller-chosen prefix and
suffix around a `SegId`. Bounding the file size bounds the storage
a log occupies, which is what lets a mission commit storage to
telemetry with confidence; splitting the log into files is what lets
it be sent down in small batches, and what limits the damage a bad
sector can do.

Records are written with `LogWrite` and read with `LogRead`.
Neither allocates once it has been constructed, so both suit
embedded systems with a fixed memory budget. The two exceptions are
documented where they appear: `LogRead::iter`, which yields owned
`Record` values, and `LogRead::take_opened_headers`.

## Setup

Two things have to be in place before a log can be written. Neither
is needed to read one, so a program that only reads can take the
crate with `default-features = false` and skip this section.

**The real-time clock must hold the correct time.** A writer reads it
once, when it is constructed, pairs it with the monotonic clock, and
measures every segment identifier and record timestamp from that
pairing -- which is what keeps identifiers in creation order when NTP
or an operator steps the real-time clock backwards. Because it is
read once, a correction arriving later does not reach the identifiers
already minted, so a clock that does not read later than the UNIX
epoch, which is what an unset clock reads on most systems, is refused
with `LogError::ClockError` rather than used.

**`TIMER_RESOLUTION` must be set when building with the `write`
feature.** It is how finely this machine's clock advances, in
nanoseconds, and nothing guesses it: left unset the crate still
builds, but opening a log reports
`LogError::TimerResolutionZero`. Set it in `.cargo/config.toml`:

```toml
[env]
TIMER_RESOLUTION = "1"
```

or on the command line, as `TIMER_RESOLUTION=1 cargo build`.

It need not be exact. A writer that finds the value too small doubles
it, goes on doubling until a segment file name is free, and keeps
what it arrived at; the figure in force is reported back, and is the
one to build with next time. So starting at `1` and reading it back
is a fine way to find it.

Both requirements, and the reasoning behind them, are set out at
length in the user manual, `docs/tcslog.rst` in [the
repository](https://github.com/vomlehn/tcslog).

## Writing

```rust
use tcslog::{Format, LogWrite, WriteCallbacks, SEGMENT_FILE_HEADER_LEN};

let mut log = LogWrite::new(
    "/var/telemetry",                 // an existing directory
    "seg-",                           // file name prefix
    ".tcslog",                        // file name suffix
    SEGMENT_FILE_HEADER_LEN + 65_536, // bytes per segment file
    Format::VariableTsRc,
    WriteCallbacks::default(),
)?;

log.write_str("attitude nominal")?;
```

`WriteCallbacks` is where a filled segment file leaves this
library's care: `send` is called with its path, and must leave no
file of that name behind -- compress it, downlink it, or rename it
out of the log's naming pattern. The default `send` does nothing,
which suits development and lets segment files accumulate.

## Reading

Reading needs no timer resolution, so a program that only reads can
take the crate without its default features:

```toml
[dependencies]
tcslog = { version = "0.2", default-features = false }
```

```rust
use tcslog::{LogError, LogRead, Meta, RecSize};

let mut log = LogRead::new("/var/telemetry", "seg-", ".tcslog")?;
let mut buf = [0u8; 4096];

loop {
    match log.read(&mut buf) {
        Ok(result) => handle(&buf[..result.n as usize], result.meta),
        Err(LogError::Eof) => break,
        // Writing was interrupted here; record numbering restarts.
        Err(LogError::SessionEnd) => continue,
        // Telemetry was lost. `lost` files are missing, and the
        // first `n` bytes are real telemetry from a record that was
        // cut short.
        Err(LogError::ReadTruncated { lost, n }) => note_loss(lost, n),
        Err(e) => return Err(e),
    }
}
```

The rule is: read again until `LogError::Eof`. Every other outcome
is news about the telemetry, not a failure of the reader.

## Recovery

The reason for the segment header's `remaining` and `sequence`
fields is that stored telemetry gets damaged. A log is read back as
far as it survives: a reader that finds a segment file missing,
unreadable, or cut short discards the record that was in progress,
finds the next whole record start in the first file that opens
cleanly, and carries on with the records after it.

- Losses are reported rather than hidden, including how many files
  went missing, and including a loss falling exactly on a record
  boundary -- the case a naive reader misses because both sides look
  ordinary.
- Nothing is invented: bytes from either side of a gap are never
  spliced into a record that was never written.
- A record cut short is handed over, marked so it cannot be mistaken
  for a whole one.
- Each segment file carries its own identity, so a file renamed or
  copied out of its directory can still be identified and read.

<!-- cargo-rdme end -->

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
