# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog][kac], and the project follows
[Semantic Versioning][semver].

The version here is the crate's. The on-disk segment file format is versioned
separately, by `VERSION_MAJOR`, `VERSION_MINOR` and `VERSION_PATCH`: a build
reads a file whose major version matches its own and whose minor version is no
greater. The two started out the same number and need not stay that way — a
release that changes the API without touching what is written leaves the stored
format where it was. Each entry below says whether the stored format moved.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## [Unreleased]

Stored format: unchanged, 0.1.0.

### Added

- `test/tsrc-lost-header_8-24` and an `api.rs` test cover the one gap whose
  arithmetic places the record continuing out of it, in the format that
  cannot use it. A single deleted segment file holding one whole data header
  and no payload byte of its record is what `combined_12-10` recovers from in
  `variable-simple`; in `variable-tsrc` the lost header also held a timestamp
  and a record count, so the record has to be given up rather than handed back
  with metadata that was never written. Nothing covered that refusal before:
  the two `api.rs` recovery tests were `variable-simple` only, and the suite's
  one `variable-tsrc` deletion scenario loses a session's first file and a
  two-file span, neither of which reaches the check. Removing the format guard
  in `recovered_payload_len` now fails both.
- `bin/verify-helper` walks a person through named scenarios, stopping after
  each until ENTER. Each is run with `-x -q`, so the output is the segment
  files as the reader will find them — after the deletions, corruptions and
  truncations — followed by the records alone, as ASCII. `make test` answers
  whether a scenario still produces what it produced before; this is for
  whether what it produces is right.
- `error-recovery-common` takes `-q`, which drops `--verbose` from the
  `tcslog-dump` run so the comparison is the records alone. What is captured is
  what is compared, so the flag chooses the form of the stored file too: the
  expected file becomes `<name>-records.expected`. Both forms are kept, 22 of
  each, so either runs without regenerating the other.
- `test/Makefile` takes `VERBOSE_EXPECTED`, passed through to every scenario.
  Empty, which is the default, compares the verbose capture as before;
  `make test VERBOSE_EXPECTED=-q` compares the records alone. The log is
  generated and damaged identically either way, and generation output still
  reaches the terminal.

  Each records-only file was confirmed to be exactly the record lines of its
  verbose counterpart, so the leaner comparison drops the header blocks,
  notices and totals and nothing else.

### Changed

- The suite stores and compares records as ASCII rather than as hexadecimal.
  A failing diff of `#1 123` against `#1 124` says what went wrong, where
  `23 31 20 31 32 33` against `23 31 20 31 32 34` leaves the reader to decode
  it. `--text` is now what `error-recovery-common` passes by default, and `-H`
  is what asks for hexadecimal, which inverts what `-T` used to mean.
- `text-combined_12-10` becomes `hex-combined_12-10`, since the variant worth
  keeping is now the one the rest of the suite does not use. Both of the
  tool's renderings are still covered end to end; which one is the exception
  is all that moved.

### Fixed

- `-r` and `-x` aborted the run on any scenario that corrupts a header. The
  hexdump passes each file to `tcslog-dumphdr`, which exits non-zero when it
  will not read a header, and under `set -e` that took the whole script down
  before the comparison it exists for — so the two options were unusable on
  exactly the scenarios they are most wanted for. The refusal is now reported
  and the hexdump still runs, that being what shows the damage.
- The hexdump after deletion ran the segment files together: each one's name
  and decoded header followed the last one's hexdump with nothing between, so
  the start of one read as the tail of the one above. A blank line ahead of
  each separates them, as the dump before deletion already did. Only what
  reaches the terminal under `-r` and `-x` changes; the compared output comes
  from a separate `tcslog-dump` run, so no expected file moves.

### Notes

- `docs/tcslog-prompt.rst` documents the whole of the lost-header recovery,
  not just the arithmetic. "Find the Next Data Record Start" now names the two
  outcomes it can report, a fresh start and a recovered one; "Segment Boundary
  Validation" and the read procedure say what a rejected crossing has to leave
  behind and what the recovered record is read with; and "Recovering a Data
  Record Whose Header Was Lost" gains the hint's lifetime, the bookkeeping that
  lets a recovered record span further segment files, and a worked example of
  `combined_12-10`, where record 2's length field was deleted with sequence 1
  and the reader produces the record whole regardless. Behavior is unchanged;
  the specification was describing only part of what the reader does.

## [0.2.4] - 2026-10-03

Stored format: unchanged, 0.1.0.

### Changed

- `error-recovery-common` takes `-T`, which passes `--text` to `tcslog-dump`,
  and `test/text-combined_12-10` is the same log and the same damage as
  `combined_12-10` read that way. The suite had driven only the default
  rendering, so one of the tool's two was covered end to end and the other by
  unit tests alone. Comparing the two expected files is itself the check that
  the flag changes the rendering and nothing else: 117 lines identical, 8
  payload lines differing, and each of those eight confirmed to be the same
  bytes shown the other way.

  The escapes stay unit-tested only. The generator emits printable ASCII, so
  no scenario in the suite produces a byte that `--text` would have to escape.

