=============
TcsLog Prompt
=============
.. contents:: Table of Contents
   :depth: 4
   :local:

Introduction
============
.. note::

    This was being used to recreate everything from scratch, but the
    approach will now switch to deltas, instread

Create a Rust library named Tcslog for onboard logging of telemetry data
for systems such as as spacecraft and autonomous underwater vehicles that
must store telemetry onboard until opportunies arise for transmission.

The deliverable is a Cargo workspace of two crates: ``tcslog``, the
library, and ``tcslog-gen``, specified under "Support Binaries". The
library also carries the ``sample`` example specified there. The
workspace shares one version, which is the version of the on-disk format
described here. The build-time timer resolution is supplied to every
crate as described under "Segment IDs".

Two further binaries, ``tcslog-dump`` and ``tcslog-dumphdr``, are
specified under "Support Binaries" but are not part of this workspace.
They are the ``tcslog-tools`` crate in its own repository, because they
only read a log: they take the library with its ``write`` feature off and
so need no timer resolution, and someone who only wants to read a log
should not have to build the writing side. The error-recovery suite
drives them from there through ``bin/tcslog-tool``.

It can be used in conjunction with live transmission of telemetry data
to ensure data from corrupted live transmission can be recovered. It
divides log storage into segment files to allow downlinking in small
batches and to easily skip data that has already been downlinked. 

In the event that some of the data stored onboard cannot be read, the format
of the the segment files allow skipping corrupted data and resynchronizing
with the telemetry data boundaries on a segment file granularity.

Data Storage
------------
When creating a Tcslog log, the maximum number of bytes that will be stored
onboard is specified, allowing confidence that the log data will not fill
available storage. Each segment file is limited to seg_size\ :sub:`max` bytes.
When that limit is reached, the segment file is passed to a user-defined
function and responsibility for managing that storage transitions to
user-defined code.

.. note::

    The maximum number of bytes is the sum of telemetry data and headers
    for segment files and data records. A small amount of extra space is
    generally required for file metadata, which may depend on file name
    size, segment file size, etc.

The Tcslog approach differs from approaches using the Linux logrotate
utility as logrotate is not strictly tied to the actual storage used,
relying instead of periodic checking.

Since error recovery is done on a segment file basis, the smaller the
segment file, the less telemetry data will be lost.

Every segment file but the last one of a session is exactly
seg_size\ :sub:`max` bytes. The storage a log occupies is therefore the
number of segment files multiplied by that size, give or take the last
one, with no per-file shortfall to account for.

There are several log formats, trading storage efficiency for automatic
recording of meta data.

Segment File Names
------------------
In addition to the value of seg_size\ :sub:`max` supplied during log file
creation, two more parameters are
specified when creating the tcslog interface: prefix and suffix. These are
strings that, respectively, appear at the beginning and end of the
name of the segment file.

Sandwiched between the prefix and suffix is a string corresponding to the
segment file ID. The segment file ID is an unsigned integer value. To
convert it to the string used in the segment file ID, it is converted to
a zero-filled hexadecimal string, where the alphabetic characters are all
in lower case. A dash ('-') is inserted after each group of four characters
in the converted string. For example, if the segment file ID is
0x1234abcd5678efabu64, the segment file name will
be the prefix, followed by the string "1234-abcd-5678-efab", ending with
the suffix.

The prefix and suffix uniquely identify a Tcslog log file. The segment file
IDs gradually increases as new segment files are created. It may be the
case that Tcslog operations are interrupted deliberately or due to a fault.
In this case, a new session is created and, when the log is being read,
Tcslog will indicated the beginning of a new session. Session boundary
identification is particularly important when record count metadata is
being used as there is no way for Tcslog to determine that records have
been lost and, thus, it is up to user code to determine how to handle
this.

Dynamic Memory Allocation
-------------------------
No dynamic memory allocation is done once a LogWrite::new() or LogRead::new()
function is called, making this well suited for embedded systems with
limited memory.

Telemetry Storage Format
------------------------
There are several formats in which telemetry can be started, ranging from
a highly efficient fixed record size, to a variable record size including
timestamps and record counts.

Customization
-------------
Callbacks are provided that allow for handling segment file as they fill
and require further processing. There is also a provision for calling functions
that can flush data and/or metadata for segment files to ensure that data
is written to stable storage, such as flush().

Buffering
---------
I/O is generally buffered but the flush() function can be used to ensure
data is written to storage.

File Format
===========
All segment files start with a header, followed by a data section. The 
data section consists of data records, which may be broken across the
data sections of two or more segment files.

Data records consist of a data header, possibly of zero length, followed
by some number of bytes of telemetry data. Some segment files formats
permit the number of bytes of telemetry data in a data record to be zero,
some do not.

Data records may be broken across sequential segment files to allow all
segment files to have a uniform size of seg_size\ :sub:`max` bytes.

Segment Header Format
---------------------
The segment file header length is the same for all formats, and is 53
bytes. The total length of a segment file must
be less than or equal to seg_size\ :sub:`max`.

The fields are packed in this order, with no padding between them, at
these offsets and lengths in bytes::

    offset  length  field
         0       8  type
         8       4  version
        12       8  segment ID
        20       8  session ID
        28       4  max size
        32       8  remaining
        40       1  data format tag
        41       4  data format argument
        45       8  sequence

The format tag is one byte and the argument that follows it is the n of
Fixed, unused by the other formats. The segment file header contains the
following:

type
    This is an ASCII string that identifies this as a TcsLog file. It has the
    value "tcslogsf". This must be the first data in the file.

version
    This is a four-character ASCII string. All characters must be in the range
    from '0' to '9'. The first two characters are the major version number, the
    next character is the minor version number, and the last character is the
    patch number. This must follow the type field.

    A given version of Tcslog can read a segment file whose major version
    number equals its own and whose minor version number is less than or
    equal to its own. Major versions must match: a differing major means
    the layout itself differs. A greater minor version means the file may
    use something this build does not know about, so it is refused, while
    a lesser or equal one is readable by the usual software convention. A
    character outside '0' to '9' anywhere in the field is refused as well,
    since a field that does not parse cannot be compared. The patch number
    takes no part in the decision. A file this build cannot read yields
    VersionMismatch.

segment ID
    Segment ID for this segment file. This must match the segment ID portion
    of the segment file name. This is an u64 value. This is provided to be
    able to recover from accidental file renaming.

session ID
    The session ID is the segment ID of the first segment file created for
    a session. The segment ID of the first segment file appears in the
    session ID field of all segment files for that session. This allows
    detection of the beginning of a new session because the new session will
    have a different segment ID.

max size
    The maximum size, in bytes, of a segment file. This is seg_size\ :sub:`max`.

remaining
    The number of bytes at the start of this segment's data section that
    are the tail of a data record whose first byte was written to an
    earlier segment file. Zero when the data section starts with a fresh
    record -- the common case for the first segment of a session and for
    every segment whose predecessor ended exactly at a record boundary.
    May be greater than the size of the data section, in which case the
    continuing record extends into one or more later segment files and
    no fresh record begins in this segment.

    The count covers every byte of that record still to come, its data
    header included. A header that straddles the boundary has its
    leading bytes in the earlier segment file and its tail among these,
    so some of the bytes counted here may be header rather than payload.

    The reader uses this field to locate the start of the first fresh
    record in a segment it has just opened during resync: the first
    ``remaining`` bytes of the data section belong to a record that
    began in an earlier, possibly missing, segment file and cannot be
    reassembled from this one alone, so they are skipped; the next byte
    begins a fresh record. That holds however the earlier record was
    split, which is why the skip needs no knowledge of the format.

data format
    Several formats are supported for storing data, which have different 
    tradeoffs for
    
    o   storage efficiency
    
    o   allowable telemetry data length

    o   support for automatically supplied metadata

    All formats allow data records to be split across the data section of
    multiple segment files, so completed segment files will generally contain
    seg_size\ :sub:`max` bytes. However, system failures or resources
    limitations may cause shorter segment files to be produced.

    Supported data formats are:

    Fixed(n)
        All records must have n bytes.
        The value of n must be at least one and less than or equal to
        RecSize.MAX. This is the most compact storage format, at the price of
        having to use fixed-length telemetry data records.

        The data header for this format is of zero size. The value of
        the ``remaining`` field in a segment file's header is the number
        of bytes at the start of that segment's data section that belong
        to a record whose first byte was in an earlier segment. If the
        record whose bytes occupy the start of the data section began
        exactly at that data section (a fresh-record boundary), or if
        the segment starts a new session, ``remaining`` is zero.
        Otherwise, letting ``offset`` denote the number of bytes of the
        continuing record that were already written to earlier segments,

            remaining = n - offset

        and after the reader skips those ``remaining`` bytes, the next
        byte of the data section is the first byte of a fresh record.

    VariableSimple
        Records may have from zero to RecSize.MAX bytes. This will generally
        used when the telemetry data being stored already contains a
        timestamp.

    VariableTsRc
        Records may have from zero to RecSize.MAX bytes.
        Each data record will be accompanied by a timestamp, which is a
        nanosecond-resolution, 64-bit offset from the UNIX epoch. This
        value will be returned when data is read.

