=============
TcsLog Prompt
=============
.. contents:: Table of Contents
   :depth: 4
   :local:

Introduction
============
Create a Rust library named Tcslog for onboard logging of telemetry data.
It can be used in conjunction with live transmission of telemetry data
to ensure data from corrupted live transmission can be recovered. It
divides log storage into segment files to allow downlinking in small
batches and to easily skip data that has already been downlinked.

In the event that some of the data stored onboard cannot be read, the format
of the the segment files allow skipping corrupted data and resynchronizing
with the telemetry data boundaries on a segment file granularity.

When creating a Tcslog log, the maximum number of bytes that will be stored
onboard is specified, allowing confidence that the log data will not fill
available storage. Each segment file is limited to size\ :sub:`max` bytes and
no more than n\ :sub:`seg` segment files will be used by Tcslog at any
time. Thus, the maxmimum amount of storage used at any time is the product
of these two values.
These two parameters are specified when the tcslog interface is created.

.. note::

    The maximum number of bytes is the sum of telemetry data and headers
    for segment files and data records. A small amount of extra space is
    generally required for file metadata, which may depend on file name
    size, segment file size, etc.

Tcslog will inform user code that a segment file is complete.
It is up to that code to manage the storage used after that point, so
it compress or delete the segment file after that point.

There are several log formats, trading storage efficiency for automatic
recording of meta data.

In addition to size\ :sub:`max` and n\ :sub:`seg`, two more parameters are
specified when creating the tcslog interface: prefix and suffix. These are
strings used in determining the names of the segment files. Segment file
names start with a prefix, followed by a segment ID of type SegId, followed
by the suffix. The segment ID is a zero-filled hexadecimal string,
using lower case
values, with a dash ('-') between each group of four hexadecimal characters.
Thus, if SegNo is of type u64, the segment ID is given by the value
0x1234abcd5678efabu64, the segment
ID will be the string "1234-abcd-5678-efab". 

In version 1.0.0, SegId values are u64 objects.

File Format
===========
All segment files start with a header, followed by a data section.

Segment Header Format
---------------------
The
header is fixed length. The total length of a segment file must
be less than or equal to size\ :sub:`max`. 

The segment file header contains the following:

type
    This is an ASCII string that identifies this as a TcsLog file. It has the
    value "tcslogsg". This must be the first data in the file.

version
    This is a four-character ASCII string. All characters must be in the range
    from '0' to '9'. The first two characters are the major version number, the
    next character is the minor version number, and the last character is the
    patch number. The file format is compatible if the major and minor
    verson numbers match. This must follow the type field.

segment ID
    Segment ID for this segment file. This will be checked to verify that
    it matches the segment ID that comprises part of the segment file
    name. This is an i64 value.

session ID
    A session starts when the LogWrite::new() function is called and ends
    when the object implementing the LogWrite interface is dropped. This
    may be explict, implicit, or when the process ends. The segment ID of
    the first segment file created in a session is stored as the session ID
    for all segment file in that session.

remaining
    This is the number of bytes remaining in the record that starts at the
    beginning of the data section. This value may be longer than the length
    of the data section, in which case the record is continued in the following
    one or more segment files.

data format
    Several formats are supported for storing data, which vary by storage
    efficiency, allowable telemetry data length, and whether timestamps are
    automatically generated. Records can generally be split across segment
    file boundaries, so that completed segment files are generally much the
    same length.

    Supported data formats are:

    FIXED(n)
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
        
    VARIABLE_SIMPLE
        Records may have from zero to RecSize.MAX bytes. This will generally
        used when the telemetry data being stored already contains a
        timestamp.

    VARIABLE_TSRN
        Records may have from zero to RecSize.MAX bytes.
        Each data record will be accompanied by a timestamp, which is a
        nanosecond-resolution, 64-bit offset from the UNIX epoch. This
        value will be returned when data is read.

The segment file header size must be the same for all data formats even
if some bytes are not used for some data formats.

Data Header Format
------------------
The data section consists of alternating data headers and telemetry data, which
are written sequentially to the data section of segment files. The size of the
data section, in bytes, is:

    size\ :sub:`max` - size of segment header

There are multiple types of data header, depending on the format specified in
the segment file header:

    FIXED(n)
        The data header is zero length, i,e. each telemetry data record is
        logical continguous with the preceeding telemetry data record.

    VARIABLE_SIMPLE
        Field in the data header for this format are:

        n
            Number of telemetry data bytes in the data record.

    VARIABLE_TSRN
        The data header type has the same initial fields as the VARABLE_SIMPLE
        format, plus the following fields:

        timestamp
            Offset from the UNIX epoch with nanosecond resolution, represented
            as a Timestamp value.

        record number
            The record number is one for the first record in the log and
            increments by one for each record writen. It is of type RecNum,
            which is expected to be a RecNum object.

Segment IDs
-----------
The segment ID is the time since the UNIX epoch, with nanosecond resolution.
When a segment file is created, the current time is read and the prefix and
suffix added to produce the name of a segment file. Tcslog attempts to create
a new segment file with name. If the file already exists, it sleeps, then
gets a new current time and tries to create the file again.
Each time it fails, it increases the sleep interval and tries again.
This ensures that it
will quickly find an unused segment ID.

The initial sleep time is one microsecond. The next sleep interval is the
previous sleep interval times four.

Using an i64 value as the segment ID assures that a huge number of segment
files can be created. Only positive values are supported, so the theoretical
number of segment files is 2\ :sup:`63` or 9,223,372,036,854,775,808.
Alternatively, there could be enough segment files for over two centuries.

Operations
==========

Find the First Data Record Start
--------------------------------
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

