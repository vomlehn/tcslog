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

## [0.2.8] - 2026-10-04

Stored format: unchanged, 0.1.0.

### Deprecated

- `WriteCallbacks`, in favour of `WriteHandler`, with removal in 0.3.0.
  The two do the same job and the trait does it better: it can carry a
  context, it is static dispatch the compiler can inline where a `fn`
  pointer is an opaque indirect call, and a stateless handler is a unit
  struct costing nothing where `WriteCallbacks` is three pointers stored
  in every writer. Keeping both would mean two ways to say one thing, in
  the API, the manual and the specification.

  Nothing breaks yet: the type still works, still implements the trait,
  and is still the default type parameter. In 0.3.0 it goes and the
  default becomes `()`. `tcslog-gen` and `examples/sample.rs` are
  migrated here rather than silenced, and both read better for it --
  their three free functions and a `const` become one unit struct with
  two methods.

### Added

- `WriteHandler`, the callbacks as a trait, for a caller whose callbacks
  must reach its own state. `WriteCallbacks` holds bare `fn` pointers with
  nowhere to keep any, so a context had to be a static -- as the suite's
  own callback test shows, counting in `AtomicUsize`es because the tests
  run in parallel and one test's counters would otherwise reach another's.
  An implementation keeps that state in `self` instead, and the writer
  owns it and lends it back through `LogWrite::handler` and
  `handler_mut`.

  Every method has a default, so an implementation names only the
  callbacks it wants, and each is called exactly where the corresponding
  field of `WriteCallbacks` is called. `LogWrite` is generic over the
  handler with `WriteCallbacks` as its default type, and `WriteCallbacks`
  implements the trait by calling its own fields: existing code passing
  `WriteCallbacks`, and existing code naming `LogWrite` without a
  parameter, are unaffected -- all 108 tests passed unchanged across the
  change.

  `&mut H` implements the trait for any handler `H`, so a handler can be
  lent to the writer rather than given up and read once the log is
  closed. That is what to reach for when the state matters afterwards:
  the writer's own drop hands the last segment file to `send`, so a count
  inside a handler the writer owns cannot be read after that, where one
  the caller still holds can. The alternative would have been to return
  the handler from the writer, which needs `unsafe` to move a field out
  of a type with a destructor -- this library has none.

  `send` is the one method with no default, and `()` implements the trait
  with a `send` that does nothing. A log that never sends fills its
  directory, and the bound on storage is the whole reason the log is
  segmented, so not sending is now something a caller writes down
  (`LogWrite::new(.., ())`) rather than something `..Default::default()`
  does quietly.

- `tcslog-c`, a C ABI over `LogWrite` and `LogRead`, as a third
  workspace member. A C interface has to be built as a `cdylib` and a
  `staticlib`, which a crate cannot be conditionally, so it could not have
  been a feature of the library; keeping it a separate crate also confines
  the `unsafe` an ABI needs to it, leaving `tcslog/src` with none. It stays
  in this repository rather than one of its own, as `tcslog-gen` does,
  because the binding mirrors the Rust API and a change to both is then one
  commit and one `make check`.

  A writer is given its callbacks when it is opened, in a
  `TcslogCallbacks` holding the three function pointers and the `void
  *ctx` each is handed back, so two logs in one process can have different
  callbacks and different contexts. That is `WriteHandler` underneath,
  which the binding implements once and aims at whichever C functions a
  writer was opened with. The structure is copied, so it need not outlive
  the call; the `ctx` is the caller's and must outlive the writer. There
  are no process-wide callback setters: the context they lacked is the
  only reason they would have existed.

  Every function returns a `TcslogStatus` and writes what the caller wanted
  through an out-parameter, so no status can be confused with data.
  `ReadTruncated` and `ReadOverflow` carry their counts into a
  `TcslogReadResult`. No function lets a panic unwind into C, that being
  undefined behaviour; each catches one and reports `TCSLOG_STATUS_PANIC`.
  The status numbers are ABI and `docs/tcslog-prompt.rst` now specifies
  them, along with the calling convention, the format tags, what a read
  fills in, and the callbacks.

- `docs/tcslog.rst` gains a "Calling From C" chapter: installing, the
  shape the interface takes and why every function returns a status, the
  write and read functions with what each out-parameter holds, the
  callbacks and what their return values mean, the panic rule, and a
  complete program. The library's own documentation gains a shorter
  "Calling from C" section, so docs.rs and crates.io say the binding
  exists rather than leaving it to be found in the repository.

- `tcslog-c/include/tcslog.h`, generated from the crate by `cbindgen`.
  `make header` writes it and `make check` fails if it is out of date, the
  arrangement `tcslog/README.md` already has with the library's doc
  comment.

- `make install` puts the header and both libraries under `PREFIX`,
  defaulting to `$HOME` as the `tcslog-tools` Makefile does, with `DESTDIR`
  for staged installs; `make uninstall` removes them. Neither the Rust
  library nor a pkg-config file is installed: a consumer needs
  `-ltcslog_c` and nothing a pkg-config file would add.