sequence
    Zero-based index of this segment file within its session. Resets to
    zero for the first segment of a new session and increments by one
    for each subsequent roll. Stored as a u64, so the counter cannot
    realistically overflow during a single session.

    Rationale: segment IDs are wall-clock timestamps, not a dense
    integer sequence, so they cannot be used to count segments or
    detect gaps. The sequence field is the canonical dense counter,
    and it plays a distinct role from the remaining field in
    loss detection.

    Without a sequence field, the reader's only continuity signal is
    remaining. That catches losses that fall inside a data record --
    the surviving segment's remaining will not match the outstanding
    byte count owed to the in-progress record -- but it is blind to
    losses that fall on record-aligned segment boundaries. In that
    case the segment before the gap ends with the reader owing zero
    bytes, and the segment after the gap has remaining = 0, which is
    exactly what the reader expects; the reader continues silently
    and the records that lived in the lost segments vanish with no
    error raised.

    With a sequence field, the reader also checks that the new
    segment's sequence equals the previous segment's sequence plus
    one. A jump makes the gap visible even when remaining agrees on
    both sides, and it reports how many segments were lost.

    Both of those checks run at a crossing, so neither can see
    segments lost before the first segment of a session that
    survives: nothing crosses into it, and its own remaining field
    describes a record whose opening segments are gone. The reader
    therefore also checks that the first segment it opens for a
    session has sequence = 0. A non-zero value is the count of
    segments lost ahead of it. Without this check a reader handed a
    session whose opening segments were lost would begin part way
    through it and report nothing at all.

    The three checks together upgrade the reader's guarantee from "no
    in-progress record was silently truncated" to "no segment in the
    session was silently dropped."

    The field also lets recovery tools reassemble a session by
    session_id + sequence when file names have been changed, since
    segment file names carry the timestamp-based segment ID and are
    not reliable if the files have been renamed or copied.

Data Header Format
------------------
The data section consists of alternating data headers and telemetry data, which
are written sequentially to the data sections of segment files. The size of the
data section, in bytes, is:

    seg_size\ :sub:`max` - size of segment header

There are multiple types of data headers, depending on the format specified in
the segment file header. Each is packed with no padding, and its fields
appear in the order given below, so the sizes are zero bytes for
Fixed(n), four for VariableSimple, and twenty for VariableTsRc:

    Fixed(n)

        The data header is zero length, i,e. each telemetry data record is
        logically continguous with the preceeding telemetry data record.

    VariableSimple

        There is one field in the data header or this format:

        n

            Number of telemetry data bytes in the data record.

    VariableTsRc

        The data header type has the same initial fields as the VARABLE_SIMPLE
        format, plus the following fields:

        timestamp

            Offset from the UNIX epoch with nanosecond resolution, represented
            as a Timestamp value.

        record count

            The record count is one for the first record of a session
            and increments by one for each record after it. It is of
            type RecordCount, a u64. The count belongs to the session,
            not to the log: a new LogWrite starts a session, and so does
            the first write() after clear(), each beginning again at
            one. That is why a reader must report a session boundary
            before handing back the records that follow it.

Segment IDs
-----------
The segment ID is the time since the UNIX epoch, with nanosecond resolution.
When a segment file is created, the current time is read and the prefix and
suffix added to produce the name of a segment file. Tcslog attempts to create
a new segment file with name. If the file already exists, it sleeps, then
gets a new current time and tries to create the file again.
This ensures that it
will quickly find an unused segment ID.

The current time is not read from the real-time clock each time. The
real-time clock does not increase monotonically: NTP, an operator, or a
time fix from the ground can step it backwards, and a segment file created
after such a step would take an ID below one created before it. A reader
replays segment files in ID order but validates them by the dense sequence
number in each header, so the two would disagree and it would report an
intact log as one missing segment files.

A LogWrite therefore reads the real-time clock exactly once, when it is
constructed, and keeps the difference between that reading and a monotonic
clock reading taken alongside it. Every segment ID and record timestamp it
mints afterwards is that difference plus however far the monotonic clock
has advanced since: nanoseconds since the UNIX epoch, as before, but
ordered by a clock that cannot step. clear() does not re-read the
real-time clock, since a session begun after a backward step would
otherwise take IDs below those of files already in the directory.

Because the real-time clock is read once, it must hold the correct time
before any Tcslog function is called; a correction arriving later does not
reach the IDs already minted. Most systems leave an unset real-time clock
at the UNIX epoch or before it, so LogWrite::new() refuses a clock that
does not read later than the epoch, returning ClockError without creating
the log. A clock that is set but wrong cannot be detected and is not
checked for.

Each LogWrite anchors on its own reading, so a backward step of the
real-time clock between two of them would leave the later one behind the
IDs the earlier one minted. The IDs in the directory are the only record
of the earlier clock, so LogWrite::new() takes the highest of them -- from
the same scan that hands the pre-existing segment files to the send
callback -- and moves its epoch forward if its own clock does not already
read past that. The epoch moves once; clamping each ID to one past the
highest would instead mint a single value until real time caught up, and
the retry above would spin on the file that already has that name. This
covers the files still in the directory, not files the send callback has
already taken.

By sleeping for twice the timer resolution, the next time the writer's
clock is read, it must be greater than the previous value. Since that
clock never decreases, it must be greater than any previous time of this
writer and so is unique.

The sleep time is twice the system-dependent time resolution, used in the
Rust thread::sleep() function. This value is named
TIMER_RESOLUTION and is specified in nanoseconds.

TIMER_RESOLUTION is a starting point, not a figure that has to be right.
It is hard to establish from outside: nothing in the Rust standard
library reports it, and what thread::sleep() actually waits for a given
duration is bounded only loosely -- on Linux by the scheduler's timer
slack rather than by any stated clock granularity. So a writer corrects a
value that proves too small. One collision while naming a segment file is
ordinary, being the event the resolution exists to resolve. If the sleep
that follows does not clear it -- a second collision on the same file --
the value is too short for this machine, so the writer doubles the
resolution and sleeps again, doubling once more for each further
collision, until an attempt finds a free name. The value it arrives at is
kept and used from then on, so the cost of learning it is paid once
rather than at every roll, and it only ever grows.

LogWrite::timer_resolution() reports the value in force. One above the
value supplied says the supplied one was too small and is the figure the
next build should be given, which is how a number is arrived at for a
machine whose resolution is unknown: start at 1, run, and read it back.
Zero is the one value this cannot correct, since doubling it leaves it
zero, which is why LogWrite::new() refuses it.

TIMER_RESOLUTION must not be defined in the code proper but is supplied
from outside as an environment variable, either from .cargo/config.toml
(see .cargo/config.toml.example) or on the command line.
The tcslog/build.rs file is then used to define it in the code.
Unparseable and zero values will result
either in a compile error or an error from LogWrite::new(). There is no
default value for TIMER_RESOLUTION.

Operations
==========
Tcslog supports two broad categorie of operations: reading and writing.

Write-Related Operations
------------------------

Initialization for Writing
~~~~~~~~~~~~~~~~~~~~~~~~~~
Call the user function send() for all existing segment files. Then
create a new segment file, called the current segment file.

Writing Telemetry Data
~~~~~~~~~~~~~~~~~~~~~~
When the user calls an appropriate write function with telemetry data, the
telemetry data is logically appended to the data header to form a logical data
record. The data header
depends on the format and may be zero length.

A logical data record's data header may be split across two segment
files. The writer starts a record wherever the current segment file has
room for even one byte, and rolls only when the file is exactly full, so
every segment file but a session's last is exactly
seg_size\ :sub:`max` bytes. No padding is ever written, because none is
needed to reach that size.

The cost is paid when a segment file is lost or cut short part way
through a header: the payload length is spread across two files, and the
surviving tail of a little-endian length cannot be told from the tail of
a longer one, so the reader cannot find where the payload begins. That
record is lost even though its payload bytes may be intact, and the
reader must report the loss rather than return anything for it. Losing
one segment file therefore costs the records whose bytes it held and, if
a header straddles its boundary, the one record that straddled. A
uniform file length is worth that: it lets a caller compute the storage
a log occupies from the number of segment files alone, and it makes a
short file unambiguous evidence of damage rather than something the
writer does in the ordinary course of events.

If writing the remaining bytes in the logical data record would cause the segment
file to grow longer than
seg_size\ :sub:`max`
write enough bytes from the logical data record to grow the segment file to
seg_size\ :sub:`max`
bytes. Then, close the current segment file, call the send() function, 
and create a new segment file.

If there are too few bytes remaining in the logical data record to grow
the segment file larger than 
seg_size\ :sub:`max`
bytes, append all remaining bytes from the logical data record to the
segment file.
and return to the caller. Do not close the current segment file, call
send(), or create a new segment file.

Error Handling
^^^^^^^^^^^^^^
If an error happens when writing to the current segment file, close the
segment file, call send(), and create a new segment file. Errors
occuring during creation of a new segment file terminate the write
operation and propogate to the caller.

Segment File Creation
~~~~~~~~~~~~~~~~~~~~~
When there current segment file fills, i.e. its length is seg_size\ :sub:`max`,
the file is closed and the send() function is called with the
name of the file.

When send() returns there must not be a file
with the name it was passed. This can mean, among other things:

o   The file was downlinked and deleted.

o   The file was renamed for later downlinking

The important thing is that the space for the former segment file is no longer
being managed by Tcslog.

