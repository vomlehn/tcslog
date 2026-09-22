=============
TcsLog Prompt
=============
.. contents:: Table of Contents
   :depth: 4
   :local:

Introduction
============
Create a Rust library named Tcslog for onboard logging of telemetry data
for systems such as as spacecraft and autonomous underwater vehicles that
must store telemetry onboard until opportunies arise for transmission.

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
The segment file header length is the same for all formats.
The total length of a segment file must
be less than or equal to seg_size\ :sub:`max`. 

The segment file header contains the following:

type
    This is an ASCII string that identifies this as a TcsLog file. It has the
    value "tcslogsf". This must be the first data in the file.

version
    This is a four-character ASCII string. All characters must be in the range
    from '0' to '9'. The first two characters are the major version number, the
    next character is the minor version number, and the last character is the
    patch number. The file format is compatible if the major and minor
    verson numbers match. This must follow the type field.

    Following a common software convention, a given version of Tcslog can
    be used with any segment file whose major and minor version numbers
    match. 
    A given version of Tcslog can also be used with any segment file whose
    major version number matches and whose minor version is less that the
    Tcslog minor version.
    Major versions of Tcslog and a given segment file must match to be used
    together.

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

    The reader uses this field to locate the start of the first fresh
    record in a segment it has just opened during resync: the first
    ``remaining`` bytes of the data section belong to a record whose
    header lives in an earlier (possibly missing) segment file and can
    therefore not be reassembled, so they are skipped; the next byte
    begins a fresh record.

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
    both sides, and it reports how many segments were lost. The two
    checks together upgrade the reader's guarantee from "no
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
the segment file header:

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

            The record count is one for the first record in the log and
            increments by one for each record writen. It is of type RecordCount,
            which is expected to be a RecordCount object.

Segment IDs
-----------
The segment ID is the time since the UNIX epoch, with nanosecond resolution.
When a segment file is created, the current time is read and the prefix and
suffix added to produce the name of a segment file. Tcslog attempts to create
a new segment file with name. If the file already exists, it sleeps, then
gets a new current time and tries to create the file again.
This ensures that it
will quickly find an unused segment ID.

The sleep time is twice the system-dependent time resolution, used in the
Rust thread::sleep() function. This value is named
TIMER_RESOLUTION and is specified in nanoseconds.
By sleeping for this amount of time, the next time the system time is
read, it must be greater than the previous value. Since the system
time increases monotonically, it must be greater than any previous
time and so is unique.

TIMER_RESOLUTION must not be defined in the code proper but
is defined on the command line via an included makefile named config.mk.
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

A logical data record's data header must be written entirely within
one segment file. Before starting a record, if the space left in the
current segment file is smaller than the data header for the format,
close the current segment file, call the send() function, and create a
new segment file; the closed file is simply shorter than
seg_size\ :sub:`max`
and no padding is written. A data header split across a segment
boundary cannot be decoded once the segment holding its leading bytes
is lost, because the surviving tail of the payload length is
indistinguishable from the tail of some longer record. Keeping the
header whole means the remaining field always points at a complete
data header, so a lost segment file costs only the records whose own
bytes it held.

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
Logwrite::clear() can be called to delete all segment files for a log. It goes
through all existing segment files in the given directory that match the
prefix, suffix, and all possible segment IDs, deleting each one.

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
the given prefix and suffix. This list is then sorted alphabtically. Since
the segment file ID monotonically increases with time, the list is
consequently sorted from oldest to newest. Processing starts with the oldest
segment file and proceeds to the newest one.

If the segment file list is empty, an error code is returned indicating that
the log could not be found.

After creating the segment file list, the start of the next data record is
found. If errors occur, they are propagated to the caller. If successful,
there will be a current segment file.

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


Validating a New Segment File
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
Each time a new segment file is opened, the segment header is read. It does
the following checks:

o   The type is "tcslogsf".

o   The version string is "0010", corresponding to version 0.1.0.

o   The segment ID matches the segment part of the segment file name

o   The segment file size is less than or equal to the value of max size
    read from the segment file header.

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
since Variable* record sizes come from the header itself. Because the
writer never splits a data header across a segment boundary, no
crossing can happen part way through one: either no byte of the record
has been consumed yet, in which case the previous segment ended on a
record boundary and the new segment's remaining field must be zero, or
the header has been decoded in full and the owed-byte count is known.
Both cases are therefore checked at the crossing itself.

A reader must still not assume a header is whole in a file it did not
write. A crossing with no bytes consumed is validated against
remaining = 0 whether or not a header is being decoded, and the
sequence check applies to every crossing, so a header that does span a
boundary in a foreign or damaged file is caught rather than spliced
together into a bogus record.

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

Print a message at each decision point.

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
files.
Some are interfaces, which requires that the user create structures to
implement log operations. Others provide results of various sorts.

LogWrite
--------
This interface is used for writing to logs. Amoung its members are:

pub fn new(dir: &str, prefix: &str, suffix: &str, seg_size_max: u32, format: Format, write_callbacks: WriteCallbacks) -> Result(LogWrite, LogError);

    Create or extend a log file.

    dir             Name of the directory in which the log file is to be
                    created.

    prefix          String that that is the first part of the the segment file
                    names that make up the log file. Must not contain a
                    filesystem delimiter.

    suffix          String used as the end of the segment file name. Must not
                    contain a filesystem delimiter.

    seg_size_max    Maximum number of bytes in a segment file

    format          Format for segment files

    write_callbacks A structure holding callbacks used at various points
                    in operations

    The value of seg_size_max must be greater than the number of bytes
    in the segment file header.