Finding the End of the Log
--------------------------
To find the end of the log, i.e. the location at which additional data
should be added, start by constructing a list of all existing segment
files matching the given prefix and suffix, in order from oldest to newest.
Starting with the newest segment file, read the segment file header.

Initialization for Writing
--------------------------
Create a list of existing segment files, sorted by name. 

If the segment file format is VARIABLE_TSRN, the record number to use for
writing the next record must be determined. To do this,
open in each segment file the order of the segment ID.
If a segment
file could not be opened, call the user defined function bad_file() with the
name of the segment file. The default definition of bad_file() attempts to
delete a file with that name. If a segment file could be opened, read
all record headers to find the largest record number in that segment file.
If no record numbers could be read, use the value one for the current
record number. Otherwise, use the largest record number plus one.

Consistency checks:
o   Record numbers must be monotomically increasing.

o   The type must be as specified

o   The version must be valid

o   If the format is FORMAT(n), remaining must be <= n.

o   The format must be valid.

o   The segment file size must be < size :sub:`max`

If there are no existing segment files, create a new one. Otherwise,
open enqueue all but the last segment file onto the send FIFO,
open the last segment file, and seek to the end of that segment file.

When creating a new segment file, use a segment ID of 0 if there were
no other segment files. Otherwise, use a segment ID one greater than
largest the segment ID of the opened number.
If the segment ID of the largest opened segment file's segment ID
is already SegId.MAX, the user function log_full() will be called. Log_full()
can call the clear() function to reset the segment ID to zero.

Creating a new segment file involves calling the check_overflow() user function.
It returns only when there are fewer than n\ :sub:`seg` segment files. It
is up to the check_overflow() function how to ensure this. It might, for
example, do one of the following:

o   Delete one of the files named in the send FIFO.

o   Wait until the first file named in the send FIFO has been sent.

o   Compress and copy some number of files in the send FIFO to some form
    of secondary storage.

When check_overflow() returns, a segment file is created using the current
segment ID. The segment ID is then incremented.

Writing Telemetry Data
----------------------
Each time a user calls the write function with telemetry data, a data
record is written which consists of a data header
which may be of zero size, followed by the telemetry data. The length of
the telemetry data portion is controlled by the format.
The data record write starts at the current location in the data section of
a segment file and must write as many bytes as it can without growing the
segment file to more than size :sub:`max` bytes.

If the segment file is shorter than size :sub:`max` bytes when the entire
data record is written, the write is complete and the function returns to the
user.

If data record bytes remain after the segment file size reaches size :sub:`max`
byte, the current segment file is closed, the segment file number is
increased, and a new segment file is created.

Writing of the data record continues until all bytes have been written,
creating new segment files as required.

Segment File Creation
---------------------
When there current segment file fills, i.e. its length is size :sub:`max`,
the file is closed and the LogWrite::send() function is called with the
name of the file.

When send() returns there must not be a file
with the name it was passed. This can mean:

o   The file was downlinked and deleted.

o   The file was renamed for later downlinking

o   Etc.

Before a new segment file is created, the segment ID is checked to
verify it is not SegId\ :sub:`max`. If it is, a TcslogError value is returned
indicating that the log is full.

When a new segment file is to be created, a check is made to see whether
there are currently at least n\ :sub:`max` waiting to be processed. If so,
the function LogWrite::force() is called.
This function must reduce the number of files
waiting to be transmitted by at least one. The user may implement various
options, incuding:

o   Dequeuing the oldest item in the send FIFO and deleting the file.

o   Waiting until the next file is downlinked.

o   Etc.

After these, and possibly other, checks, are made a new segment file is created.
This becomes the current segment file.  After this, a segment file header is
written.
The value in the remaining field is the number of bytes from the data
record that must still be written.

Logwrite::clear() can be called to delete all segment files.

Error Handling
^^^^^^^^^^^^^^
When a segment file is created, a TcslogError value specific to the create
operation is returned, containing the Error 
code returned by the that operation. Unless the segment ID is already
SegId\ :sub:`max`, the segment ID is incremented.

If an operation related to the contents of a segment file, such as getting
the length (though this should normally be maintained internally by Tcslog),
write, seek, etc. a TcslogError value identifying that operation and
containing the error returned by that function, should be returned. The file
is closed and the
file name is placed on the send FIFO. The segment ID is incremented
unless it is already SegId\ :sub:`max`.

Reading Telemetry Data
----------------------
Each time a new segment file is opened, the segment header is read. It does
the following checks:

o   The type is "tcslogsg".

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

Initialization for Reading
--------------------------
Given prefix and suffix strings, create a list of matching segment file
names in a specific directory. This list should be sorted from the smallest
segment ID to the largest segment IDs. Some segment IDs between
the smallest and largest may not have corresponding segment filesl

It is an error if no matching segmennt file names are found.

Reading a Record
----------------
If there is no current segment file, remove the segment file name with the
smallest segment ID from the list of matching segment file names
and attempt to open it. If the open fails and the match segment file name
list is now empty, we have reached to end of the log and return an
approprite status. Otherwise, extract the segment ID from the segment
file name as a SegId value.

Read the segment file header. If

Data Structures
===============

Restrictions
============
Values written to segment files are packed, that is, there are no padding
values. All numerical values are written in little-endian format, so
all objects whose values are read and written to and from the file must
have to_le_bytes() and from_le_bytes() functions.

It is an error if size\ :sub:`max` is less than or equal to the number of
bytes in the file used for the segment header and the number of bytes
used for one data header.

No memory allocations may be done after calls to LogRead::new() and
LogWrite::new() until those objects are dropped.

o   Prefix and suffix values must not contain the path delimiters. If they
    do, the return value must be LogError::PathDelimiterNotAllowed