After these, and possibly other, checks, are made a new segment file is created.
This becomes the current segment file.  After this, a segment file header is
written.
The value in the ``remaining`` field is the number of bytes of the
previous segment's last data record that continue at the start of the
new segment's data section. If the previous segment ended exactly at a
data record boundary, ``remaining`` is zero. A roll must not be
performed while ``remaining`` would otherwise be armed to the full
record size (i.e., before any byte of the current record has been
written to the previous segment); the writer must roll first, then
begin the record in the new segment, so that the fresh-record start
is unambiguously encoded as ``remaining = 0``.

That is the only roll the writer performs before a record. It does not
roll to keep a data header whole: a record begins wherever there is room
for a byte of it, and its header runs into the next segment file if the
current one fills first.

Every segment file but a session's last is therefore exactly
seg_size\ :sub:`max` bytes. A shorter one is either the segment the
writer is still filling, which is the last of its session, or a file
damaged after it was written. A reader must still take the file's own
length as the end of its data section, since it cannot tell those two
apart by length alone; what the length no longer does is describe a
segment the writer closed early of its own accord.

Error Handling
^^^^^^^^^^^^^^
When a segment file cannot be created,
a LogError value specific to the create
operation is returned, containing the io::Error 
code returned by the that operation. Likewise, if the segment
file header cannot be written, a LogError containing an io::Error
is returned.

If an operation related to the contents of a segment file, such as getting
the length (though this should normally be maintained internally by Tcslog),
write, seek, etc. a LogError value identifying that operation and
containing the error returned by that function, should be returned. The file
is closed and the
file name is placed on the send FIFO. The segment ID is incremented
unless it is already SegId\ :sub:`max`.

Deleting a Log
~~~~~~~~~~~~~~
Logwrite::clear() can be called to delete all segment files for a log.
It starts by closing any open segment files, then goes
through all existing segment files in the given directory that match the
prefix, suffix, and all possible segment IDs, deleting each one. Closing
first is what makes the deletion complete: an open segment file cannot be
removed on every supported platform, and one left behind would still read
back as a log.

The send() callback is not called for the files deleted. It exists to
hand a segment file to user code, and these are being discarded.

Afterwards the LogWrite has no current segment file and no log exists in
the directory for a reader to open. The writer stays usable: the next
write() starts a new session, whose first segment file carries sequence
zero and whose records are numbered from one. Continuing the cleared
session's sequence instead would leave a single segment file claiming a
position with nothing ahead of it, which the reader is required to report
as that many lost segment files -- a log that had just been emptied
deliberately would come back as a log damaged by a fault.

Read-Related Operations
-----------------------
The LogRead struct provides for reading data records from a log. It must
tolerate two independent classes of fault:

o   I/O errors when reading the current segment file (bad sectors, an
    unreadable header, a short read).

o   Missing segment files, i.e. a gap in the sequence of segment IDs
    returned by the initial directory scan. A gap may skip a segment
    that held the middle of a multi-segment data record, or it may
    skip whole records that lived entirely within the missing file.

In either case, the reader must discard whatever record was in progress,
resynchronize on the next segment file that opens cleanly, and continue
returning subsequent records. The reader is permitted to return an error
to the caller for the record that was lost, but the reader itself must
remain usable: the very next call must recover, not repeat the failure.
The only condition under which reading ends is exhaustion of the segment
file list.

Recovery is coordinated through a piece of LogRead state called the
resync flag, described under "Reading Data Records" below.

Initialization
~~~~~~~~~~~~~~
The read process begins by creating a list of segment files that match
the given prefix and suffix. Each name is parsed back into its segment
file ID and the list is sorted on those IDs. Since the segment file ID
increases with time, the list is consequently sorted from oldest to
newest. Sorting the parsed IDs and sorting the names alphabetically give
the same order, because the ID is rendered as fixed-width zero-filled
hexadecimal; the parsed form is used because it is the order that is
actually meant. Processing starts with the oldest segment file and
proceeds to the newest one.

If the segment file list is empty, an error code is returned indicating that
the log could not be found.

Initialization opens no segment file. It leaves the reader with the
pending list, no current segment file, and resynchronization armed, so
that the first read finds the start of the first data record by the same
path a read recovering from a fault takes. There is one way into the data
rather than two, and the case of a log whose opening segment files are
already gone is handled on the ordinary path instead of needing its own.

Find the Next Data Record Start
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
Finding the next data record starts with asserting that there is no
current segment file, i.e. there may not be an open segment file.

This process starts by trying to open the next segment file, which becomes the
current segment file. If this fails and there are no more items in the
segment file list, an error is returned indicating the log ended prematurely.
If the open fails and there are more items in the segment list, repeat
trying to open the next segment file.

If the next segment file could be opened, perform the usual segment file
header validation. Then:

o   If the ``remaining`` field value is greater than or equal to the
    size of the data section: the entire data section belongs to a
    data record that began in an earlier segment file which is not
    available (either because it was lost, or because we are resyncing
    after a fault), so no fresh record can be located here. Discard
    the segment and go back to trying to open the next segment file.

o   Otherwise the ``remaining`` field value is less than the size of
    the data section: skip past the first ``remaining`` bytes of the
    data section, which are the unrecoverable tail of a record from a
    prior segment, and treat the following byte as the first byte of a
    fresh data record. Note that ``remaining = 0`` -- the fresh-record
    boundary case -- is not special here: skipping zero bytes leaves
    the read position exactly at the fresh record's first byte.

Both of the cases above discard the record that continues into the
segment being resynchronized in. The ``remaining`` field says how many
of that record's bytes are still to come but not where the record
started, and without its data header there is no way to tell a whole
payload from the tail of one whose front is gone. Before applying them,
however, the reader must check the one case where arithmetic settles
it.

Recovering a Data Record Whose Header Was Lost
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
When the resync follows a crossing that was rejected for a gap of
exactly one segment file, and the segment now being examined is the one
that crossing opened, and its ``remaining`` field is non-zero, every
byte the missing segment held is accounted for:

o   The missing segment held a full data section. The segment after it
    continues a data record, so the missing segment ended part way
    through one, and a mid-record roll happens only when a segment file
    has reached ``seg_size_max``.

o   The data record the gap interrupted owed a known number of bytes,
    because its data header was read before the gap. Subtract those
    from the data section size; call the result the room the gap had
    for data records of its own.

o   If that room equals the format's data header size exactly, the gap
    held the data header of the record that continues into this segment
    and nothing else of it: no payload byte of that record, and no
    complete record ahead of it, because even a zero-length record
    costs a data header of its own. The record's payload therefore
    begins at the first byte of this segment's data section and is
    ``remaining`` bytes long. Return it as a normal data record: the
    read position is then already at the next record boundary and the
    resync flag is cleared.

Any other value for that room leaves a choice between payload bytes
lost from the front of the continuing record and whole records lost
ahead of it, which nothing on disk resolves; the record must then be
discarded as described above. Room smaller than a data header is one of
those values: the gap held the leading bytes of a header whose tail
begins this segment, and neither piece can be read without the other.

The recovery is attempted only when exactly one segment file is
missing. The size of a larger gap is no longer the obstacle -- every
segment file but a session's last is exactly ``seg_size_max`` bytes, so
a gap of *k* files held exactly *k* data sections -- but reaching the
record that continues out of such a gap means accounting for the whole
records that lay within it, which this specification does not require.
A reader may implement it; none is obliged to.

This recovery applies only to the ``VariableSimple`` format, whose data
header holds nothing but the payload length that ``remaining``
supplies. A ``VariableTsRc`` data header also carries the timestamp and
record count, which cannot be reconstructed, and a ``Fixed`` log has no
data header for a gap to swallow.


Validating a New Segment File
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
Each time the reader takes the next segment file from its pending list,
it must satisfy all of the following before becoming the current segment
file:

o   The file can be opened and its length obtained.

o   Its length is at least the segment file header length. A file too
    short to hold a header cannot be one.

o   The type is "tcslogsf".

o   The version is one this build can read, by the rule given under
    "Segment Header Format". This is a comparison of the parsed major and
    minor numbers and not a match against the literal string this build
    writes, which would refuse files it is required to accept.

o   The data format tag is one of the defined values, and its argument is
    non-zero when the tag is that of Fixed.

o   The segment ID matches the segment part of the segment file name.

o   The segment file size is less than or equal to the value of max size
    read from the segment file header.

A file failing any of these is skipped and the reader moves to the next
one in the pending list. It is skipped silently: no error reaches the
caller for the file itself, and no distinction is drawn between the
reasons, because to a reader a segment it cannot use is a segment it does
not have. The loss surfaces the way a deleted file's would, through the
sequence gap the surviving neighbours show, so a corrupt segment and a
missing one are reported alike. This is also why no error variant may
exist for these conditions: nothing would ever construct it.

At the end of reading the segment file header, the next position to read
will the beginning of the data section. The new segment file becomes the
current segment file.

Reading Data Buffers
~~~~~~~~~~~~~~~~~~~~
Data buffers are implemented as byte arrays and read from the data section.
If there is no current segment file, the next segment file is opened. If
there is no next segment file, an end of log indication is returned,
otherwise, the segment file header is read, propagating any errors.

As many bytes as are required to fill the data buffer are then read from the current location in the segment file.
If an error occurs, an error code is returned indicating the error and the
number of bytes successfully read into the beginning of the data buffer.
If the data buffer is not filled, the current segment file is closed, a
new one opened--propagating any errors--and reading to the data buffer
continued until the entire data buffer has been successfully read.