pub const SEGMENT_FILE_HEADER_LEN: u32

    This is the length of the segment file header, in bytes.

pub fn write_str(&self, msg: &str) -> Result(u32, LogError);

    Write a string to the log file. This invokes write().

    self            Reference to LogWrite

    msg             Reference to string to write

    Write_str() calls write().

pub fn write(&self, msg: &[byte]) -> Result(u32, LogError);

    Write a byte array to the log file.

    self            Reference to LogWrite

    msg             Reference to array of bytes to write

    The user callback function record_complete() is called after all bytes
    have been written. This may flush data is data integrity is the priority,
    otherwise this may do nothing if performance is the priority.

    An error is returned if the format is Fixed(n) and the length of msg
    is zero. It is a distinct error if the format is Fixed(n) and the
    number of bytes in msg is not n.

pub fn flush(&self) -> Result((), LogError);

    self            Reference to LogWrite

    Flush all pending data to mass storage.

pub fn send(name: &str) -> Result((), Error);

    Process a completed segment file.

    name            Name of the segment file, including the directory.

pub fn clear();

    Remove all existing segment files.

pub fn drop(&self);

    If the current segment file has data in the data section, call the
    user callback function send(). Then proceed with the rest of
    processing drop is expected to do.

LogRead
-------
The LogRead interface is used for reading from logs. Its members include:

pub fn new(dir: &str, prefix: &str, suffix: &str) -> Result(LogRead, LogError);

    Open an existing log file.

    dir             Name of the directory in which the log file is to be
                    created.

    prefix          String that that is the first part of the the segment file
                    names that make up the log file. Must not contain a
                    filesystem delimiter.

    suffix          String used as the end of the segment file name. Must not
                    contain a filesystem delimiter.

pub fn read_str(&self, msg: &str) -> Result(ReadResult, LogError);

    Read up to the msg.len() characters from the log. Returns Ok(u32) to
    indicate the number of bytes read.

    self            Reference to LogRead

    msg             String to write

pub fn read(&self, msg: &str) -> Result(LogResult, LogError);

    Read up to msg.len() bytes from the log. Return Ok(u32) to indicate the
    number of bytes read.

    self            Reference to LogRead

    msg             Bytes to write

An Iterator must be implemented for LogRead.

LogError
--------
    Enum used to return error values. It includes the following:

    IoError(io::Error)

        An error occurred from an I/O operation.

    InvalidPathname

        The prefix and suffix cannot be combined with a segment file ID
        and a directory name to form a valid path name.

    InvalidFileName
        The combination of prefix, suffix, and segment file ID does not
        form a valid file name.

    ReadOverflow(u32)

        There was too much telemetry data in the data record to fit in
        the supplied buffer. The value indicates the actual number
        of bytes or characters placed in the buffer.

    SegSizeTooSmall

        Indicates that the specified size of a segment file is less than
        or equal to LogWrite::SEGMENT_FILE_HEADER_LEN. This must not
        take any argument.

    SessionEnd

        All records from a previous session have been read and a segment
        file header has been read with a different session ID. The next
        read operation will return the first record of the next session,
        if there is one.

ReadResult
----------
    Structure returning the result of a read operation. It has the following
    elements:

    n

        Number of bytes stored in the buffer. This may be less than or
        equal to the buffer size.

    meta

        Object of type Meta containing format-specific data.

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

RecSize
-------
    This is the type used to contain the size of telemetry portion of
    a data record. For version 1.0.0, this is defined to be u32.

WriteCallback
-------------
    This structure contains various callback functions used during
    log writing operations. These members should be defined in a way that
    avoids use of heal allocaton and dyn dispatch.:

    fn record_complete(file: File) -> Result((), Error);

        Called after each data record is written. It is up to the implementation
        what this does. It may flush the given file, do nothing, or do
        something else.

    fn send(path: &str) -> Result((), Error);

        Transfer ownership of a segment file from Tcslog to user code.

        path            Name of the segment file, including the directory
                        name.

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

SegId
-----
This is the data structure that holds the segment file ID. Segment file IDs
are based on u64 values.

Using an u64 value as the segment ID assures that a huge number of segment
files can be created. Only positive values are supported, so the theoretical
number of segment files is 2\ :sup:`63` or 1.8e19.
Alternatively, there could be enough segment files for over two centuries.

Converting this value into a string yields something of fixed length, with
a value known as SegId::STR_LEN.

Testing
=======
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

User Documentation
==================
User documentation is written as an .RST file. It must not have any
constructs that cause errors when processed with rst2html. The title of
the document should be "Tcslog User Documentation".

Introduction
------------
The introduction to user document should specify the key advantages of
using Tcslog both internal of error-free operation and for error-recovery
operation.

Functions
---------
Documentation for user-accessible functions should have a description of
what the function does, a description of each parameter, and the
return value.

Building and Installation
-------------------------
Include instructions on how to build code using the tcslog crate library and
how to build, install, and run tcslog-dump. Also, show how to build and
run tcslog-sample.

Record formats
--------------
Per-format documentation on record formats must be presented as a table with
the following columns:

o   Name of the record format

o   Minimum and maximum record size in bytes

o   Elements, their types and definitions, and the number of bytes in the
    data header

o   Details on when to use this record format.

Excluded
--------
The user documentation omits mention of internal tcslog testing and the on-disk
format.

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
code generation was done with Claude Code. Claude Code is also completely
responsible for generating the user interface documentation.

In addition, Claude Code was used to review the code and documentation it
produced and suggestions incorporated into this file.
