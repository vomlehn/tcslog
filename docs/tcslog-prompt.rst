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

File Format
===========
All segment files start with a header, followed by a data section. The 
data section consists of data records, which may be broken across the
data sections of two or more segment files.

Data records consist of a data header, possibly of zero length, followed
by some number of bytes of telemetry data. Some segment files formats
permit the number of bytes of telemetry data in a data record to be zero,
some do not.

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
    This is the number of bytes remaining in the data record that contains the
    first byte in the data section.
    This value may be longer than the length
    of the data section, in which case the record is continued in the following
    one or more segment files.

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

        The data header for this format is of zero size. A record with a byte
        in the first byte of the data section for a segment file with
        segment ID n will have 0 or more bytes in preceeding segment
        files. Thus, the offset of that byte in the record is:
        
            offset = (s * size of data section) mod n

        If this is zero, the first byte of the data data record is at the
        first byte of the data section of the segment file with segment
        number n.

        The number of bytes remaining in the record are:

            remaining = n - offset
        
    VariableSimple
        Records may have from zero to RecSize.MAX bytes. This will generally
        used when the telemetry data being stored already contains a
        timestamp.

    VariableTsRc
        Records may have from zero to RecSize.MAX bytes.
        Each data record will be accompanied by a timestamp, which is a
        nanosecond-resolution, 64-bit offset from the UNIX epoch. This
        value will be returned when data is read.

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

The sleep time is twice the system-dependent time resolution, named
TIMER_RESOLUTION. This is specified in nanoseconds.
By sleeping for this amount of time, the next time the system time is
read, it must be greater than the previous value. Since the system
time increases monotonically, it must be greater than any previous
time and so is unique.

TIMER_RESOLUTION is defined in the cargo build command by using
the --features option. It is a u64 value.

Using an u64 value as the segment ID assures that a huge number of segment
files can be created. Only positive values are supported, so the theoretical
number of segment files is 2\ :sup:`63` or 1.8e19.
Alternatively, there could be enough segment files for over two centuries.

Operations
==========
Tcslog supports two broad categorie of operations: reading and writing.

Write-Related Operations
------------------------

Initialization for Writing
~~~~~~~~~~~~~~~~~~~~~~~~~~
Call the user function send() for all existing segment files.

Writing Telemetry Data
~~~~~~~~~~~~~~~~~~~~~~
Each time a user calls the write function with telemetry data, a data
record is written which consists of a data header
which may be of zero size, followed by the telemetry data. The length of
the telemetry data portion is controlled by the format.
The data record write starts at the current location in the data section of
a segment file and must write as many bytes as it can without growing the
segment file to more than seg_size\ :sub:`max` bytes.

If the segment file is shorter than seg_size\ :sub:`max` bytes when the entire
data record is written, the write is complete and the function returns to the
user.

If the segment file size reaches seg_size\ :sub:`max` bytes during the write
operation,
the current segment file is closed, the segment file number is
increased, and a new segment file is created.

Writing of the data record continues until all bytes have been written,
creating new segment files as required.

Segment File Creation
~~~~~~~~~~~~~~~~~~~~~
When there current segment file fills, i.e. its length is seg_size\ :sub:`max`,
the file is closed and the LogWrite::send() function is called with the
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
The value in the remaining field is the number of bytes from the data
record that must still be written.

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

Deleting a Log File
~~~~~~~~~~~~~~~~~~~
Logwrite::clear() can be called to delete all segment files. It goes
through all existing segment files and deletes each one.

Read-Related Operations
-----------------------

Find the First Data Record Start
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
There may be missing segment files at the beginning of a log. To skip
any missing or corrupted segment files and find the start of the
first data record, begin by constructing a list of all existing
segment files matching the given prefix and suffix, in order from oldest
to newest. Then, starting with the oldest segment file, read the segment
file headers. There are three cases:

o   The remaining field value is greater than the size of the data section:
    continue to the next segment file.

o   The remaining field value is less than the size of the data section:
    The log starts at an offset of remaining into the data section.

o   The remaining field value and size of the data section are equal:
    If this is the last segment file, there is no data available in the
    log. Otherwise, the log starts at the first byte of the next segment
    file.

If a segment file cannot be opened or the header cannot be read, the
search for the start of the log advances to the next segment file.

It is an error if no matching segment file names are found.

Reading Telemetry Data
~~~~~~~~~~~~~~~~~~~~~~
Each time a new segment file is opened, the segment header is read. It does
the following checks:

o   The type is "tcslogsf".

o   The version string is "0010", corresponding to version 0.1.0.

o   The segment ID matches the segment part of the segment file name

It can then start reading data records.

To read a data record, first try to read a data header. If an end of file
is encountered, close the segment file and open the one with the next
segment ID, do the file header verification, and try to read the
a data header again. Keep doing this until no more segment files are
available.


Once we read a data header, read the number of bytes specified by n.
Copy as many as will fit into the user's buffer. If the buffer is too
small, set a flag indicating this so we can return an overflow status.
Keep

If we can't open the segment file, and we found more
segment files, skip this one and open the next.

Reading a Record
~~~~~~~~~~~~~~~~
If there is no current segment file, remove the segment file name with the
smallest segment ID from the list of matching segment file names
and attempt to open it. If the open fails and the match segment file name
list is now empty, we have reached to end of the log and return an
approprite status. Otherwise, extract the segment ID from the segment
file name as a SegId value.

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

pub fn send(name: &str) -> Result((), Error);

    Process a completed segment file.

    name            Name of the segment file, including the directory.

pub fn clear();

    Remove all existing segment files.

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

LogError
--------
    Enum used to return error values. It includes the following:

    ReadOverflow(u32)

        There was too much telemetry data in the data record to fit in
        the supplied buffer. The value indicates the actual number
        of bytes or characters available.

    IoError(io::Error)

        An error occurred from an I/O operation.

    HasDelimiter

        A prefix or suffix in a new() call has a filesystem delimiter.

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
    log writing operations:

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

User Documentation
==================

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
        
Restrictions
============
Values written to segment files are packed, that is, there are no padding
values. All numerical values are written in little-endian format, so
all objects whose values are read and written to and from the file must
have to_le_bytes() and from_le_bytes() functions.

It is an error if seg_size\ :sub:`max` is less than or equal to the number of
bytes in the file used for the segment header and the number of bytes
used for one data header.

No memory allocations may be done after calls to LogRead::new() and
LogWrite::new() until those objects are dropped.

o   Prefix and suffix values must not contain the path delimiters. If they
    do, the return value must be LogError::PathDelimiterNotAllowed

o   LogError values must be returned instead of panicing.

o   All functions must be preceeded by documentation specifying the
    purpose of the function, the usage of parameters, and return values.

o   Avoid operating-specific constructs, i.e. generate code that will work on
    Linux, Windows, VxWorks, FreeRT.

o   Check for spelling

Code Generation Restrictions
============================
o   Request guidance in case of ambiguous, incomplete, or contradictory input

o   Violations of Rust coding style conventions are to be identified and
    an marked as an error.