If there are more bytes in the read buffer to be read and either there are no
more segment files available, or there is another segment file available
but its session ID is different from the session ID of the segment file
from which the previously read bytes have been obtained,
an error indicating that the read has been truncated is returned, along
with a ReadResult value.

Segment Boundary Validation
~~~~~~~~~~~~~~~~~~~~~~~~~~~
Whenever a read that spans segment files crosses from one segment file
into the next, before consuming any bytes from the new segment's data
section the reader must confirm the following:

o   The new segment's ``remaining`` field equals the number of bytes
    of the current data record that were in the previous segment's
    tail. Concretely, if the reader has already consumed one or more
    bytes of the current record from earlier segment(s), the new
    segment's ``remaining`` must equal ``total_record_size -
    bytes_consumed``. If the reader has consumed zero bytes of the
    current record -- meaning the previous segment ended exactly at a
    record boundary and this is a boundary crossing rather than a
    mid-record crossing -- the new segment's ``remaining`` must be
    zero, marking a fresh record starting at the new segment's data
    section. Any other value means the new segment does not contain
    the continuation of the current record: either the current
    record's tail was in a segment file that has been lost, or the
    on-disk data was corrupted. (Segment IDs are wall-clock
    timestamps, not a dense integer sequence, so ID-based contiguity
    checks are not meaningful; the ``remaining`` field is the
    authority for in-record continuation.)

o   The new segment's sequence field equals the previous segment's
    sequence plus one. A jump means one or more segments between the
    two were lost. This check is required in addition to the remaining
    check because the remaining check is blind to losses that fall on
    record-aligned segment boundaries: in that case both surrounding
    segments show remaining = 0 and the remaining check silently
    accepts the crossing even though whole segments' worth of records
    have vanished. See the sequence field description under "Segment
    Header Format" for the full rationale. On sequence-jump the
    reader must respond identically to the remaining-field failure:
    close the segment, push its ID to the front of the pending list,
    set the resync flag, and return the read-truncated error.

The number of bytes still owed to the in-progress record cannot be
computed until the current record's data header has been fully decoded,
since Variable* record sizes come from the header itself. A crossing
part way through a header therefore has no owed-byte count to check
against, and that is the one case the writer does produce.

It must not be let through unchecked. The bytes after the boundary
would be spliced onto the partial header and whatever they decode to
returned as a record: in practice a segment identifier read as a
timestamp, giving a record dated decades away carrying a payload of
bytes that were never one. The check is deferred rather than skipped.

Every byte of the record consumed so far is a header byte, so the rest
of the header is its length less what has been read, and the new
segment's remaining field must cover at least that much. Whatever it
holds beyond that is the payload, which fixes the payload length
exactly. An intact log satisfies this by construction, remaining being
the writer's own count of the bytes of this record still to come. The
reader therefore:

o   Rejects the crossing at once if remaining is too small to hold the
    rest of the header.

o   Otherwise records the payload length that remaining implies, and
    checks it against the length the completed header decodes to. A
    disagreement means the bytes that finished the header were not this
    record's, and the crossing is rejected.

o   Rejects the crossing if a second crossing inside the same header
    implies a different payload length.

A rejection is handled exactly as any other failed crossing: the
offending segment is pushed back, resync is armed, and the read-
truncated error is returned. The record is lost and the resync picks up
at the next record the surviving segment can offer, which may be several
records further on.

This check is weaker than the others in one respect, and a reader should
not be written as though it were not. Every other crossing is validated
against a total learned from a header read before the gap, which is
evidence from the intact side of the damage. This one compares a value
derived from the damaged side against a header decoded from the damaged
side, so a garbage header that happens to decode to exactly the implied
length is accepted. That is a numeric coincidence rather than anything
an intact log can produce, but it is not the structural impossibility
the other checks rest on.

Both checks above run at a crossing and so cannot see segments lost
before the first segment of a session that survives: no crossing
reaches it. The reader must therefore also check, when it opens the
first segment it can read for a session, that that segment's sequence
field is zero. A non-zero value means that many segments were lost
ahead of it, and the reader must report the read-truncated error with
that count before returning any record from the session. The segment
itself is sound, so the reader keeps it and the records that did
survive are still returned by subsequent reads.

If the check fails, the reader must:

o   Close the newly opened segment file and push its segment ID back to
    the front of the pending list so it will be re-examined as the start
    of a fresh record.

o   Set the resync flag.

o   Return control to the record-reading layer with an error indicating
    that the read has been truncated, along with a ReadResult value.
    successfully read from the previous segment(s).

The record-reading layer will then, on the next call, honor the resync
flag by invoking "Find the Next Data Record Start" against the pushed-back
segment file, which uses that segment's own remaining field to locate a
fresh record boundary.

Skipping To Data Record End
~~~~~~~~~~~~~~~~~~~~~~~~~~~
In the case where the user supplied a buffer smaller than the length of the
telemetry data, it is necessary to skip over the rest of the data record
to find the segment file and data section data to find where the next
record begins. This is only done when data has been successfully read
into the user buffer, so errors at this stage will not affect the
status returned. If errors are encountered during skipping, however,
the resync flag is set to true to indicate error recovery must occur the next
time a record is
read.

Reading Data Records
~~~~~~~~~~~~~~~~~~~~
The reader owns a boolean resync flag, initialized to true when
LogRead::new() returns. Setting the flag to true on construction ensures
that the very first read finds a valid record start using "Find the Next
Data Record Start" rather than assuming the first pending segment file's
data section already begins on a record boundary.

The read function proceeds as follows:

1.  If no more items remain in the segment file list and no segment is
    currently open, return an end of log indication.

2.  If the resync flag is set, invoke "Find the Next Data Record Start."
    If it returns an error, leave the resync flag set and propagate the
    error to the caller -- the caller may retry, and the reader will
    resume the search on the next call. If it succeeds, clear the resync
    flag and continue.

3.  If the format has a non-zero data header, read it. If any read error
    occurs (I/O error, or a segment-boundary validation failure as
    described under "Segment Boundary Validation"), set the resync flag
    and propagate the error. Do not attempt to interpret partial header
    bytes.

4.  Using the payload length taken from the data header (or the fixed
    size, for Format::Fixed), read the telemetry data into the
    user-supplied buffer. If any read error occurs, set the resync flag
    and propagate the error. On success, if the number of bytes read
    matches the payload length and fit within the buffer, return the
    number of bytes read.

5.  If the payload length exceeded the caller's buffer, there is
    telemetry data remaining for this data record that did not fit. Skip
    to the end of that record. The number of bytes in the data section
    to skip is the difference between the payload length and the buffer
    size. If any error occurs during the skip, set the resync flag but
    do not disturb the ReadOverflow result -- the caller has already
    received valid bytes for this record, and the resync flag guarantees
    the next call will recover.

At every point where the resync flag is set on error, the reader is
guaranteed to make forward progress on the next call: "Find the Next Data
Record Start" either opens a new segment successfully (clearing the flag
and yielding the next record), or exhausts the pending list and returns
end of log. In no case may an error leave the reader pointing at a
position within a segment that would cause the next call to produce
garbage.

Public Data Structures
======================
Tcslog provides several public data structures used for operations on log
files. Some are interfaces, which requires that the user create structures
to implement log operations. Others provide results of various sorts.

Every signature below is the one the code must present, including the
mutability of the receiver: reading and writing both advance internal
state, so those methods take ``&mut self``.

LogWrite
--------
This interface is used for writing to logs. Among its members are:

pub fn new(dir: &str, prefix: &str, suffix: &str, seg_size_max: u32, format: Format, callbacks: WriteCallbacks) -> Result<LogWrite, LogError>

    Begin writing a log, in a new session.

    dir             Name of the directory holding the segment files. It
                    must already exist; this function does not create it.

    prefix          String that is the first part of the segment file
                    names that make up the log file. Must not contain a
                    filesystem delimiter.

    suffix          String used as the end of the segment file name. Must
                    not contain a filesystem delimiter.

    seg_size_max    Maximum number of bytes in a segment file. It must be
                    strictly greater than the segment file header plus one
                    data header for the chosen format. A segment file that
                    could not hold a single data header would leave the
                    writer with nowhere to put a record.

    format          Format for segment files

    callbacks       A structure holding callbacks used at various points
                    in operations

    Every segment file already in dir whose name matches the prefix and
    suffix is handed to the send callback before the new session's first
    segment file is created. A new LogWrite therefore does not append to
    what it finds: it takes responsibility for the older files off the
    library's hands and starts a session of its own.

    The errors are TimerResolutionZero when the build-time timer
    resolution is zero, ClockError when the real-time clock does not read
    later than the UNIX epoch and so has not been set,
    PathDelimiterNotAllowed for a prefix or suffix holding a path
    separator, SegSizeTooSmall for a seg_size_max that is
    not strictly greater than the segment header plus one data header,
    FixedLenMismatch for Format::Fixed(0), InvalidPathname when dir does
    not name a directory, and IoError from directory enumeration, the
    send callback, segment file creation, or the header write.

pub const SEGMENT_FILE_HEADER_LEN: u32

    The length of the segment file header, in bytes. An alias for the
    crate-level constant of the same name, provided here because
    seg_size_max is specified relative to it.

pub fn session_id(&self) -> SegId

    The segment ID of this session's first segment file, which is the
    value written to the session ID field of every segment file in the
    session.

