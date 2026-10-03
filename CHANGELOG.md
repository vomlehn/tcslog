# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog][kac], and the project follows
[Semantic Versioning][semver]. The version is also the version of the on-disk
segment file format: a build reads a file whose major version matches its own
and whose minor version is no greater.

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

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

[0.1.0]: https://github.com/vomlehn/tcslog/releases/tag/v0.1.0