- `tcslog-c/examples/downlink.c` and `bin/run-capi-example`, the C
  counterpart of `examples/sample.rs` and `bin/run-sample`. The example
  writes telemetry with a `send` callback that renames each filled segment
  file out of the log's namespace, which is what the callback's contract
  asks of it, then again with no callback, which is what a log looks like
  before a ground station comes into view, and reads back what remains.

- `make capi-test`, run by `make test`, compiles
  `tcslog-c/examples/smoke.c` against the generated header, links the
  staticlib, and drives nine scenarios through the ABI under `-Wall
  -Wextra -Werror`: a round trip, the fixed format and the two lengths it
  refuses, a format tag outside the three, a record too large for the
  buffer, a segment file deleted from the middle of the log, a `send`
  callback that refuses a file, a cleared log, the resolution callback,
  and the arguments that must be refused rather than dereferenced. Each
  works in its own directory, so one cannot leave state another depends
  on. The Rust unit tests in `tcslog-c` call the same functions as Rust,
  so only this would catch a header that described the wrong argument
  order or struct layout.

- `make check` builds the library with `--no-default-features`, the
  read-only configuration nothing else here builds. An item of the
  `write` module re-exported without its `cfg` compiled fine until the
  scenario suite built `tcslog-tools` against it, which is a slow way to
  find a one-line mistake; this finds it in the check that is meant to.

- `make capi-memcheck` runs that program under the address and
  undefined-behaviour sanitizers, which is what checks the handles for
  leaks and double frees: they are boxed in Rust and released from C, and
  nothing else would notice a close that leaked one. The staticlib is
  built without sanitizers, which is enough -- LeakSanitizer intercepts
  the process allocator and Rust's default allocator here is the system
  one. Confirmed to have teeth by leaking a writer on purpose, which it
  reports. Not part of `make test`, needing a compiler that has them.

### Fixed

- The C binding documented `ReadOverflow` backwards, in all four places it
  described it: the status, the result field, the specification, and the
  user manual. It said the result held the size the record needs with
  nothing placed in the buffer, and that the next read returned the same
  record. The library does the opposite, and says so -- the front of the
  record reaches the buffer and is real telemetry, the rest is skipped,
  and the next read starts at the record after it. Writing a test for the
  case is what found it: the test asserted the invented behaviour and
  failed. The manual now says the name invites the wrong reading, since it
  invited this one.

- `LogWrite`'s `Drop` flushes the segment file being written and hands it
  to `send` if it holds any records, which the C binding's
  `tcslog_write_close` documentation had said it did not do. Writing the
  example is what found it: every segment file left the log, including the
  short last one, and the read that followed found an empty directory. The
  binding now documents what happens, including that an error from that
  flush or from `send` is discarded because a close cannot report one.

## [0.2.7] - 2026-10-04

Stored format: unchanged, 0.1.0.

### Changed

- The keyword `datalogger` becomes `logging`. The manifest had argued the
  other way, that `logging` is a crowded search term and says less; the
  comment saying so is gone rather than left to contradict the value beneath
  it. Keywords are published metadata, so crates.io shows the old set until
  the next release.

## [0.2.6] - 2026-10-04

Stored format: unchanged, 0.1.0.

### Added

- `bin/make-releases` creates a GitHub release for each tag, with the notes
  for each taken from `docs/release-notes/<version>.md`. A tag alone is enough
  for this file's footer links, which resolve whether or not a release exists;
  the releases exist so that page carries the version's notes rather than just
  its commit. The notes are kept as files rather than extracted from here
  because a release whose crate did not change says so in its notes and
  nowhere in this file.

### Changed

- The library's documentation now opens with the user manual's introduction
  rather than a condensation of it, so docs.rs and crates.io say what
  `docs/tcslog.rst` says: what the library is for, the five advantages in
  normal operation, and the six when things go wrong. The old `Recovery`
  section is gone, having been a second, shorter telling of the last of
  those. The manual's bare literals became intra-doc links, so the docs.rs
  page is navigable, and its `Record Formats` cross-reference became a link
  to this page's own section, which resolves on both docs.rs and crates.io.
- `tcslog/README.md` is now generated in its entirety. The four sections from
  "Record formats" on were hand-written below the generated region, which left
  them out of the library's own documentation and so off docs.rs; they are now
  part of the doc comment. What the README says and what docs.rs shows can no
  longer differ, and `make check` fails if they do.

### Fixed

- The manual's storage claim held only for a log written in one session. It
  said that every segment file but the last one being written is exactly the
  maximum size and that nothing is left short, where the file that ends each
  session is short as well -- in a log of several sessions those sit in the
  middle of it, not at the end. Generating two sessions of five files gives
  sizes 63 63 63 63 61 63 63 63 63 61, which is what the claim denied. The
  entry now names both short cases, keeps the point that nothing is padded,
  which the writer does guarantee, and states the file count times the maximum
  size as an upper bound, which it is in every case.

## [0.2.5] - 2026-10-04

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

[0.2.8]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.8
[0.2.7]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.7
[0.2.6]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.6
[0.2.5]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.5
[0.2.4]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.4
[0.2.3]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.3
[0.2.2]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.2
[0.2.1]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.1
[0.2.0]: https://github.com/vomlehn/tcslog/releases/tag/v0.2.0
[0.1.0]: https://github.com/vomlehn/tcslog/releases/tag/v0.1.0