pub fn current_segment_id(&self) -> SegId

    The segment ID of the segment file being written.

pub fn timer_resolution(&self) -> u64

    How finely this writer believes its clock advances, in nanoseconds:
    the build-time TIMER_RESOLUTION to begin with, and whatever the
    widening rule under `Segment IDs`_ has raised it to since. A value
    above the one supplied says the supplied one was too small for this
    machine and is the figure the next build should be given.

pub fn last_meta(&self) -> Meta

    The metadata minted for the most recently written record. For
    Format::VariableTsRc this is how a caller learns the timestamp and
    record count that were stored, since those are generated as the data
    header is built rather than supplied by the caller.

pub fn write_str(&mut self, msg: &str) -> Result<u32, LogError>

    Write the UTF-8 bytes of a string as one record, by calling write().

pub fn write(&mut self, msg: &[u8]) -> Result<u32, LogError>

    Write a byte array to the log file as one data record.

    self            Reference to LogWrite

    msg             Reference to array of bytes to write

    Returns the total number of bytes written, counting the per-record
    data header as well as the payload.

    The record may span segment files. Each time the current file fills,
    the send callback is invoked with its path and a fresh segment file
    is opened.

    When there is no current segment file, which is the state clear()
    leaves behind, a new session is started before the record is built.
    It must happen in that order: starting a session restarts the record
    count, so building the data header first would stamp this record
    with the cleared session's count and then issue that same number
    again to the record after it.

    The user callback function record_complete() is called after all
    bytes have been written. This may flush data if data integrity is
    the priority, otherwise it may do nothing if performance is the
    priority.

    FixedLenMismatch is returned if the format is Fixed(n) and the
    length of msg is anything other than n. A zero-length payload is
    that same error rather than one of its own, since zero differs from
    n like any other wrong length. PayloadTooLarge is returned if
    msg.len() exceeds RecSize::MAX, and also if the payload plus its
    data header would exceed the u32 this function returns -- a count
    that wrapped would understate what was written. IoError and
    ClockError are returned as encountered, along with any error from
    creating a segment file on a roll.

pub fn flush(&mut self) -> Result<(), LogError>

    Flush buffered data for the current segment file to mass storage.
    Returns IoError if the underlying flush fails.

pub fn clear(&mut self) -> Result<(), LogError>

    Remove every one of this log's segment files from the directory,
    including the one being written.

    The current segment file is closed before any name is unlinked. Not
    every platform this library targets permits removing an open file,
    and closing it is also what makes clearing complete rather than
    partial: a log cleared down to its last segment still reads back as
    a log, which is not what a caller reclaiming storage asked for.

    The records in those files are discarded, including a record part
    way through being written. The send() callback is not called for any
    of them. That callback hands a segment file to user code, and these
    files are being thrown away rather than handed anywhere.

    The writer remains usable and is left with no current segment file.
    The next write() begins a new session, whose first segment file
    carries sequence zero and numbers its records from one. Beginning a
    new session is required rather than merely tidy: continuing the
    cleared session's numbering would leave a lone segment file claiming
    a position with nothing before it, which a reader must report as
    that many segment files lost.

    Until that write, current_segment_id() and session_id() still report
    the cleared session's identifiers, which name files that no longer
    exist.

    IoError is returned if directory enumeration or file removal fails.
    The current segment file is closed before either is attempted, so it
    is closed even when the removal that follows fails.

fn drop(&mut self)

    The Drop implementation, not an inherent method. If the current
    segment file has data in the data section, flush it and call the user
    callback function send(). Then proceed with the rest of processing
    drop is expected to do.

    A destructor has no way to report a failure and must not panic, so
    errors from the flush and from send() are discarded here. This is the
    one place in the library where an error is dropped rather than
    returned.

LogRead
-------
The LogRead interface is used for reading from logs. Its members include:

pub fn new(dir: &str, prefix: &str, suffix: &str) -> Result<LogRead, LogError>

    Open an existing log for reading. The directory must already hold at
    least one segment file matching the prefix and suffix.

    dir             Name of the directory holding the segment files

    prefix          String that is the first part of the segment file
                    names that make up the log file. Must not contain a
                    filesystem delimiter.

    suffix          String used as the end of the segment file name. Must
                    not contain a filesystem delimiter.

    The errors are PathDelimiterNotAllowed for a prefix or suffix holding
    a path separator, InvalidPathname when dir does not name a directory,
    NoSegmentFiles when no file in it matches the pattern, and IoError on
    directory enumeration failure.

