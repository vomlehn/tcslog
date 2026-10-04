=========================
Tcslog User Documentation
=========================

.. contents:: Table of Contents
   :depth: 3
   :local:

Introduction
============

Tcslog stores telemetry on board a vehicle that cannot send it home as it
is produced. Spacecraft, autonomous underwater vehicles, buoys, and
balloons all spend long stretches out of contact, and all of them have a
storage budget fixed long before launch. Tcslog is built for that
situation: it keeps telemetry in a form that can be handed down in small
pieces whenever a link appears, and it keeps it in a form that still
yields most of its contents after part of the storage has gone bad.

A Tcslog log is a directory of *segment files*. Each one is at most a
size the caller chooses, and its name is a caller-chosen prefix and
suffix wrapped around an identifier. Records are appended to the file
being written; when it is full, Tcslog hands it to a function the caller
supplied and opens the next one.

Advantages in normal operation
------------------------------

**The storage a log occupies is known in advance.** The writer fills a
segment file to exactly the maximum and continues the record in the next
one, so nothing is padded and no file is left short of its own accord.
Two kinds are short because the writer stopped rather than because it
rolled: the file currently being written, and the file that ended each
earlier session. Every other file is exactly the maximum size, so the
number of files times that size is the space a log occupies, and is an
upper bound on it in every case. A mission can commit a storage budget
to telemetry and know it will be honoured.

**Data leaves in small pieces.** Telemetry is handed over a file at a
time, as each one fills, rather than as one growing file that must be
sent whole. A pass over a ground station that is too short for the whole
log is still long enough for some of it, and what has already gone down
is a set of files that can simply be skipped.

**No allocation once running.** Writing a record and reading a record
allocate nothing, so a Tcslog log can be written and read on a system
with a fixed memory budget and no heap to grow. Records are read into a
buffer the caller owns, and the internal buffers a segment file name and
path need are reserved once and reused, so even rolling from one segment
file to the next allocates nothing.

The three exceptions are all operations a caller asks for explicitly, and
each says so where it is described: enumerating the directory, which
``LogRead::new``, ``LogWrite::new``, and ``LogWrite::clear`` must do and
which no platform offers without allocating; ``LogRead::iter``, whose
records own their payloads; and ``LogRead::take_opened_headers``.

**Metadata only where it is wanted.** Three record layouts are offered,
from one that stores payload bytes and nothing else to one that stamps
every record with the time it was written and its position in the run.
The cost of the metadata is paid only by the logs that use it. See
`Record Formats`_.

**The caller decides what durability costs.** Two callbacks decide how
hard Tcslog works to get bytes onto stable storage: one runs after every
record, and one runs when a segment file fills. A log that must survive
an unplanned reset can flush at every record; one that must keep up with
a fast sensor need not.

Advantages when things go wrong
-------------------------------

Stored telemetry gets damaged. A sector goes bad, a file is lost in a
reset part way through a write, a copy off the vehicle drops a file. What
distinguishes Tcslog is what a reader can still do with the remains.

**Damage is contained.** Splitting a log into segment files puts a bound
on what any single failure costs: the records in the damaged file, and at
most one more that happened to straddle its boundary. Everything else in
the log still reads. The bound is a choice the caller makes, because the
segment file size is a parameter: smaller files mean less telemetry lost
per failure.

**The reader recovers by itself.** On finding a file missing,
unreadable, or cut short, the reader gives up the record it was in the
middle of, finds the start of the next whole record in the first file
that opens cleanly, and carries on. It does not stop, and it does not
need to be reset. The only thing that ends a read is running out of
files.

**Losses are reported, not hidden.** A reader that quietly returned a
smaller log would be worse than one that failed, because nothing
downstream would know that the gap in the telemetry was a gap rather
than a quiet spell. Tcslog reports every loss it finds, and reports how
many files' worth went missing. Losses that fall exactly on a record
boundary are caught too, which is the case a naive reader misses
entirely: both sides of such a gap look perfectly ordinary.

**Nothing is invented.** The reader will not stitch bytes from either
side of a gap into a record and hand it over. Where the surviving bytes
cannot be shown to be one record, the record is reported lost. A
plausible-looking record that was never written is a worse outcome than a
missing one, because it cannot be told from real telemetry.

**Partial records are still telemetry.** Bytes that reached the caller
before a gap are real measurements, and are handed over rather than
discarded, marked so that a record cut short cannot be mistaken for a
whole one.

**A renamed or copied file is still readable.** Each segment file
carries its own identity, so a file that has been renamed, or copied out
of its directory, can still be identified and its contents examined.

Concepts
========

Segment file
    One file of a log. It holds a header followed by records. At most
    ``seg_size_max`` bytes, and exactly that many unless it is the last
    one of a run.

Log
    A directory together with a prefix and a suffix. Every file in that
    directory whose name begins with the prefix and ends with the suffix
    is a segment file of that log. One directory can therefore hold
    several unrelated logs, distinguished by their prefixes.

Record
    One unit of telemetry: a payload supplied by the caller, together
    with whatever metadata the log's format adds. A record may be larger
    than a segment file, in which case it continues into the next one.

Session
    Everything one ``LogWrite`` writes. Constructing a ``LogWrite``
    starts a session, and so does the first write after ``clear()``. A
    reader reports a boundary between one session and the next before
    returning the records of the later one.

    Sessions matter because a new one begins whenever writing was
    interrupted -- deliberately, or by a fault. Record numbering restarts
    with each session, so a reader that did not announce the boundary
    would appear to hand back two records with the same number.

Building and Installation
=========================

Prerequisites
-------------

A Rust toolchain of version 1.75 or later. Tcslog depends on nothing
outside the standard library except a small error-handling helper, so
there is nothing else to install.

The real-time clock must be set
-------------------------------

**The system's real-time clock must hold the correct time before any
Tcslog function is called.** On a system that acquires the time from
elsewhere -- a ground station, a GPS receiver, an NTP server, a
real-time clock chip read at boot -- that acquisition has to complete
first.

``LogWrite::new`` checks what it can. Most systems leave an unset clock
at the UNIX epoch or before it, so a clock that does not read later than
the epoch is taken as unset and refused with ``ClockError``; the log is
not created and nothing is written. A clock that is set but wrong cannot
be detected, and Tcslog does not try: the times it records are then
wrong by however much the clock is, and nothing later corrects them.

The reason the requirement is this strict, rather than a matter of tidy
timestamps, is in `The clock a writer keeps`_: a writer reads the
real-time clock exactly once, when it is constructed, and a correction
arriving after that does not reach the identifiers already minted.

The timer resolution
--------------------

A segment file is identified by the time it was created, so Tcslog needs
to know how finely the writer's clock advances: when two files are named
within one tick of it, the writer waits for the clock to move on, and it
has to be told how long that is. The value is supplied at build time, in
nanoseconds, as ``TIMER_RESOLUTION``. Nothing is guessed for you, since a
figure guessed in the source would be wrong on some machine.

**It need not be exact.** The true figure is hard to come by: nothing in
the Rust standard library reports it, and what ``thread::sleep`` actually
waits for a given duration is only loosely bounded -- on Linux it is
subject to the scheduler's timer slack rather than to any stated clock
granularity. So a writer corrects a value that proves too small, by the
rule in `Learning the timer resolution`_, and ``timer_resolution()``
reports what it arrived at. A value of 1 is a reasonable place to
start.

The usual way is a per-machine Cargo configuration file, which keeps the
value out of the source tree. Copy the example and edit it::

    cp .cargo/config.toml.example .cargo/config.toml
    $EDITOR .cargo/config.toml

It holds::

    [env]
    TIMER_RESOLUTION = "1"