- The error-recovery suite's 20 expected-output files are regenerated for
  `tcslog-tools`, which now prints payloads as hexadecimal rather than as one
  character a byte. Nothing in this crate changed; the suite drives those tools
  and compares their output, so a change in how they render reaches it. Every
  payload line was confirmed to decode back to exactly what it had shown, and
  no structural line — segment header, loss notice, or total — differs.

## [0.2.3] - 2026-10-03

Stored format: unchanged, 0.1.0.

### Changed

- `tcslog/README.md` is now generated from the library's own documentation by
  [`cargo-rdme`](https://crates.io/crates/cargo-rdme), so the two cannot drift
  apart the way they had. `make readme` regenerates it; `make check` fails when
  it is out of date. Everything from "Record formats" on sits outside the
  generated region and is still hand-written, that being the crates.io
  furniture rather than the library's documentation.
- The library's documentation absorbed the readme's Writing and Reading
  sections, which were better than the single combined example it had: the
  `send` contract, the read loop with a line on what each outcome asks of a
  caller, and the rule the outcomes add up to. Generating the readme from a
  thinner source would have cost the crates.io page that material, so it moved
  rather than being dropped. Both examples compile as doctests.

## [0.2.2] - 2026-10-03

Stored format: unchanged, 0.1.0.

### Fixed

- The crate described itself three ways. The description, the readme and the
  library's own documentation each named a different set of vehicles —
  `buoys` and `balloons` appeared in two of the three — so the library's
  opening paragraph now matches the other two. Its Setup section also says
  where the long version of the same material lives, the user manual, so a
  reader wanting more need not go looking and an editor can see that this is
  not the only copy.

## [0.2.1] - 2026-10-03

Stored format: unchanged, 0.1.0.

### Added

- The library's own documentation gains a Setup section, which is what a reader
  arriving from docs.rs sees. `TIMER_RESOLUTION` and the real-time clock
  requirement were documented on `LogWrite::new` and on the errors they
  produce, but nowhere on the landing page, so the first thing a new caller has
  to do was the one thing the front page did not mention.

### Fixed

- `README.md` said `docs/tcslog-prompt.rst` is "the Claude Code prompt that
  generates the sources", in the present tense, two paragraphs above a section
  saying changes are now made directly. The prompt no longer generates
  anything. The Documentation section now tells the two documents under `docs/`
  apart — the user guide and the specification — and says that both are
  maintained by hand, so a change in behaviour belongs in both. This is the
  workspace README, not the one published with the crate.

## [0.2.0] - 2026-10-03

A segment file's identifier is the time it was created, and a reader replays
segment files in identifier order while checking them by the dense sequence
number in each header. Those two agreed only as long as the clock the
identifiers came from rose, and the real-time clock does not: NTP, an operator,
or a time fix from the ground can step it backwards. This release takes the
identifiers off that clock, and makes the one build-time figure the library
asks for forgiving of a wrong answer.

Stored format: unchanged, 0.1.0. Files written before and after this release
are read by either.

### Added

- `LogWrite::timer_resolution()`, which reports how finely a writer believes
  its clock advances. Above the value the build supplied, it says the supplied
  one was too small for this machine and names the figure to build with next.
- `WriteCallbacks::timer_resolution_adjusted`, invoked with the new value each
  time the resolution is widened. It returns nothing and the writer carries on
  regardless, so the choice of what a widening means stays with the caller: a
  deployed system will usually record it and keep logging, since the log is
  unharmed, while a system under development may prefer to stop where the
  faulty value was found.
- `LogWrite::new` now refuses a real-time clock that does not read later than
  the UNIX epoch, which is what an unset clock reads on most systems, reporting
  `ClockError` without creating a log. The clock is read once, so it must hold
  the correct time before any Tcslog function is called; a correction arriving
  later cannot mend identifiers already minted.
- A writer seeds its clock past the highest identifier already in its
  directory, taken from the same scan that hands pre-existing files to `send`,
  so a backward step of the real-time clock between two writers cannot put the
  later one behind the earlier one's files.

### Changed

- Segment identifiers and record timestamps now come from the real-time clock's
  epoch advanced by the monotonic clock, paired once when the writer is
  constructed. An identifier is still nanoseconds since the UNIX epoch; what
  orders it is a clock that cannot step.
- `TIMER_RESOLUTION` is no longer required at build time. Left unset it is
  zero, the crate builds, and `LogWrite::new` refuses to open a log and reports
  `TimerResolutionZero`. The crate has to build without one: a dependent taking
  tcslog from a registry has no `.cargo/config.toml` of this repository's, so a
  build that stopped would leave it with a panic inside somebody else's build
  script, and no documentation on docs.rs either. A value that is not a
  nanosecond count is still a build failure — not setting the variable says
  nobody said, while setting it to nonsense says somebody said something wrong.
  An empty or blank setting counts as unset. Reading a log needs no resolution
  at all.
- When one is supplied it is now a starting point rather than a figure that has
  to be right. One collision while naming a segment file is ordinary; a second
  says the value is too small, so the writer doubles it, doubling again for each
  collision after that, and keeps what it arrived at. The true figure is
  otherwise hard to come by: nothing in the standard library reports it, and
  what `thread::sleep` waits for a given duration is bounded only loosely.
- `TimerResolutionZero`'s message now says what to do about it, that error
  having gone from something only a deliberate zero could produce to what every
  dependent sees before supplying a value.
- `WriteCallbacks` has a third member, which breaks a literal that names every
  field. Naming only the fields that matter and taking the rest from
  `..WriteCallbacks::default()` keeps a literal working when a callback is
  added, and is now what the documentation recommends.
- `WriteCallbacks::default()` no longer does nothing in every case: it reports a
  widened resolution on standard error, silence being the wrong default for a
  figure that is not otherwise visible.
- `ClockError` now reports an unset real-time clock rather than one set before
  the epoch, and is raised only by `LogWrite::new`. `write` can no longer return
  it, the clock a record is stamped from having been validated at construction.

### Fixed

- A complete, undamaged log could read back as a damaged one. If the real-time
  clock was stepped backwards while a writer was running, a segment file created
  afterwards took an identifier below one created before it; the reader then
  replayed the files in an order its sequence checks disagreed with and reported
  segment files lost, with the records out of order. No data was ever missing,
  and a genuine fault is announced the same way, so the two could not be told
  apart.

### Notes

- `docs/tcslog.rst` gains "The real-time clock must be set" under Prerequisites,
  and "The clock a writer keeps" and "Learning the timer resolution" under
  Theory of Operation. "What a caller should do with each outcome" is now a
  top-level section, and closes with the rule its six entries add up to: read
  again until `Eof`.
- `base-prompt` is removed. `docs/tcslog-prompt.rst` is no longer used to
  generate code, so the instruction it held had nothing left to drive.

## [0.1.0] - 2026-10-02

First release of tcslog, a Rust library for storing telemetry on board a
vehicle that cannot send it home as it is produced — spacecraft, autonomous
underwater vehicles, buoys, balloons — where storage is budgeted long before
launch and contact is intermittent.

A log is a directory of segment files, each at most a caller-chosen size.
Records are appended to the file being written; when it fills, the library
hands it to a caller-supplied function and opens the next one. That shape is
what makes a log downlinkable a piece at a time, and what bounds the damage any
single bad sector can do.

Stored format: 0.1.0, the first.

### Added

- `LogWrite` and `LogRead`, the writing and reading halves of a log, and
  `WriteCallbacks`, whose two hooks decide how hard the library works to reach
  stable storage: one runs after every record, one when a segment file fills.
- Three record formats. `Format::Fixed` stores payload bytes with no data
  header at all; `VariableSimple` adds a length; `VariableTsRc` stamps every
  record with a timestamp and its position in the run. Metadata costs only the
  logs that ask for it.
- Storage that is known in advance: every segment file but the one being
  written is exactly `seg_size_max` bytes, so a log's footprint is the file
  count times that size. Nothing is padded and nothing falls short.
- Allocation-free operation once running. Writing and reading a record allocate
  nothing, and records are read into a caller-owned buffer. The three
  exceptions are operations a caller asks for explicitly and are documented as
  such: directory enumeration, `LogRead::iter`, and
  `LogRead::take_opened_headers`.
- `tcslog-gen`, which writes generated records to give tests segment files to
  damage.
- `tcslog-dump`, which reads a log and prints it, and `tcslog-dumphdr`, which
  prints one segment file's header and never consults the file's name, for
  files renamed or copied out of their directory. Both are the `tcslog-tools`
  crate, in its own repository, since they only read and so need neither the
  `write` feature nor the timer resolution it requires:
  <https://github.com/vomlehn/tcslog-tools>

### Error recovery

- Recovery is automatic. On a file that is missing, unreadable, or cut short,
  the reader abandons the record it was in, finds the next whole record start in
  the first file that opens cleanly, and carries on. Only running out of files
  ends a read.
- Losses are reported rather than hidden, including how many files went missing,
  and including losses falling exactly on a record boundary — the case a naive
  reader misses because both sides look ordinary.
- Nothing is invented. Bytes from either side of a gap are never stitched into a
  record; where the survivors cannot be shown to be one record, it is reported
  lost.
- Partial records are handed over rather than discarded, marked so that a record
  cut short cannot be mistaken for a whole one.
- Each segment file carries its own identity, so a renamed or copied file can
  still be identified and read.

### Notes

- A runnable sample lives in `tcslog/examples/sample.rs`; `bin/run-sample` runs
  it against a temporary directory.
- User documentation is `docs/tcslog.rst`.
- Requires Rust 1.75 or later. Dual licensed under MIT OR Apache-2.0.

[unreleased]: https://github.com/vomlehn/tcslog/compare/v0.2.4...HEAD
[0.2.4]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.4
[0.2.3]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.3
[0.2.2]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.2
[0.2.1]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.1
[0.2.0]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.0
[0.1.0]: https://github.com/vomlehn/tcslog/releases/tag/v0.1.0