pub fn read(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError>

    Read the next record's payload into buf.

    self            Reference to LogRead

    buf             Buffer to receive the payload

    If the payload is longer than buf, the first buf.len() bytes are
    copied and ReadOverflow is returned carrying that same count, so the
    caller can tell a filled buffer from a complete record. The rest of
    the payload is discarded so that the following read begins at the
    next record rather than in the middle of this one.

    Eof is returned when the segment list is exhausted, SessionEnd once
    at each session boundary, ReadTruncated when a mid-record gap or
    corruption is found, and IoError on underlying failure. On any
    failure other than Eof, SessionEnd, and ReadOverflow the reader arms
    its resynchronization flag and gives up any partly opened segment, so
    that the next call recovers as described under "Find the Next Data
    Record Start." ReadOverflow is not a failure of that kind: the
    segment stays open and the read position stays valid.

pub fn read_str(&mut self, buf: &mut [u8]) -> Result<ReadResult, LogError>

    Equivalent to read(), named for callers whose payloads are UTF-8
    text. It takes the same byte buffer, since a partially recovered
    payload need not be valid UTF-8.

pub fn iter(&mut self) -> LogReadIter<'_>

    An iterator over the remaining records, yielding Record values. A
    Record owns its payload, so the iterator is a convenience that gives
    up the crate's no-allocation guarantee; read() is what a caller bound
    by that guarantee uses.

pub fn current_header(&self) -> Option<&SegmentHeader>

    The header of the segment file now open, or None whenever none is:
    before the first read, and after a read that returned Eof,
    SessionEnd, or ReadTruncated. A ReadOverflow leaves the segment open.

pub fn segments_opened(&self) -> u64

    The number of segment files the reader has opened. A record spanning
    several segments only ever reports the one it ended in, so this tally,
    not a count of headers seen by the caller, is what describes a log's
    extent.

pub fn collect_opened_headers(&mut self, enable: bool)

    Ask the reader to retain the header of every segment file it opens.
    Retention is off by default and must be, because the headers
    accumulate until taken and a caller that never takes them would grow
    the buffer without bound.

pub fn take_opened_headers(&mut self) -> Vec<SegmentHeader>

    Remove and return the headers retained since the last call. Taking
    them before examining a read's result puts each header ahead of the
    records it carried, and lets a read that ended the log still report
    the segments it opened.

LogError
--------
    Enum used to return error values. Its variants are:

    ClockError

        The real-time clock did not read later than the UNIX epoch when
        LogWrite::new() was called, so it has not been set.

    Eof

        No more data records are available.

    FixedLenMismatch

        The format is Fixed(n) and the payload length is not n. A
        zero-length payload under Fixed(n) is this error. It is also
        returned for a format of Fixed(0), which no log may use.

    InvalidHeader

        The segment file header did not match the on-disk layout: the
        type field is not "tcslogsf", or the format tag is not one of the
        defined values, or it is Fixed with a length of zero.

    InvalidPathname

        The prefix and suffix cannot be combined with a segment file ID
        and a directory name to form a valid path name, or the named
        directory is not one.

    IoError(io::Error)

        An error occurred from an I/O operation.

    NoSegmentFiles

        No file in the directory matches the prefix and suffix, so there
        is no log there to read.

    PathDelimiterNotAllowed

        The prefix or suffix contains a path separator.

    PayloadTooLarge

        A payload larger than RecSize::MAX was supplied to a write, or
        one whose length plus its data header exceeds the byte count
        write() returns.

    ReadOverflow(u32)

        There was too much telemetry data in the data record to fit in
        the supplied buffer. The value indicates the actual number
        of bytes or characters placed in the buffer.

    ReadTruncated { lost: u64, n: RecSize }

        A crossing between segment files found a continuation that does
        not belong after the current one: a gap in the sequence, or a
        remaining field that does not match the bytes still owed to the
        record in progress. The record was cut short and the next read
        resynchronizes.

        lost is how many segment files the sequence shows are missing at
        that crossing, and is zero when the sequence is intact and the
        crossing was refused because the surviving segment is itself
        corrupt or short. n is how many payload bytes of the cut-short
        record reached the front of the caller's buffer; those bytes are
        real telemetry and the caller may use them. It is zero when the
        record was cut short before any payload was reached, which is the
        case while a data header was being decoded and ahead of a
        session's first surviving segment.

    SegSizeTooSmall

        The requested seg_size_max is not strictly greater than the
        segment file header plus one data header for the format in use.
        This must not take any argument.

    SessionEnd

        All records from a previous session have been read and a segment
        file header has been read with a different session ID. The next
        read operation will return the first record of the next session,
        if there is one.

    TimerResolutionZero

        The build-time timer resolution is zero, so segment ID generation
        could not make progress, and doubling zero cannot change that.

    VersionMismatch

        The segment file was written by an incompatible version of
        Tcslog.

    No variant may exist that nothing constructs. A variant for a
    condition the code has chosen not to distinguish is dead code under a
    name that suggests coverage the library does not have: either the
    check that raises it exists, or the variant does not.

ReadResult
----------
    Structure returning the result of a read operation. It has the following
    elements:

    n

        Number of bytes stored in the buffer, of type RecSize. This may
        be less than or equal to the buffer size.

    meta

        Object of type Meta containing format-specific data.

Record
------
    One record's payload together with its metadata, produced by
    LogRead::iter(). It holds meta, of type Meta, and payload, which owns
    its bytes. This is the one public structure that allocates, which is
    why the iterator that yields it is offered alongside read() rather
    than in place of it.

Meta
----
    Enum specifying a Format-dependent result from a read:

    Fixed

        The log file uses fixed-length records.

    VariableSimple

        The record uses a variable length record with no additional metadata

    VariableTsRc(Timestamp, RecordCount)

        This record uses a variable length record with a timestamp
        and record count.

Format
------
    Enum selecting how records are laid out in the data section, with
    variants Fixed(RecSize), VariableSimple, and VariableTsRc as
    described under "Segment Header Format". It provides tag(), the
    single byte stored in the segment header, which is 0, 1, and 2 in
    that order; fixed_len(), the n of Fixed and zero for the others; and
    data_header_len(), the on-disk size of one data header, which is
    zero, four, and twenty bytes respectively.

SegmentHeader
-------------
    The in-memory form of a segment file header, holding segment_id,
    session_id, max_size, remaining, format, and sequence as described
    under "Segment Header Format". It converts to and from the on-disk
    byte encoding and reads from and writes to a stream, rejecting a
    header that does not match the layout. It also reports the data
    section length implied by a given maximum size, so that callers need
    not repeat the subtraction.

Version Constants
-----------------
    The major and minor numbers of the on-disk format this build writes
    and reads are public, so that a caller can report them or refuse a
    log it was not built for without parsing a segment file itself. They
    are the two numbers the compatibility rule under "Segment Header
    Format" compares against.

RecSize, Timestamp, and RecordCount
-----------------------------------
    RecSize is the type used to contain the size of the telemetry portion
    of a data record. For the on-disk format version 0.1.0 it is defined
    to be u32. Timestamp and RecordCount are the types of the two
    VariableTsRc metadata fields, both u64: a Timestamp is nanoseconds
    since the UNIX epoch, and a RecordCount is one for a session's first
    record and one more for each record after it.

WriteCallbacks
--------------
    This structure contains the callback functions used during log
    writing operations. Its members are plain function pointers rather
    than trait objects or closures, so that the structure can be stored
    inline in a LogWrite with no heap allocation and no dynamic dispatch.
    A default is provided whose members do nothing, apart from reporting
    a widened timer resolution on standard error, which suits local
    development; a user storing telemetry for real is expected to replace
    the send member. A literal naming only the members it cares about and
    taking the rest from the default keeps working when a member is
    added.

    record_complete: fn(&mut File) -> std::io::Result<()>

        Called after each data record is written. It is up to the
        implementation what this does. It may flush the given file, do
        nothing, or do something else.

    send: fn(&Path) -> std::io::Result<()>

        Transfer ownership of a segment file from Tcslog to user code.
        Called with the full path of a segment file whose data section
        has filled, and also with each pre-existing segment file that
        LogWrite::new() finds.

        User code may do anything appropriate, such as:

        o   Compress the file and move it to another location

        o   Downlink the file

        o   Notify controllers that the file is now completed.

        Upon return, the file must be deleted or renamed such that there is not
        file with the given path name and that it does not match the pattern
        for any segment file names for this log.

        This function may perform other operations. It may, for example,
        be helpful to flush data to the file in order to reduce the chance
        of corruption due to a system restart.

    timer_resolution_adjusted: fn(u64)

        Called when the timer resolution has been widened, with the value
        now in force in nanoseconds. See `Segment IDs`_ for when a
        widening is concluded. It is called once per doubling, and not
        for a first collision, which computes no new value, nor once the
        value has saturated and a doubling leaves it unchanged.

        It returns nothing, and the writer waits and retries whatever the
        implementation does, so it is a notification rather than a
        decision. That leaves the choice of what a widening means with
        user code, and the two reasonable answers differ.

        A deployed system will usually record the value and continue. The
        widening is the writer correcting itself: the log is unharmed and
        every record is written, so stopping telemetry over it would
        trade a sound log for no log.

        A system under development will usually prefer the opposite. A
        too-small value is a configuration fault, and the easiest place
        to act on it is where it was found, so an implementation that
        panics or aborts here stops the program with the faulty value in
        hand. That is why this is a callback rather than something the
        library decides: the same code must be able to behave both ways.

        The default takes the conservative half, printing a message
        naming the new value on standard error and returning, so the log
        keeps being written and the figure is not lost. The value passed
        is the one to put in TIMER_RESOLUTION for the next build.

SegId
-----
This is the data structure that holds the segment file ID. Segment file IDs
are based on u64 values and are times since the UNIX epoch, so they
increase but are not dense: they cannot be used to count segment files or
to detect a gap, which is what the sequence field of the segment header is
for. What makes them increase is the writer's own clock rather than the
real-time clock, which can be stepped backwards; see `Segment IDs`_.

Using an u64 value as the segment ID assures that a huge number of segment
files can be created. Nanosecond timestamps that fit in a u64 run to the
year 2554, so a log's segment IDs cannot collide within any plausible
mission.

Converting this value into a string yields something of fixed length, with
a value known as SegId::STR_LEN, which is 19: sixteen hexadecimal digits
and the three dashes that group them.

SeqId
-----
The type of the segment header's sequence field, wrapping a u64. It offers
a zero constant for the value that opens a session, a saturating step to
the next value, and conversion to and from little-endian bytes for the
header. The step saturates rather than wrapping, because a sequence that
wrapped to zero would present itself as the first segment of a session.

Free Functions
--------------
    format_timestamp(ts: Timestamp) -> String

        Render a Timestamp as an ISO 8601 UTC date and time with
        nanosecond precision, for example 2026-09-21T16:45:12.123456789Z.
        The conversion is done in the crate rather than through a
        date-and-time crate, so that the dependency set stays minimal.

    record_trailer(payload_len: usize, meta: Meta) -> String

        Render a record's length and its format-specific metadata as the
        parenthesised trailer the binaries print after a payload. Both
        tcslog-gen and tcslog-dump use it, so a record reads the same
        coming out as it did going in.

Support Binaries
================
Three binaries and one example ship alongside the library:
``tcslog-gen`` in this workspace, ``tcslog-dump`` and ``tcslog-dumphdr``
in the ``tcslog-tools`` repository, and the ``sample`` example in the
library crate. The three binaries are what the error-recovery test suite
drives, and that suite compares their output against stored files, so the
output wording specified below is part of the requirement rather than an
illustration of it.

tcslog-gen
----------
Writes a log of generated records, giving a test segment files to
damage. It takes the log directory, prefix, and suffix as positional
arguments in that order, creating the directory if it does not exist,
and accepts:

o   ``-f``/``--format`` ``KIND:LEN`` or ``KIND:MIN..MAX``, where KIND is
    ``fixed``, ``variable-simple``, or ``variable-tsrc``. ``fixed``
    accepts only a single length; the variable kinds accept either form.

o   ``-d``/``--data-size`` bytes, the size of each segment file's data
    section, excluding the segment header. The log is created with
    seg_size\ :sub:`max` of ``SEGMENT_FILE_HEADER_LEN + data_size``, so
    a test states the data section it wants and needs no knowledge of the
    header length.

o   ``-n``/``--number``, the count of data records to write.

o   ``-v``/``--verbose``, which adds the record format and the segment,
    header, and data-section sizes ahead of the records, and the record
    count and root file name after them.

The payload of record *i*, counting from one, begins with ``#i`` and a
space, and continues with the group ``123456789`` and a space repeated
until the record's length is reached, then cut to exactly that length.
Every record therefore names itself, which is what lets a stored expected file
pin the identity and order of the records a damaged log yields rather
than only their number. A format given as a range picks each length from
a pseudo-random generator held to a fixed seed, so the same parameters
produce the same sequence of lengths on every run. Both properties are
required: the stored files are worthless without them.

One line per record must be printed as it is written, giving the record
number, the payload, and the trailer described for ``tcslog-dump``.

tcslog-dump
-----------
Reads a log back. It takes the directory, prefix, and suffix as
positional arguments, and accepts ``-t``/``--text`` to choose between
hex and text rendering, and ``-v``/``--verbose``.

Records are read into a buffer of a fixed 256 bytes named by a single
constant. That constant is what makes the read-overflow path reachable
from a command line, so it must stay fixed rather than being sized to the
log being read.

Without ``--verbose`` the output is one line per record and nothing else.
``--verbose`` adds, and must add only, the following:

o   A block per segment file traversed, printed before the record it
    carried, giving the file name and every header field. Every segment a
    read traversed must be reported, not merely the one a record ended
    in: a record spanning several segments begins in one that naming only
    the current segment never mentions. Segment headers are to be
    collected for printing only when ``--verbose`` is in effect, since a
    run that never prints them would otherwise buffer them forever.

o   ``    -- {lost} missing segment file(s); resynchronizing --`` when a
    truncated read reports a non-zero count of lost segment files, and
    ``    -- corrupted or truncated segment file; resynchronizing --``
    when it reports zero. The second wording claims no lost file because
    none was: the sequence was intact and the crossing was refused for
    damage within a surviving segment.

o   ``    (payload larger than 256-byte buffer; {n} bytes captured,
    remainder discarded)`` after a record that overflowed the buffer.

o   ``--- End of Session---``, preceded by a blank line, at a session
    boundary.

o   ``read {total} message(s) across {n} file(s)`` at the end, where the
    file count comes from the reader's own tally of segments opened
    rather than from counting the headers printed, which undercounts
    whenever a record spanned segments. A further line
    ``{n} segment file(s) lost`` follows it when any were.

Bytes that reached the buffer before a gap, and bytes captured before an
overflow, are real telemetry and must be printed rather than discarded,
marked so that neither can be mistaken for a whole record. A usage error
exits with status 2 after printing the help text.

tcslog-dumphdr
--------------
Prints one segment file's header on a single line, as
``segment_id=``, ``session_id=``, ``max_size=``, ``remaining=``,
``format=``, and ``sequence=`` pairs separated by commas, with the two
identifiers rendered as timestamps rather than as the dashed hexadecimal
that names the file.

It reads the file named on its command line, or standard input when none
is named, and must never consult the file's name, so that it also serves
a segment file that has been renamed or copied out of its log -- the case
in which the name is exactly what cannot be trusted. Only the header is
read, leaving the stdin form usable on a pipe whose writer is still
running. A read failure exits 1, a usage error exits 2.

sample
------
An example of the library crate, at ``tcslog/examples/sample.rs``, rather
than a crate of its own: it is the shortest complete illustration of the
writing side, so it belongs with the API it illustrates and is compiled
by ``cargo test`` along with everything else.

Writes a small demonstration log, taking the directory, prefix, and
suffix as positional arguments -- the same three ``tcslog-dump`` takes --
so that it and ``tcslog-dump`` form a runnable pair for someone new to
the crate. It caps the segment size low enough to force rollover after a
handful of records, so the chain it leaves has several segment files to
inspect, and it supplies a ``send`` callback that announces each
completed segment file rather than doing nothing, so the handover point
is visible. ``-v``/``--verbose`` adds the sizes it chose, a column ruler,
and a closing count.

``bin/run-sample`` runs it against a temporary directory, lists the
segment files it wrote, and deletes the directory afterwards.

Testing
=======
Testing has two layers and both are required. Unit tests inside the
crates reach the library through its API, where a fault can be injected
directly. The error-recovery suite under ``test/`` reaches it the way a
user does, through ``tcslog-gen`` and ``tcslog-dump``, with the faults
applied to the bytes on disk.

Unit Tests
----------
Make sure to do the following:

o   Test Format::Fixed records that do not span multiple segment files and
    those that do, including spanning of both one segment file and more
    than one segment file.

o   Verify zero length Variable and VariableTsRc records, records that don't
    span segment files, and very long records that span multiple segment
    files

o   Make sure corrupt header skipping is tested in code that opens the next
    segment

o   Check that sessions are correctly detected and that errors preceeding and
    following yield the expected number of data records.

o   Where it makes sense, all tests should be tested with each segment file
    format.

o   Simulate write errors to verify error propogation and that the next call to
    write() creates a new segment file.

o   Simulate file creation failure to verify error propogation and that the
    next call to write() creates a new segment file.

o   Simulate the correct behavior in the presence for faults:

    -   Missing and unreadable segment files

    -   Read failures when:

        *   In data records that don't span segment files

        *   In the beginning, middle, and end of data records that span multiple
            segment files

o   Verify that the callback function send() is called exactly as many times as
    there are segment files with data in the data section.

o   Ensure the callback function record_complete() is called each time
    a data record is written and no more times than that.

o   Verify that the user callback function sent() is called when the LogWrite
    drop() function is invoked.

o   Check that every segment file but a session's last is exactly
    seg_size\ :sub:`max` bytes, for each of the variable formats, and
    that a data header does straddle a boundary when one falls inside it.

o   Check that a segment file cut short part way through a data header
    yields the read-truncated error rather than a record, and that the
    reader then recovers and returns the records after it. A reader that
    spliced the bytes after the boundary onto the partial header would
    return a record the writer never wrote, so assert that every record
    returned is one that was written.

o   Check that clear() leaves no segment file in the directory, starting
    from a log whose segment files include both a closed one and the one
    still open, and that a reader then finds no log there at all.

o   Check that the first write() after clear() starts a new session, with
    a new session identifier and its record count restarted at one, and
    that the log then reads back as exactly the records written after the
    clear, with no loss reported.

Error-Recovery Test Suite
-------------------------
A suite under ``test/`` generates a log, damages the segment files, reads
the log back, and compares the result against a stored file. It checks
both halves of what the reader owes a caller: which records survive, and
what the reader says it lost. A reader that quietly dropped records while
producing a plausible-looking subset would pass the first check and fail
the second.

Layout and Naming
~~~~~~~~~~~~~~~~~
The directory holds a ``Makefile``, a stored-file directory
``expected/``, and one script per test case. The driver
``error-recovery-common`` lives in ``bin/`` alongside the other
hand-run tools, as does ``dump-before``, an aid that dumps a directory
of segment files.

Each case is a short script that sets the format, data-section size,
record count, and the damage it wants as shell variables, then runs the
driver with those as options followed by ``"$@"`` so that a flag given to
the case reaches the driver. A comment at the top states what the case
establishes and why nothing else covers it.

A case's file name must match what it does, because a name is how a
reader of the directory judges what is covered:

o   The script's basename and the basename of its stored file must be
    the same string.

o   The name states the format when it is not ``variable-simple``:
    ``tsrc-`` for ``variable-tsrc`` and ``fixed`` for ``Format::Fixed``.
    A ``variable-simple`` case needs no marker.

o   A trailing ``_<record>-<data>`` records the format's record length
    and the data-section size, so that two cases differing only in
    geometry are distinguishable.

o   A name that disagrees with what the script does is an error, not an
    untidiness: it overstates the coverage the directory has.

The Driver
~~~~~~~~~~
``error-recovery-common`` does the work; the cases only supply
parameters. It must change to ``test/`` before doing anything else,
naming that directory relative to its own rather than simply using its
own, since it sits in ``bin/`` and its stored files do not. Cargo
locates both the workspace manifest and the ``.cargo/config.toml`` that
supplies the build-time timer resolution by walking up from the working
directory, so resolving any of the three against the caller's directory
instead would leave the cases runnable only from the test directory. No
option takes a path, so nothing is left pointing at the caller's
directory.

Segment files are generated into a fresh temporary directory, removed by
a single exit trap so that the removal happens however the run ends. The
options are:

o   ``-e`` names the expected file within ``expected/`` and is
    required. A missing file is an error that names ``-g`` rather than a
    silent pass.

o   ``-f``, ``-s``, ``-n``, and ``-S`` set the record format, the
    data-section size, the record count, and the number of generation
    passes. Each pass beyond the first writes a further session into the
    same directory, so that the reader must end one session before
    reading the next.

o   ``-d``, ``-c``, ``-t``, ``-V``, ``-I``, and ``-O`` each take a list
    of segment file indices to damage, described below.

o   ``-g`` writes the stored file from this run instead of comparing
    against it.

o   ``-k`` keeps the temporary directory and prints its path, and ``-r``
    and ``-x`` hexdump the segment files before and after the damage.
    These are for working on a case by hand, as is ``-h``, which prints
    the option summary.

Indices start at one, and the word ``last`` stands for the
highest-numbered segment file. How many files a run produces depends on
the format and the sizes, so ``last`` can only be resolved after
generating the log. An index naming no segment file must fail the run:
accepted silently, it would leave a case asserting less than it appears
to. So must an index whose file an earlier action already removed.

The damage each option applies must be exactly this, since the stored
files record the reader's response to it:

o   ``-d`` deletes the file.

o   ``-c`` overwrites the eight-byte file-type magic with zeros. The file
    remains, so its neighbours' sequence numbers still step over it and
    the reader sees the same gap a deleted file would leave -- which is
    the point: a corrupt segment and a missing one are the same event to
    a reader, and only a test that leaves the file in place proves it.

o   ``-t`` cuts the file to the header plus half of the data bytes the
    file actually holds. Half of the nominal data section is wrong: a
    session's last segment file is shorter than
    seg_size\ :sub:`max`, and truncating to a larger size would pad it
    with zeros instead of cutting it, quietly substituting a different
    kind of damage for the one the case asked for. The header survives,
    so the sequence is intact and a record running off the short end is a
    truncation the sequence cannot explain -- the case where the lost
    count must be zero.

o   ``-V`` sets the version field to a major this build cannot read.

o   ``-I`` inverts a byte of the stored segment identifier so that it no
    longer matches the identifier in the file name, as a renamed or
    copied file would not.

o   ``-O`` pads the file one byte past the maximum size its own header
    declares, which the format forbids.

Comparison
~~~~~~~~~~
One ``--verbose`` capture is the whole comparison, held against one
stored file. That capture carries the records, the per-segment header
blocks, the truncation and overflow notices, the session markers and the
totals, in the order the reader produced them, so the stored file fixes
not only what was reported but how the reports interleave: which segment
each record came from, which session a loss fell in, which record a
notice follows. Splitting the records and the diagnostics into two
captures compared against two files would assert each list separately
and none of that ordering, which is where a reader that recovered the
right records and attributed them to the wrong place would hide.

Comparing the header blocks is the point of capturing them. Of their
fields only the two identifiers fail to repeat: ``max_size``,
``remaining``, ``format`` and ``sequence`` are fixed by the log and the
damage done to it, and ``remaining`` and ``sequence`` are the two fields
the reader's loss detection rests on. A suite that discards them cannot
see a reader that computes either one wrongly unless the surviving
record set happens to change with it.

Two kinds of value in that capture cannot be stored as they stand:

o   Segment identifiers are wall-clock timestamps, so neither they nor
    the file names built from them ever repeat. Each distinct one is
    replaced by a label numbered in order of first appearance. This
    keeps the values out of the comparison while holding what they say:
    that two segments belong to one session, or that a later segment
    opens a new one. Masking them all alike would discard exactly that.
    A label that appears as some segment's session while never appearing
    as a segment of its own is itself the evidence that the segment
    which opened that session is gone.

o   The ``variable-tsrc`` format stamps every record with the time it
    was written. Those are replaced by a single label rather than
    numbered ones, because whether two records share a timestamp depends
    on the clock's granularity and is not a distinction the reader is
    making. Every other field, the record count included, is compared
    exactly.

Without ``--verbose`` the tool prints records and nothing else. The
stored file is the verbose form and so cannot show that, which would
leave the quiet mode uncovered; a run without the flag is therefore
checked directly for the absence of every verbose-only line.

Because the stored files are written from the tools' own output, they
record what the tools currently do, and a regeneration captures a
regression as readily as a fix. Regenerating is therefore a reviewed
operation: the resulting diff is the thing to read.

The two hexdumps that ``-r`` and ``-x`` produce, and ``dump-before``,
must skip ``SEGMENT_FILE_HEADER_LEN`` bytes before dumping the data
section, so that no part of the header is presented as though it were
telemetry, and must invoke ``tcslog-dumphdr`` for the header itself --
the library crate has no binary to run.

Cases
~~~~~
The suite covers the following. Cases are grouped so that a group can be
run alone while working on the behaviour it covers.

o   Whole segment files lost from a ``Format::Fixed`` log, with a record
    per segment, a record filling a segment exactly, and a record
    spanning segments: lengths of one, eight, and five bytes against
    data sections of one, eight, and seventeen. A further case truncates
    a data section rather than deleting the file.

o   Whole segment files lost from a ``variable-simple`` log, both at
    fixed record lengths and at lengths drawn from a range, including a
    record that ends exactly on a segment boundary and a case that
    deletes the last segment file -- the one whose loss a reader is most
    likely to mistake for a clean end of log.

o   The same for ``variable-tsrc``, whose data header is the wider of the
    two and so offers the most bytes that a resynchronizing reader could
    misread as a record length.

o   Damage that leaves the file in place, in both variable formats: an
    unopenable header, which still leaves a sequence gap, and a short
    data section, which leaves the sequence intact and so must be
    reported with a lost count of zero. One case applies a deletion, an
    unopenable header, and a short data section to different segment
    files of one log, to show that the reader's accounting survives
    recovering repeatedly.

o   Files the reader must refuse outright rather than read with the wrong
    assumptions: an unreadable version, a stored segment identifier that
    disagrees with the file name, and a file larger than the maximum size
    its own header declares. All three are settled before the header's
    format tag is consulted, so one format exercises them and a case per
    format would assert nothing further.

o   A record too large for the caller's buffer, in both variable
    formats. This one does need a case per format: the count reported is
    of payload bytes, so a data header charged against the caller's
    buffer would show up as a differing count, and the two formats'
    headers differ in width.

o   Several sessions in one directory with a segment file missing from
    one of them, so that the reader must end each session before reading
    the next and must attribute the loss to the right one.

The parameters behind those groups are the following. Format is the
``-f`` spec, which carries the record length; data section is ``-s`` and
records is ``-n``; damage is the remaining options, described above. A
row stands for more than one case only where the cases share a format
spec, a data-section size and a record count, differing in the damage
alone -- so a row is one log geometry, and the damages listed against it
are what that one geometry is asked to survive. A case whose geometry
differs from every row's is a row of its own, however small the
difference: two cases that look combinable but are not share no stored
file and prove nothing about each other.

.. list-table::
   :header-rows: 1
   :widths: 21 20 8 7 17 27

   * - Case
     - Format
     - Data section
     - Records
     - Damage
     - Establishes
   * - ``recovery-fixed_1-1``
     - ``fixed:1``
     - 1
     - 5
     - ``-d 1 3 4``
     - One record per segment file.
   * - ``recovery-fixed_8-8``
     - ``fixed:8``
     - 8
     - 4
     - ``-d 2``
     - A record filling the data section exactly.
   * - ``recovery-fixed_5-17``, ``fixed-truncated_5-17``
     - ``fixed:5``
     - 17
     - 20
     - ``-d 1 3 4``; ``-t 3``
     - A record spanning segments, and a short data section in a format
       with no stored record length to check.
   * - ``variable_0-10-17``
     - ``variable-simple:0..10``
     - 17
     - 20
     - ``-d 1 3 4``
     - Lengths drawn from a range that stays inside the data section.
   * - ``variable_0-33-10``
     - ``variable-simple:0..33``
     - 10
     - 20
     - ``-d 1 3 4``
     - Lengths drawn from a range wider than the data section, so
       records span segments and some span more than one.
   * - ``variable-exact_6-20``
     - ``variable-simple:6``
     - 20
     - 8
     - ``-d 3``
     - A record ending exactly on a segment boundary, which the other
       variable cases never reach.
   * - ``variable-last_12-10``
     - ``variable-simple:12``
     - 10
     - 3
     - ``-d last``
     - The final segment lost while a record still runs into it, which
       must not read as a clean end of log.
   * - ``corrupt_12-10``, ``truncated_12-10``, ``version_12-10``,
       ``segid_12-10``, ``oversize_12-10``
     - ``variable-simple:12``
     - 10
     - 5
     - ``-c 2``; ``-t 2``; ``-V 2``; ``-I 2``; ``-O 2``
     - One damaged segment in an otherwise whole log, each kind against
       the same geometry so the stored files differ only by the reader's
       response to the damage.
   * - ``combined_12-10``
     - ``variable-simple:12``
     - 10
     - 10
     - ``-d 2 -c 6 -t 10``
     - Three kinds of damage in one log, on separate segments, so the
       accounting survives recovering repeatedly.
   * - ``variable-tsrc_0-10-21``
     - ``variable-tsrc:0..10``
     - 21
     - 20
     - ``-d 1 3 4``
     - Whole segments lost from the format with the wider data header.
   * - ``tsrc-truncated_10-24``, ``tsrc-corrupt_10-24``
     - ``variable-tsrc:10``
     - 24
     - 6
     - ``-t 2``; ``-c 2``
     - Damage leaving the file in place where the data header offers the
       most bytes a resync could misread as a length.
   * - ``overflow_300-64``, ``tsrc-overflow_300-64``
     - ``variable-simple:300`` and ``variable-tsrc:300``
     - 64
     - 3
     - none
     - A record too large for the caller's buffer. The exception to the
       rule above: the two cases share a geometry and differ by format,
       because the reported count is of payload alone and the formats'
       data headers differ in width.
   * - ``sessions_8-40``
     - ``variable-simple:8``
     - 40
     - 6
     - ``-S 3 -d 3``
     - Three sessions in one directory with the loss in the middle one.

The Makefile
~~~~~~~~~~~~
One target per case, gathered into one target per group, with ``test``
running every group. A ``regenerate`` target rewrites every stored file.
It must find the cases by walking the directory's executable files and
skipping the ``Makefile`` and the stored-file directory by name; the
driver and ``dump-before`` need no exclusion, as they are not in the
directory being walked. Matching case names against a pattern instead
leaves the target silently doing nothing the first time the cases are
renamed.

Restrictions
============
Values written to segment files are packed, that is, there are no padding
values. All numerical values are written in little-endian format, so
all objects whose values are read and written to and from the file must
have to_le_bytes() and from_le_bytes() functions.

It is an error if seg_size\ :sub:`max` is less than or equal to the number of
bytes in the file used for the segment header and the number of bytes
used for one data header.

No dynamic memory allocations may be done after calls to LogRead::new() and
LogWrite::new() until those objects are dropped.

o   Prefix and suffix values must not contain the path delimiters. If they
    do, the return value must be LogError::PathDelimiterNotAllowed

o   LogError values must be returned instead of panicing.

o   All functions must be preceeded by documentation specifying the
    purpose of the function, the usage of parameters, and return values.

o   Avoid operating-specific constructs, i.e. generate code that will work on
    Linux, Windows, VxWorks, FreeRT.

o   Check for spelling

Ideally, no dynamic memory allocation would be done at all, if it can be
avoided.

Code Generation Restrictions
============================
o   Request guidance in case of ambiguous, incomplete, or contradictory input

o   Violations of Rust coding style conventions are to be identified and
    an marked as an error.

o   Do not allow dead code.

o   Do not create a tar file.

Use of AI
=========
This file was used as the AI prompt file. The
file spells out the algorithms used, so AI didn't do this, but the actual
code generation was done with Claude Code. The user documentation at
``docs/tcslog.rst`` was generated from this file originally, but is now
maintained by hand and is no longer regenerated from it.

In addition, Claude Code was used to review the code and documentation it
produced and suggestions incorporated into this file.