Either way works; the environment variable can also be given on the
command line::

    TIMER_RESOLUTION=1 cargo build

**Supplying nothing is allowed, and is reported at run time.** Left unset,
the value is zero: the crate builds, and ``LogWrite::new`` then refuses to
open a log and reports ``TimerResolutionZero``. That is deliberate. A
program taking Tcslog from a registry has no ``.cargo/config.toml`` of
this repository's, and a build that stopped would leave it with a failure
inside somebody else's build script rather than anything it could act on;
an error from ``LogWrite::new`` names itself and says what to set. Zero is
also the one value the widening rule cannot correct, doubling it leaving
it zero, so it cannot simply be treated as a small number.

A value that is not a nanosecond count does stop the build. Not setting
the variable says nobody said; setting it to nonsense says somebody said
something wrong, which is worth stopping for.

Reading a log needs no timer resolution, because nothing is being
created. A program that only reads can take the crate without the
default features and supply no value at all::

    [dependencies]
    tcslog = { version = "0.2", default-features = false }

Building a program against the library
--------------------------------------

Name the crate as a dependency::

    [dependencies]
    tcslog = "0.2"

Or, working inside a checkout of this repository::

    [dependencies]
    tcslog = { path = "../tcslog" }

Then build as usual::

    cargo build

A complete program that writes a log and reads it back:

.. code-block:: rust

    use tcslog::{
        Format, LogError, LogRead, LogWrite, WriteCallbacks,
        SEGMENT_FILE_HEADER_LEN,
    };

    fn main() -> Result<(), LogError> {
        // Segment files of one kibibyte of telemetry each.
        let seg_size_max = SEGMENT_FILE_HEADER_LEN + 1024;

        let mut log = LogWrite::new(
            "/var/telemetry",
            "tlm-",
            ".seg",
            seg_size_max,
            Format::VariableTsRc,
            WriteCallbacks::default(),
        )?;
        log.write_str("attitude nominal")?;
        log.write(&[0x01, 0x02, 0x03])?;
        log.flush()?;
        drop(log);

        let mut log = LogRead::new("/var/telemetry", "tlm-", ".seg")?;
        let mut buf = [0u8; 256];
        loop {
            match log.read(&mut buf) {
                Ok(result) => {
                    let payload = &buf[..result.n as usize];
                    println!("{}", String::from_utf8_lossy(payload));
                }
                Err(LogError::Eof) => break,
                Err(LogError::SessionEnd) => println!("-- end of session --"),
                Err(LogError::ReadTruncated { lost, n }) => {
                    println!("lost {lost} file(s); {n} byte(s) recovered");
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

Note that ``WriteCallbacks::default()`` does nothing when a segment file
fills, which leaves the files in the directory. That suits development
and the examples above. A program storing telemetry in earnest should
replace the ``send`` member; see `Handing over a segment file`_.

Inspecting a log: tcslog-tools
------------------------------

``tcslog-dump`` reads a whole log and prints it, and ``tcslog-dumphdr``
prints one segment file's header. They are a separate crate,
``tcslog-tools``, in its own repository: they only read a log, so they
want neither the library's ``write`` feature nor the build-time timer
resolution that comes with it, and someone who only needs to look at a
log should not have to build the writing side to do it. Install both
from the registry::

    cargo install tcslog-tools

Their own documentation -- installing them, their arguments, and their
options -- is ``docs/tcslog-tools.rst`` in the `tcslog-tools repository
<https://github.com/vomlehn/tcslog-tools>`_.

The error-recovery suite under ``test/`` drives both tools, so it needs
a checkout of that repository. ``bin/tcslog-tool`` is what finds it: it
expects ``../tcslog-tools`` beside this repository, and ``TCSLOG_TOOLS``
points it somewhere else.

Running the sample
------------------

The ``sample`` example writes a small demonstration log, so that it and
``tcslog-dump`` form a runnable pair for someone meeting the crate for
the first time. It is an example of the library crate, so it needs no
separate build step and is compiled by ``cargo test`` along with
everything else.

It takes the log directory, prefix, and suffix as positional arguments,
in that order -- the same three ``tcslog-dump`` takes, so a log written
by one is read back by the other::

    cargo run --example sample -- /tmp/demo demo- .seg --verbose
    ./bin/tcslog-tool tcslog-dump /tmp/demo demo- .seg --verbose

``bin/run-sample`` does the same against a temporary directory, lists
the segment files it wrote, and deletes the directory afterwards::

    ./bin/run-sample --verbose

The example's source, ``tcslog/examples/sample.rs``, is the shortest
complete illustration of the writing side: the segment size it picks,
the callbacks it supplies, and what it does with the session ID.

Record Formats
==============

The format is chosen when a log is created and cannot change within it.
It fixes what each record may hold and how much every record costs
beyond its payload.

In the table, *data header* is the per-record bookkeeping Tcslog adds
ahead of the payload; the byte count given is what each record costs in
addition to its payload bytes. ``RecSize`` is the type holding a payload
length, which is a 32-bit unsigned integer, so ``RecSize::MAX`` is
4294967295. ``Timestamp`` and ``RecordCount`` are both 64-bit unsigned
integers.

.. table:: Record formats
   :widths: 12 14 30 44

   +----------------+----------------+---------------------------------+----------------------------------------+
   | Format         | Payload size   | Data header elements            | When to use it                         |
   |                | (bytes)        |                                 |                                        |
   +================+================+=================================+========================================+
   | ``Fixed(n)``   | Exactly ``n``, | None. **0 bytes per record.**   | Telemetry of one fixed shape: a        |
   |                | which must be  |                                 | sensor frame, a fixed-width status     |
   |                | from 1 to      | Records sit directly against    | word, a packed structure. The most     |
   |                | ``RecSize::    | one another, since a reader     | compact choice, because a record       |
   |                | MAX``.         | already knows how long each     | costs nothing beyond its payload.      |
   |                |                | one is.                         | Use it when the length will not        |
   |                |                |                                 | change for the life of the log, and    |
   |                |                |                                 | when the payload already carries       |
   |                |                |                                 | whatever timing the ground needs.      |
   |                |                |                                 | A payload of any other length,         |
   |                |                |                                 | including an empty one, is refused.    |
   +----------------+----------------+---------------------------------+----------------------------------------+
   | ``Variable     | 0 to           | ``n``: ``RecSize``. The number  | Telemetry of varying size that         |
   | Simple``       | ``RecSize::    | of payload bytes in this        | already carries its own timestamp:     |
   |                | MAX``.         | record.                         | text messages, packets from a bus      |
   |                |                |                                 | that stamps them, anything already     |
   |                | Empty records  | **4 bytes per record.**         | timed upstream. Pays for a length      |
   |                | are allowed.   |                                 | and nothing else. This is the usual    |
   |                |                |                                 | choice when records vary in size.      |
   +----------------+----------------+---------------------------------+----------------------------------------+
   | ``Variable     | 0 to           | ``n``: ``RecSize``. The number  | Telemetry that must be dated and       |
   | TsRc``         | ``RecSize::    | of payload bytes in this        | counted by the logger rather than by   |
   |                | MAX``.         | record.                         | its source: sensors that report no     |
   |                |                |                                 | time of their own, or a log that must  |
   |                | Empty records  | ``timestamp``: ``Timestamp``.   | show the order and spacing of what it  |
   |                | are allowed.   | When the record was written, as | received. The record count also        |
   |                |                | nanoseconds since the UNIX      | reveals a gap in a session directly.   |
   |                |                | epoch. Generated by Tcslog.     | The widest data header of the three,   |
   |                |                |                                 | so the least suited to very short      |
   |                |                | ``record count``:               | records: twenty bytes of bookkeeping   |
   |                |                | ``RecordCount``. This record's  | on a four-byte payload is mostly       |
   |                |                | position within its session,    | bookkeeping.                           |
   |                |                | starting at 1. Generated by     |                                        |
   |                |                | Tcslog.                         |                                        |
   |                |                |                                 |                                        |
   |                |                | **20 bytes per record.**        |                                        |
   +----------------+----------------+---------------------------------+----------------------------------------+

Choosing a segment file size
----------------------------

``seg_size_max`` must be strictly greater than
``SEGMENT_FILE_HEADER_LEN`` plus one data header for the format in use; a
file that could not hold a single data header would leave the writer
nowhere to put a record. Anything less is refused with
``SegSizeTooSmall``.

Beyond that floor the size is a trade. A smaller segment file loses less
telemetry when one is damaged, and lets data go down in finer pieces. A
larger one spends less of the log on per-file headers and rolls less
often. Sizing the file so that a whole number of records fits exactly is
worth doing where records have a fixed size, because then no record ever
straddles a boundary and no record is ever at risk from the loss of two
files instead of one.

Note that ``seg_size_max`` counts the whole file, header included, which
is why ``SEGMENT_FILE_HEADER_LEN`` is public: a caller wanting a
particular amount of telemetry per file adds the two.

Functions
=========

Everything below is reached through the crate root, for example
``use tcslog::LogWrite;``. Reading and writing both advance internal
state, so those methods take ``&mut self``.

Writing: LogWrite
-----------------

``LogWrite::new(dir, prefix, suffix, seg_size_max, format, callbacks) -> Result<LogWrite, LogError>``
    Begins writing a log, in a new session. Every segment file already in
    ``dir`` whose name matches the prefix and suffix is handed to the
    ``send`` callback before this session's first segment file is
    created, so a new ``LogWrite`` never appends to what it finds: it
    takes the older files off the library's hands and starts afresh.

    ``dir``
        Name of the directory that is to hold the segment files. It must
        already exist; this function does not create it.

    ``prefix``
        String forming the first part of every segment file name. Must
        not contain a path separator.

    ``suffix``
        String forming the last part of every segment file name. Must not
        contain a path separator.

    ``seg_size_max``
        Largest a segment file may grow, in bytes, counting its header.
        Must be strictly greater than ``SEGMENT_FILE_HEADER_LEN`` plus
        one data header for ``format``.

    ``format``
        How records are to be laid out. See `Record Formats`_.

    ``callbacks``
        A ``WriteCallbacks`` holding the functions to invoke as records
        complete and segment files fill.

    *Returns* the new writer, with its first segment file created, or one
    of ``TimerResolutionZero``, ``ClockError`` (the real-time clock does
    not read later than the UNIX epoch, so it has not been set -- see
    `The real-time clock must be set`_), ``PathDelimiterNotAllowed``,
    ``SegSizeTooSmall``, ``FixedLenMismatch`` (for ``Format::Fixed(0)``),
    ``InvalidPathname``, or ``IoError``.

    The real-time clock is read once, here, and paired with the monotonic
    clock; every time this writer records afterwards is measured from that
    pairing. See `The clock a writer keeps`_.

``LogWrite::write(&mut self, msg) -> Result<u32, LogError>``
    Writes a byte array to the log as one record. The record may span
    segment files; each time the current one fills, the ``send`` callback
    is invoked with its path and a fresh file is opened. The
    ``record_complete`` callback is invoked once every byte has been
    written.

    ``msg``
        The payload bytes to store.

    *Returns* the total number of bytes written, counting the per-record
    data header as well as the payload, or one of ``FixedLenMismatch``
    (the format is ``Fixed(n)`` and ``msg`` is not ``n`` bytes, an empty
    payload included), ``PayloadTooLarge``, or ``IoError``. A timestamp
    cannot fail here: the clock a record is stamped from was validated
    when the writer was constructed.

``LogWrite::write_str(&mut self, msg) -> Result<u32, LogError>``
    Writes the UTF-8 bytes of a string as one record, by calling
    ``write``.

    ``msg``
        The text to store.

    *Returns* what ``write`` returns, and the same errors.

``LogWrite::flush(&mut self) -> Result<(), LogError>``
    Flushes buffered data for the segment file being written to storage.
    There is nothing to do when no segment file is open.

    *Returns* nothing on success, or ``IoError`` if the underlying flush
    fails.

``LogWrite::clear(&mut self) -> Result<(), LogError>``
    Removes every one of this log's segment files from the directory,
    including the one being written, which is closed first. The records
    in them are discarded, including a record part way through being
    written, and the ``send`` callback is **not** invoked for any of
    them: that callback hands a file to the caller, and these are being
    thrown away.

    The writer stays usable and is left with no segment file open. The
    next ``write`` begins a new session, numbering its records from one
    again. Until then, ``session_id`` and ``current_segment_id`` still
    report the cleared session, which names files that no longer exist.

    *Returns* nothing on success, or ``IoError`` if enumerating the
    directory or removing a file fails. The open segment file is closed
    before either is attempted, so it is closed even when the removal
    that follows fails.

``LogWrite::session_id(&self) -> SegId``
    *Returns* the identifier of this session's first segment file, which
    every segment file of the session carries and which is what a reader
    watches for a change in.

``LogWrite::current_segment_id(&self) -> SegId``
    *Returns* the identifier of the segment file being written.

``LogWrite::timer_resolution(&self) -> u64``
    *Returns* how finely this writer believes its clock advances, in
    nanoseconds: the build-time ``TIMER_RESOLUTION`` to begin with, and
    whatever the rule in `Learning the timer resolution`_ has widened it
    to since. A value above the one supplied says the supplied one was
    too small for this machine, and is the figure the next build should
    be given.

``LogWrite::last_meta(&self) -> Meta``
    *Returns* the metadata stored with the most recently written record.
    For ``Format::VariableTsRc`` this is how a caller learns the
    timestamp and record count that were written, since Tcslog generates
    both rather than taking them from the caller.

``LogWrite::SEGMENT_FILE_HEADER_LEN``
    The length of a segment file header in bytes, as a constant. An alias
    for the crate-level constant of the same name, repeated here because
    ``seg_size_max`` is given relative to it.

``drop``
    Dropping a ``LogWrite`` flushes the segment file being written and
    hands it to ``send`` if it holds any records, so that the records
    written last are not stranded in a file the caller was never told
    about. A destructor cannot report a failure, so errors from the flush
    and from ``send`` are discarded; call ``flush`` first if you need to
    know.

Reading: LogRead
----------------

``LogRead::new(dir, prefix, suffix) -> Result<LogRead, LogError>``
    Opens an existing log for reading. No segment file is opened yet: the
    first read finds the start of the first record by the same path a
    read recovering from a fault takes, so a log whose opening files are
    already gone needs no special handling from the caller.

    ``dir``
        Name of the directory holding the segment files.

    ``prefix``
        String forming the first part of the segment file names. Must not
        contain a path separator.

    ``suffix``
        String forming the last part of the segment file names. Must not
        contain a path separator.

    *Returns* the new reader, or one of ``PathDelimiterNotAllowed``,
    ``InvalidPathname``, ``NoSegmentFiles`` (no file in the directory
    matches, so there is no log there), or ``IoError``.

``LogRead::read(&mut self, buf) -> Result<ReadResult, LogError>``
    Reads the next record's payload into ``buf``.

    ``buf``
        Buffer to receive the payload. If the payload is longer, the
        first ``buf.len()`` bytes are copied and ``ReadOverflow`` is
        returned carrying that same count, so that a filled buffer can be
        told from a complete record. The rest of that payload is
        discarded, so the following read begins at the next record rather
        than inside this one.

    *Returns* a ``ReadResult`` giving how many bytes were placed in
    ``buf`` and the metadata the record carried, or one of ``Eof`` (the
    log is finished), ``SessionEnd`` (once at each session boundary),
    ``ReadTruncated`` (telemetry was lost; see `Theory of Operation`_),
    ``ReadOverflow``, or ``IoError``.

``LogRead::read_str(&mut self, buf) -> Result<ReadResult, LogError>``
    Equivalent to ``read``, named for callers whose payloads are text. It
    takes the same byte buffer rather than a string, because a payload
    recovered in part need not be valid UTF-8.

    ``buf``
        Buffer to receive the payload.

    *Returns* what ``read`` returns, and the same errors.

``LogRead::iter(&mut self) -> LogReadIter``
    *Returns* an iterator over the remaining records, yielding ``Record``
    values that own their payloads. This is a convenience that gives up
    the crate's no-allocation guarantee; ``read`` is what a caller bound
    by that guarantee uses. Session boundaries and unrecoverable records
    are skipped, and a payload longer than the iterator's internal buffer
    of 64 KiB is yielded cut to it. A caller with records that long
    should size its own buffer and use ``read``, which reports the
    overflow.

``LogRead::current_header(&self) -> Option<&SegmentHeader>``
    *Returns* the header of the segment file now open, or ``None``
    whenever none is: before the first read, and after a read that
    returned ``Eof``, ``SessionEnd``, or ``ReadTruncated``. A
    ``ReadOverflow`` leaves the file open.

``LogRead::segments_opened(&self) -> u64``
    *Returns* how many segment files the reader has opened. A record
    spanning several files only ever reports the one it ended in, so this
    tally, rather than a count of the headers the caller has seen, is
    what describes a log's extent.

``LogRead::collect_opened_headers(&mut self, enable)``
    Asks the reader to retain the header of every segment file it opens,
    for a caller that wants to report on the files a read passed through.

    ``enable``
        Whether to retain headers from now on.

    Retention is off by default and must be: the headers accumulate until
    taken, so a caller that never took them would grow the buffer without
    bound.

``LogRead::take_opened_headers(&mut self) -> Vec<SegmentHeader>``
    *Returns* the headers retained since the last call, oldest first, and
    removes them from the reader. Taking them before examining a read's
    result puts each header ahead of the records it carried, and lets a
    read that ended the log still report the files it opened.

Callbacks: WriteCallbacks
-------------------------

A structure of three function pointers, rather than closures or trait
objects, so that it sits inside a ``LogWrite`` with no allocation and no
dynamic dispatch. ``WriteCallbacks::default()`` supplies functions that
do nothing, apart from reporting a widened timer resolution on standard
error, which suits development.

Naming only the fields that matter and taking the rest from the default
keeps a literal working when a callback is added::

    WriteCallbacks {
        send: ship_it,
        ..WriteCallbacks::default()
    }

``record_complete: fn(&mut File) -> std::io::Result<()>``
    Invoked after each record has been written, with the segment file the
    record ended in.

    What it does is the caller's choice of priority. Flushing the file
    trades throughput for a smaller window in which an unplanned reset
    loses the record; doing nothing makes the opposite trade. An error
    returned here reaches the caller of ``write`` as ``IoError``, after
    the record's bytes have already been written.

``send: fn(&Path) -> std::io::Result<()>``
    Invoked with the full path of a segment file whose data section has
    filled, and also with each pre-existing segment file that
    ``LogWrite::new`` finds.

    *Returns* success, or an error that reaches the caller of ``write``
    as ``IoError``.

``timer_resolution_adjusted: fn(u64)``
    Invoked when the timer resolution has been widened, with the value
    now in force in nanoseconds.

    A widening says the build-time ``TIMER_RESOLUTION`` is below what
    this machine needs -- see `Learning the timer resolution`_ for when
    that is concluded. It is reported once per doubling, and not at all
    for a first collision, which computes no new value, nor once the
    value has saturated and a doubling leaves it unchanged.

    *Returns* nothing, and the writer waits and retries whatever the
    callback does. It is a notification, not a decision.

**Detecting a too-small resolution.** Taking no return value, this
callback leaves the choice of what a widening means with the caller, and
the two reasonable answers point in opposite directions.

A deployed system will usually want to record the value and carry on. The
widening is the writer repairing itself: the log is unharmed, every record
is written, and the only cost is the wait that was spent discovering the
right figure. Stopping a vehicle's telemetry over it would trade a sound
log for no log.

A system under development will usually want the opposite. A too-small
value is a configuration fault, and the easiest place to act on it is
where it was found, so a callback that panics or aborts here stops the
program with the faulty value in hand. That is the reason this is a
callback rather than something the library decides: the same code should
be able to behave both ways.

The default does the conservative half -- it prints a message naming the
new value on standard error and returns, so the log keeps being written
and the figure is not lost. The value passed is the one to put in
``TIMER_RESOLUTION`` for the next build.

.. _Handing over a segment file:

**Handing over a segment file.** ``send`` is how a segment file leaves
Tcslog's care. It may do anything appropriate:

- compress the file and move it elsewhere;
- send it down;
- tell a controller that another file is ready;
- flush it, to narrow the window in which a reset could damage it.

When it returns there must be no file at the path it was given, and none
matching the log's naming pattern. That is the contract, and it is what
makes the storage bound hold: the space is no longer being accounted for
by the library. The default ``send``, which does nothing, therefore lets
segment files accumulate, and is not suitable for a log that runs for
long.

**Rename the file rather than leave it.** The cheapest way to satisfy the
contract is to rename the file to something that cannot be a segment file
name of this log -- a name that does not both begin with the prefix and
end with the suffix -- and to work on it under that name. A rename within
a directory is a metadata operation; a copy is not.

It also keeps the file out of what ``LogWrite::new`` has to look at.
Constructing a writer enumerates the log's directory, parses the
identifier out of every name matching the prefix and suffix, and sorts
them. That one scan does two jobs -- it finds the files to hand to
``send``, and it supplies the clock seed described in `The clock a writer
keeps`_ -- and it costs time in proportion to how many such files there
are. A ``send`` that leaves them in the log's namespace makes every later
``LogWrite::new`` pay for all of them, and hands each of them over again.
On a system that opens a log at every restart, that cost grows without
bound.

A file renamed out of the namespace stops seeding the clock as well. That
is the contract working as intended rather than something lost: once
``send`` has taken a file, the library accounts for it no further.

Results and errors
------------------

``ReadResult``
    What one successful read produced. ``n`` is the number of payload
    bytes placed in the caller's buffer, and ``meta`` is the metadata the
    record carried.

``Record``
    One record's payload with its metadata, as produced by
    ``LogRead::iter``: ``meta``, and ``payload``, which owns its bytes.
    This is the one public structure that allocates.

``Meta``
    The metadata a record carried, which follows the log's format:
    ``Fixed`` and ``VariableSimple`` carry none, and
    ``VariableTsRc(Timestamp, RecordCount)`` carries the time the record
    was written and its position within its session.

``Format``
    Which record layout a log uses: ``Fixed(RecSize)``,
    ``VariableSimple``, or ``VariableTsRc``. ``data_header_len()`` gives
    what one record costs beyond its payload, and ``fixed_len()`` gives
    the ``n`` of ``Fixed``, or zero for the others.

``SegId``
    A segment file's identifier: nanoseconds since the UNIX epoch, taken
    when the file was created, from the clock described in `The clock a
    writer keeps`_. It increases with time but is not dense, so it cannot
    be used to count files or to notice a gap between two of them.
    Displaying one yields a fixed-length string of ``SegId::STR_LEN``
    characters, which is the part of a segment file name between the
    prefix and the suffix.

``SeqId``
    A segment file's position within its session. Unlike ``SegId`` this
    does count, and it is what lets a reader see that a file is missing.

``SegmentHeader``
    What a segment file's header holds, as reported by
    ``LogRead::current_header`` and ``LogRead::take_opened_headers``:
    ``segment_id``, ``session_id``, ``max_size``, ``remaining``,
    ``format``, and ``sequence``.

``RecSize``, ``Timestamp``, ``RecordCount``
    The types of a payload length, a record timestamp, and a record
    count. See `Record Formats`_.

``VERSION_MAJOR``, ``VERSION_MINOR``, ``VERSION_PATCH``
    The version of the stored format this build reads and writes, so that
    a caller can report it or refuse a log it was not built for.

``format_timestamp(ts) -> String``
    Renders a ``Timestamp`` as an ISO 8601 UTC date and time with
    nanosecond precision, for example
    ``2026-09-21T16:45:12.123456789Z``.

    ``ts``
        Nanoseconds since the UNIX epoch.

    *Returns* the rendered string.

``record_trailer(payload_len, meta) -> String``
    Renders a record's length and its metadata as the parenthesised
    trailer the support binaries print after a payload, so that a record
    reads the same coming out of a log as it did going in.

    ``payload_len``
        Number of payload bytes the record holds.

    ``meta``
        The metadata the record carried.

    *Returns* the trailer, parentheses included.

``LogError``
    Returned by everything that can fail. Nothing in the library panics,
    and no variant exists that nothing raises.

    ``ClockError``
        The real-time clock did not read later than the UNIX epoch when
        ``LogWrite::new`` was called, which is what an unset clock reads
        on most systems. No log was created. See `The real-time clock
        must be set`_.

    ``Eof``
        No more records are available.

    ``FixedLenMismatch``
        The format is ``Fixed(n)`` and the payload is not ``n`` bytes; an
        empty payload under ``Fixed(n)`` is this error. Also returned for
        a format of ``Fixed(0)``, which no log may use.

    ``InvalidHeader``
        The bytes offered do not form a segment file header.

    ``InvalidPathname``
        The directory, prefix, and suffix do not combine into a usable
        path, or the named directory is not a directory.

    ``IoError``
        An underlying I/O operation failed. Carries the cause.

    ``NoSegmentFiles``
        No file in the directory matches the prefix and suffix, so there
        is no log there to read.

    ``PathDelimiterNotAllowed``
        The prefix or suffix contains a path separator.

    ``PayloadTooLarge``
        The payload exceeds ``RecSize::MAX``, or its length plus its data
        header would exceed the count ``write`` returns.

    ``ReadOverflow(n)``
        The record was longer than the supplied buffer. ``n`` is how many
        bytes reached the front of it; they are real telemetry. The rest
        of the record was skipped, so the next read starts at the
        following one.

    ``ReadTruncated { lost, n }``
        Telemetry was lost. ``lost`` is how many segment files are
        missing, and is zero when no file is missing and the damage was
        inside one that survived. ``n`` is how many payload bytes of the
        cut-short record reached the front of the buffer; those bytes are
        real telemetry and may be used. It is zero when the record was
        cut short before any payload was reached. The next read
        resynchronizes.

    ``SegSizeTooSmall``
        The requested ``seg_size_max`` leaves no room for a record.

    ``SessionEnd``
        Every record of the session just read has been returned, and the
        next segment file belongs to a different session. The read after
        this one returns that session's first record.

    ``TimerResolutionZero``
        The build-time timer resolution is zero, so no wait could
        separate two segment identifiers, and doubling zero cannot change
        that. No log was created. This is what an unset
        ``TIMER_RESOLUTION`` gives, so it is what a program that has
        supplied no value sees. See `The timer resolution`_.

    ``VersionMismatch``
        The segment file was written by a version of the stored format
        this build cannot read.

Calling From C
==============

A C program reaches the same two interfaces through ``tcslog-c``, a
crate in this workspace that presents ``LogWrite`` and ``LogRead`` as a
C ABI. It is a separate crate because a C interface has to be built as a
shared and a static library, which a crate cannot be only when asked,
and because the unsafe code an ABI needs is then confined to it: the
library itself contains none.

Nothing about the log changes. A log written from C is read by a Rust
caller and the other way round, the stored format being the same one.

Installing
----------

From the repository root::

    make install

The header goes to ``$HOME/include/tcslog.h`` and both libraries to
``$HOME/lib``. ``make install PREFIX=/usr/local`` chooses somewhere
else, ``DESTDIR`` stages the install for packaging, and ``make
uninstall`` removes them again, honouring both variables. Then compile
against it::

    cc prog.c -I$HOME/include -L$HOME/lib -ltcslog_c

Both a static and a shared library are installed, so either kind of
link works. Neither a pkg-config file nor the Rust library is
installed: a consumer needs ``-ltcslog_c`` and nothing a pkg-config
file would add.

``TIMER_RESOLUTION`` is a build-time value of the Rust library, so it is
fixed when the library is built and a C caller cannot supply it. See
`The timer resolution`_; a library built without it
reports ``TCSLOG_STATUS_TIMER_RESOLUTION_ZERO`` when a writer is opened.

How the interface is shaped
---------------------------

Every function returns a ``TcslogStatus``, and everything the caller
wants back is written through a pointer argument. That is what lets a C
caller check an outcome without ambiguity: no status can be confused
with data, and no out-parameter has to double as an error signal.

``TCSLOG_STATUS_OK`` is zero and every other code is positive, so ``if
(status)`` reads as "something happened". The first five are not
failures at all -- they are what the library reports about the telemetry
itself, and a caller carries on after each. The full list, and which
``LogError`` each failure corresponds to, is in
``docs/tcslog-prompt.rst``; ``tcslog_status_str`` gives a short
description of any of them at runtime.

The numbers are ABI. Once a program has been compiled against the
header they are fixed, so a code's value never changes and a new one is
only ever added after the last.

Handles are opaque. ``tcslog_write_open`` and ``tcslog_read_open``
produce one, leaving it null if they fail, and ``tcslog_write_close``
and ``tcslog_read_close`` release one. Both closers accept null, as
``free`` does, and a handle must not be used after being closed.

Writing
-------

``tcslog_write_open(dir, prefix, suffix, seg_size_max, format_tag, fixed_len, out)``
    Begins writing, as ``LogWrite::new`` does and with the same
    arguments, except that the format is an integer tag --
    ``TCSLOG_FORMAT_FIXED``, ``TCSLOG_FORMAT_VARIABLE_SIMPLE``, or
    ``TCSLOG_FORMAT_VARIABLE_TS_RC`` -- and ``fixed_len`` carries the
    record length, which is read only for the first of those. A tag
    outside the three is refused with
    ``TCSLOG_STATUS_INVALID_FORMAT``. The callbacks are not an argument;
    see `Callbacks from C`_.

``tcslog_write_record(h, data, len, written)``
    Writes one record of ``len`` bytes. ``written``, when it is not
    null, is left holding what the record occupied in the log, its data
    header included. A ``len`` of zero is allowed and ``data`` may then
    be null.

``tcslog_write_flush(h)``
    Flushes the segment file being written, and reports a failure to do
    so.

``tcslog_write_clear(h)``
    Removes every segment file of this log.

``tcslog_write_timer_resolution(h, out)``
    Writes the resolution in force, which is the build-time value unless
    the writer found it too small and widened it.

``tcslog_write_close(h)``
    Closes the writer. The segment file being written is flushed and, if
    it holds any records, handed to ``send`` -- so the records written
    last are not stranded in a file the caller was never told about.
    That file is short, unlike every other file ``send`` is given.

    A close cannot report a failure, so an error from that flush or from
    ``send`` is discarded. A caller that needs to know the last records
    reached storage calls ``tcslog_write_flush`` first, which does
    report.

Reading
-------

``tcslog_read_open(dir, prefix, suffix, out)``
    Begins reading, as ``LogRead::new`` does.

``tcslog_read_record(h, buf, cap, result)``
    Reads the next record into ``buf``. ``result`` is filled whatever
    the status, so its fields can be read without checking first, and
    holds:

    ``n``
        Payload bytes placed in the buffer, which are real telemetry
        whichever status came with them: the whole record on
        ``TCSLOG_STATUS_OK``, the bytes recovered of a record cut short on
        ``TCSLOG_STATUS_READ_TRUNCATED``, and the front of a record too
        large for the buffer on ``TCSLOG_STATUS_READ_OVERFLOW``.

    ``meta``, ``timestamp``, ``record_count``
        Which of the three metadata shapes the record had, and, for
        ``TCSLOG_META_VARIABLE_TS_RC``, the time it was written and its
        position in the session. The latter two are zero for the other
        shapes.

    ``lost``
        Segment files found missing, on
        ``TCSLOG_STATUS_READ_TRUNCATED``. Zero where a record was cut
        short with no file missing at all.

``tcslog_read_segments_opened(h, out)``
    Writes the number of segment files this reader has opened.

``tcslog_read_close(h)``
    Releases the reader.

The rule is the Rust one: read again until ``TCSLOG_STATUS_EOF``.
Everything else is news about the telemetry rather than a failure of the
reader, and `What a caller should do with each outcome`_ applies
unchanged -- including what it says about
``TCSLOG_STATUS_READ_OVERFLOW``, which is worth repeating because it is
the one code whose name invites the wrong reading. The buffer holds the
front of the record and the rest of it is gone, skipped rather than
kept; reading again returns the record after it, not another attempt at
this one. A buffer that overflowed once is too small for the log, and
the remedy is a bigger buffer next time rather than a retry now.

Callbacks from C
----------------

The three callbacks ``WriteCallbacks`` holds are bare function pointers
with no context argument, so there is nowhere to put a per-log C
context. They are set for the whole process instead::

    int on_send(const char *path);
    int on_record_complete(int fd);
    void on_timer_resolution_adjusted(uint64_t resolution_ns);

    tcslog_set_send_callback(on_send);
    tcslog_set_record_complete_callback(on_record_complete);
    tcslog_set_timer_resolution_adjusted_callback(on_adjusted);

Passing null unsets one, which is the default. Set them before opening a
writer: a writer takes the callbacks as they stand when it is opened, so
a setter called afterwards does not reach a writer already open. A
caller that must tell its logs apart has the segment file's path, which
``send`` is given, and the log's directory and prefix are in it.

``send`` and ``record_complete`` return an ``int``: zero for success,
and anything else makes the write that triggered the callback report
``TCSLOG_STATUS_IO_ERROR``. That is the right answer for a ``send`` that
could not take a file -- the storage bound the log was given has stopped
holding, and the caller needs to know. ``record_complete`` is handed a
descriptor the library still owns, so it must not close it.

What ``send`` has to do is what it has to do in Rust: when it returns
there must be no file at the path it was given and none matching the
log's naming pattern. See `Callbacks: WriteCallbacks`_.

Panics do not cross the boundary
--------------------------------

Letting a panic unwind out of a C function is undefined behaviour, so
every function catches one and reports ``TCSLOG_STATUS_PANIC`` instead.
A caller that sees it should treat the handle as unusable: the panic
happened part way through an operation and the binding cannot say how
far.

A complete program
------------------

Writing a log and reading it back:

.. code-block:: c

    #include <stdio.h>
    #include <string.h>
    #include "tcslog.h"

    int main(void) {
        TcslogWrite *w = NULL;
        TcslogStatus s = tcslog_write_open(
            "/var/telemetry", "seg-", ".tcslog",
            tcslog_segment_file_header_len() + 1024,
            TCSLOG_FORMAT_VARIABLE_TS_RC, 0, &w);
        if (s != TCSLOG_STATUS_OK) {
            fprintf(stderr, "open: %s\n", tcslog_status_str(s));
            return 1;
        }

        const char *msg = "attitude nominal";
        s = tcslog_write_record(w, (const uint8_t *)msg, strlen(msg), NULL);
        if (s != TCSLOG_STATUS_OK) {
            fprintf(stderr, "write: %s\n", tcslog_status_str(s));
            tcslog_write_close(w);
            return 1;
        }
        tcslog_write_close(w);

        TcslogRead *r = NULL;
        s = tcslog_read_open("/var/telemetry", "seg-", ".tcslog", &r);
        if (s != TCSLOG_STATUS_OK) {
            fprintf(stderr, "open for reading: %s\n", tcslog_status_str(s));
            return 1;
        }

        for (;;) {
            unsigned char buf[4096];
            TcslogReadResult result;
            s = tcslog_read_record(r, buf, sizeof buf, &result);
            if (s == TCSLOG_STATUS_EOF) {
                break;
            }
            if (s == TCSLOG_STATUS_OK) {
                printf("#%lu %.*s\n", (unsigned long)result.record_count,
                       (int)result.n, buf);
            } else if (s == TCSLOG_STATUS_READ_TRUNCATED) {
                printf("lost %lu file(s); %u byte(s) recovered\n",
                       (unsigned long)result.lost, result.n);
            } else if (s != TCSLOG_STATUS_SESSION_END) {
                fprintf(stderr, "read: %s\n", tcslog_status_str(s));
                tcslog_read_close(r);
                return 1;
            }
        }
        tcslog_read_close(r);
        return 0;
    }

A longer one, with a ``send`` callback that takes each filled segment
file out of the log the way a vehicle would, is
``tcslog-c/examples/downlink.c`` in the repository.
``bin/run-capi-example`` builds it and runs it against a temporary
directory.

Theory of Operation
===================

What a segment file carries
---------------------------

Everything in this section rests on three things each segment file
records about itself, beyond the telemetry:

Which session it belongs to
    Every segment file of one session names that session, so a reader
    sees one session end and the next begin by watching this change.

Where it sits in its session
    A count starting at zero for a session's first file and rising by one
    for each file after it. Because it is a dense count, a jump in it is
    proof that files between the two are gone. The identifier in a file's
    name cannot do this job: it is the time the file was created, so it
    rises but skips unpredictably, and no gap in it means anything.

How much of the first record it holds is owed to an earlier file
    A record can be larger than a segment file, so a file may open part
    way through one. This count says how many bytes of that record are
    still to come, starting at the beginning of this file. It is zero
    when the file opens with a fresh record. This is what lets a reader
    both continue a record across files and, when recovering, skip past
    the unusable front of one.

The writer
----------

``LogWrite::new`` hands over whatever segment files it finds, so the
library stops accounting for their storage, and then starts a session by
creating that session's first segment file. Each new file is named for
the time it was created, as read from the clock described in `The clock a
writer keeps`_. If a file of that name already exists -- which means two
were created within one tick of that clock -- the writer sleeps for twice
the clock's resolution and tries again; since the clock never goes
backwards, the next reading is larger than every previous one, so the
retry terminates quickly. What it does when the resolution it was given
is too short for that is `Learning the timer resolution`_.

``write`` appends the record's bookkeeping and then its payload. The
record goes in wherever the current file has room for even one byte of
it, and the file rolls only when it is exactly full. That is what makes
every segment file but a session's last exactly ``seg_size_max`` bytes:
nothing is padded, because nothing needs to be.

There is one exception, and it matters. If the current file is already
exactly full when a record is about to begin, the writer rolls *before*
starting the record rather than after. Rolling afterwards would make the
new file claim that its first bytes were owed to an earlier file, and a
reader would then skip, as the unreadable tail of something else, a
record that in fact began right there.

Rolling means: flush and close the file, hand it to ``send``, and create
the next one, recording in it how much of the record in progress is still
owed. A record spanning several files therefore rolls several times, and
``send`` sees each file as it completes.

Dropping a ``LogWrite`` flushes the file being written and hands it to
``send`` if it holds any records, so the last records written are not
stranded in a file nobody was told about.

The clock a writer keeps
------------------------

A segment file's identifier is the time it was created, in nanoseconds
since the UNIX epoch, and a reader relies on those identifiers sorting
into the order the files were written: it collects the files by name,
sorts them, and replays them in that order.

The system's real-time clock cannot be trusted to give identifiers that
sort that way. It is not a rising clock. NTP can step it, an operator
can set it, and a time fix from a ground station can correct it, each of
which can move it backwards. A file created after such a step takes an
identifier below one created before it, and then the reader's two
sources of order disagree: it replays the files in identifier order but
checks them by the dense sequence number in each header. Seeing sequence
2 where it expected 0, it concludes that files are missing. The log is
whole, every record is on disk, and the reader reports segment files
lost and hands the records back out of order. A fault would be announced
the same way, so there would be no telling the two apart.

The monotonic clock has the opposite problem. It never goes backwards,
but it has no epoch: its zero point is an arbitrary moment, different
after every boot, and the standard library offers no way to read a value
out of it. It can say how much time has passed and nothing else, so it
cannot name a file and cannot be a timestamp.

A writer therefore keeps a clock made of both. When it is constructed it
reads the real-time clock once and takes a monotonic reading alongside
it, and keeps the difference between them -- the distance from the UNIX
epoch to that monotonic reading. Every time it needs afterwards, for a
segment identifier or a record timestamp, is that difference plus
however far the monotonic clock has advanced since. The result is
nanoseconds since the UNIX epoch, exactly as before, but its ordering
comes from a clock that cannot step.

Two consequences follow, and both are deliberate.

The real-time clock is read once per writer, so it has to be right by
then. That is the requirement in `The real-time clock must be set`_. A
clock set after a writer was constructed, or corrected afterwards, does
not affect the times that writer records: it goes on measuring from the
epoch it captured. This is what makes a step harmless, and it is equally
what makes a late correction ineffective. Opening a new ``LogWrite``
takes a fresh reading; ``clear()`` deliberately does not, since a session
begun after a backward step would otherwise take identifiers below those
of files already in the directory.

Each writer anchors on its own reading, so a backward step between two of
them would leave the later writer behind the identifiers the earlier one
minted. What the later writer has to go on is those identifiers
themselves: they are the only surviving record of the earlier clock. It
therefore takes the highest identifier in the directory -- from the same
scan that hands the pre-existing files to ``send`` -- and, if its own
clock does not already read past that, moves its epoch forward until it
does.

The epoch moves once rather than each identifier being clamped. A clamped
clock would mint the same value over and over until real time caught up,
and since a file of that name exists already, the writer would retry for
as long as that took. Moving the epoch instead leaves a clock that
advances normally from its new starting point.

This covers the files still in the log's directory, which is all a reader
of that directory can see. It does not cover files ``send`` has already
taken: the new writer has no record of their identifiers, so if the
real-time clock was stepped backwards after they were shipped, what
received them can still be given later files with earlier identifiers.
Only the real-time clock being correct from the start prevents that,
which is the requirement above.

On Linux the monotonic clock is ``CLOCK_MONOTONIC``, which NTP slews but
never steps. A writer's clock therefore follows real time's *rate*,
staying as close to it as a slewed clock is, while being immune to its
jumps -- so the times recorded do not drift away from real time over a
long run. ``CLOCK_MONOTONIC`` does not advance while the system is
suspended, so a writer that outlives a suspend records times short by
however long it lasted; the standard library exposes no clock that counts
suspended time.

Learning the timer resolution
-----------------------------

The retry above rests on the supplied ``TIMER_RESOLUTION`` really being as
coarse as the clock. That figure is awkward to establish: it is a property
of the machine, the standard library does not report it, and the sleep it
is used for is bounded only loosely -- a ``thread::sleep`` of a stated
length may return later than asked, and on Linux how much later is set by
the scheduler's timer slack rather than by any clock granularity. A value
that has to be right would be a poor thing to ask a caller for.

So the value is treated as a first guess, and a writer corrects it. One
collision is ordinary: it is the event the resolution exists to resolve,
and the wait that follows is expected to clear it. A second collision
while naming the same file is different -- the wait did not clear it, so
the value is too short for this machine. The writer doubles the
resolution, waits again, and doubles it again for every further
collision, until an attempt finds a free name.

What it arrived at is kept and used from then on, so a log pays the cost
of learning it once rather than at every roll. It only ever grows: a
resolution large enough to break the tie once is large enough next time,
and letting it decay would re-learn the same thing at the cost of another
collision.

``timer_resolution()`` reports the figure in force. A value above the one
supplied says the supplied one was too small, and is the figure to give
the next build -- which is the practical way to arrive at a number for a
machine whose resolution is unknown: start at 1, run, and read it back.

Each widening is also announced through the
``timer_resolution_adjusted`` callback, so a caller need not poll for it.
What to do about it is the caller's choice, and `Callbacks:
WriteCallbacks`_ sets out the two usual answers.

Doubling cannot rescue a resolution of zero, which stays zero however
often it is doubled. That is why ``LogWrite::new`` refuses that value
outright, with ``TimerResolutionZero``, rather than leaving the retry to
discover that it cannot widen its way out -- and why an unset
``TIMER_RESOLUTION``, which leaves zero behind, is refused there rather
than corrected here.

The cost of filling every file
------------------------------

Letting a record begin wherever there is room means a record's
bookkeeping can straddle a boundary, with its payload length split across
two files. This is a deliberate trade, and it has a price worth stating
plainly.

If the earlier of those two files is lost, that record cannot be
recovered even though its payload bytes may be perfectly intact: the
surviving tail of a split length cannot be told from the tail of a longer
one, so there is no way to know where the payload begins. Losing one
segment file therefore costs the records it held and, if a record's
bookkeeping straddled its boundary, that one record as well.

What is bought is that a segment file's length means something. Every one
but a session's last is exactly ``seg_size_max``, so the storage a log
occupies follows from the file count alone, and a short file is
unambiguous evidence of damage rather than something the writer does in
the ordinary course of events.

When a write fails
------------------

If writing to a segment file fails, the file is given up rather than
retried: its contents can no longer be trusted. It is closed and handed
to ``send``, a fresh file is opened so the next write has somewhere to
go, and the original error is reported to the caller. The record being
written is abandoned, so the new file begins with a fresh record. A
failure to create that replacement is itself reported, and the next
``write`` tries again.

The upshot for a caller is that a failed ``write`` is not the end of the
log. The error says that record did not make it; the next call carries
on.

The reader
----------

``LogRead::new`` lists the directory, keeps the files whose names match
the prefix and suffix, and sorts them by the identifiers in those names.
Since an identifier is the time the file was created, that sorts them
oldest to newest. Nothing is opened yet.

The reader keeps one piece of state that drives everything below: a flag
saying "find a record boundary before reading anything." It is set when
the reader is built, so the very first read takes the same path as a read
recovering from damage. There is one way into the telemetry rather than
two, and the case of a log whose opening files are already gone is
handled on the ordinary path instead of needing its own.

Each read then:

1. reports the end of the log if nothing is left to read;
2. if the flag is set, finds the start of the next whole record, and
   clears the flag;
3. reads the record's bookkeeping, if the format has any;
4. reads that many payload bytes into the caller's buffer, crossing into
   further segment files as needed;
5. if the record was longer than the buffer, skips the rest of it, so
   that the next read starts at the following record.

Before a segment file may be used
---------------------------------

A file must satisfy all of the following, or it is passed over:

- it opens, and its length can be obtained;
- it is at least long enough to hold a header;
- it identifies itself as a Tcslog segment file;
- its stored format version is one this build can read;
- its record format is one of the defined ones;
- the identifier stored in it matches the one in its name, which catches
  a file that has been renamed or copied;
- it is no larger than the maximum size it declares for itself.

A file failing any of these is skipped silently. No error reaches the
caller for the file itself and no distinction is drawn between the
reasons, because to a reader a file it cannot use is a file it does not
have. The loss surfaces the same way a deleted file's does, through the
gap in the count that the surviving neighbours show. A corrupt segment
file and a missing one are therefore reported alike, which is the
behaviour a caller can actually rely on.

Crossing from one segment file into the next
--------------------------------------------

A record may span files, so mid-record the reader crosses into the next
one. Before consuming a single byte of it, two things must hold.

**The new file must owe exactly what is outstanding.** If the reader has
already taken some of this record from earlier files, the new file must
say that precisely the rest is still owed. If it has taken none -- the
previous file ended exactly at a record boundary -- the new file must say
it owes nothing, marking a fresh record. Any other value means this is
not the continuation of the record in hand: either the tail was in a file
that is gone, or what is stored has been corrupted.

**The count must step by one.** The new file's position within its
session must be exactly one more than the previous file's. This check is
needed *in addition* to the first, and the reason is worth spelling out.
The first check is blind to a file lost exactly on a record boundary: in
that case both surrounding files say they owe nothing, which is just what
the reader expects, so the crossing is accepted and whole files' worth of
records vanish with no error raised. The count catches it, and says how
many files went.

One crossing cannot be checked the first way, and it is the one the
writer does produce: a crossing part way through a record's bookkeeping,
before the reader knows how long the record is. It must not be waved
through -- the bytes after the boundary would be spliced onto the partial
bookkeeping and whatever they decoded to returned as a record, in
practice something dated decades away carrying bytes that were never one
record. So the check is deferred rather than skipped. Every byte taken so
far is bookkeeping, so the rest of it is known, and the new file must owe
at least that much; whatever it owes beyond that is the payload, which
fixes the payload length exactly. The reader then compares that against
the length the completed bookkeeping decodes to, and rejects the crossing
if they disagree, or if a second crossing inside the same bookkeeping
implies a different length.

This last check is weaker than the others, and is not written as though
it were not. Every other crossing is checked against a total learned
before the damage, which is evidence from the intact side. This one
compares a value derived from the damaged side against bookkeeping
decoded from the damaged side, so bookkeeping that happens to decode to
exactly the implied length is accepted. That is a numeric coincidence
rather than anything an intact log can produce, but it is not the
structural impossibility the other checks rest on.

When a crossing is refused, the reader gives the file back to its pending
list so that it will be re-examined as the start of a fresh record, sets
the resynchronize flag, and reports ``ReadTruncated`` with the number of
files missing and the number of payload bytes that did reach the caller.

Losses ahead of a session
-------------------------

Both checks above happen at a crossing, so neither can see files lost
*before* the first surviving file of a session: nothing crosses into it,
and what it says it is owed describes a record whose opening files are
gone. The reader therefore also checks that the first file it opens for a
session sits at position zero. A non-zero position is the count of files
lost ahead of it, and is reported before any record of that session is
handed back. The file itself is sound, so it is kept and the records that
did survive are still returned by later reads.

Without this check, a reader handed a session whose opening files were
lost would begin part way through it and report nothing at all.

Resynchronizing
---------------

With the flag set, the next read finds the start of the next whole
record. It opens segment files in turn until one offers a record
boundary:

- If what a file says it is owed covers its whole data section, every
  byte of it belongs to a record that began in a file which is not
  available. No fresh record can be found there, so the file is given up
  and the search moves on.

- Otherwise, the reader skips past the bytes the file says are owed --
  the unrecoverable tail of an earlier record -- and the byte after them
  begins a fresh record. A file that is owed nothing is not a special
  case: skipping nothing leaves the position exactly at the fresh
  record's first byte.

Both of those give up the record that continued into the file. What the
file says it is owed tells the reader how many of that record's bytes are
still to come, but not where the record started, and without its
bookkeeping a whole payload cannot be told from the tail of one.

Recovering a record whose bookkeeping was lost
----------------------------------------------

There is one case where arithmetic settles it, and the reader tries it
before giving up. Suppose exactly one segment file is missing, the file
after it continues a record, and that record's length was known before
the gap, because its bookkeeping was read. Then:

- the missing file held a full data section, since a record only carries
  over into the next file when the current one is exactly full;
- the interrupted record owed a known number of bytes; subtract those
  from the data section size, and what is left is the room the gap had
  for records of its own;
- if that room is exactly the size of one record's bookkeeping, the gap
  held the bookkeeping of the record continuing into this file and
  nothing else of it -- no payload byte of it, and no complete record
  ahead of it, because even an empty record costs bookkeeping of its own.

In that case the record's payload begins at the first byte of this file's
data section, and its length is what the file says it is owed. The record
is returned normally, and the reader is then already at the next record
boundary.

Any other value for that room leaves a choice between payload bytes lost
from the front of the continuing record and whole records lost ahead of
it, and nothing stored settles that, so the record is given up as
described above. Room smaller than one record's bookkeeping is one of
those values: the gap held the front of some bookkeeping whose tail
begins this file, and neither piece can be read without the other.

This recovery applies only to ``VariableSimple``, whose bookkeeping holds
nothing but the payload length that the "owed" count supplies.
``VariableTsRc`` also carries a timestamp and a record count, which
cannot be reconstructed, and ``Fixed`` has no per-record bookkeeping for
a gap to swallow.

Progress is guaranteed
----------------------

Wherever the reader arms the resynchronize flag, the next read makes
progress. Finding the next record start either opens a file successfully,
clearing the flag and yielding the next record, or runs out of files and
reports the end of the log. No error can leave the reader positioned
inside a file such that the next read produces nonsense, and no error
leaves it unable to continue. A caller may therefore treat
``ReadTruncated`` as news about the telemetry rather than as a failure of
the reader, and simply read again.

What a caller should do with each outcome
=========================================

Reading a log produces one of six outcomes. What each asks of a caller
follows; that the reader is always fit to continue is the subject of
`Progress is guaranteed`_.

``Ok``
    A whole record. Use ``n`` bytes of the buffer.

``SessionEnd``
    Writing was interrupted here, deliberately or by a fault. Record
    numbering restarts after this point, so anything downstream counting
    records should be told. Read again for the next session's first
    record.

``ReadTruncated``
    Telemetry was lost. ``lost`` files are missing, or the damage was
    inside a file that survived if it is zero. The first ``n`` bytes of
    the buffer are real telemetry from the record that was cut short and
    may be used, marked so they cannot be mistaken for a whole record.
    Read again; the reader recovers.

``ReadOverflow``
    The record did not fit. The first ``n`` bytes of the buffer are the
    front of it; the rest is gone. If this happens, the buffer is too
    small for this log. Read again for the next record.

``Eof``
    The log is finished.

``IoError``
    The storage itself failed. Read again: the reader gives up the file
    it was on and resynchronizes. Persistent errors from every file mean
    the storage, not the log, needs attention.

Taken together, these reduce to one rule: read again until ``Eof``. Every
other outcome is news about the telemetry -- a session boundary, a loss, a
buffer too small, a bad sector -- and none of them is a reason to stop
reading or to do anything to the reader first. A caller can write one loop
that reads until the log is finished and treats everything else as an
annotation on the records it got.
