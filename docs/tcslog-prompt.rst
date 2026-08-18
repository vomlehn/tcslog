**offset = (k · S) mod L**

Reasoning: byte 0 of `seg(k)` sits at absolute byte address `k·S`, since segments are contiguous and start at 0. Messages are also contiguous from 0, so `msg(i)` covers absolute bytes `[i·L, (i+1)·L − 1]`.

That gives you both quantities:
- `l = ⌊(k·S) / L⌋` — which message it lands in
- `offset = k·S − l·L = (k·S) mod L` — where inside that message

Quick sanity check with L = 100, S = 30, k = 7: absolute address is 210, so `l = 2` (msg(2) spans 200–299) and the offset is 10. And if S is a multiple of L, the offset is always 0, which is what you'd expect.
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

There are several log formats, trading storage efficiency for automatic
recording of meta data.

In addition to size\ :sub:`max` and n\ :sub:`seg`, two more parameters are
specified when creating the tcslog interface: prefix and suffix. These are
strings used in determining the names of the segment files. Segment file
names start with a prefix, followed by a segment ID of type SegNum, followed
by the suffix. The segment ID is a zero-filled hexadecimal string,
using lower case
values, with a dash ('-') between each group of four hexadecimal characters.
Thus, if SegNo is of type u64, the segment number is given by the value
0x1234abcd5678efabu64, the segment
ID will be the string "1234-abcd-5678-efab". 

In version 1.0.0, SegNum values are u64 objects.

File Format
-----------
All segment files start with a header, followed by a data section. The
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

timestamp
    Time at which the segment file was created. The value written is that
    returned by the to_le_bytes() function in Timestamp and it is read as
    a byte array and converted to a Timestamp value with its from_le_bytes()
    function. This is written as an i64, which is a nanosecond offset from the
    UNIX epoch.

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
        segment number n will have 0 or more bytes in preceeding segment
        files. Thus, the offset of that byte in the record is:
        
            offset = (s * size of data section) mod n

`       If this is zero, the first byte of the data data record is at the
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

Because the FIXED(n) format requires extra space in the segment file
header to hold the RecSize telemetry
data length, the segment file header size is fixed so that the data section
size is fixed.

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

        first
            Boolean value indicating that the telemetry data that follows is
            the beginning of the telemetry data record being written. If false,
            this is a continuation of a telemetry data record in another
            segment file from where this first data was written.

        n
            Number of bytes from the telemetry data record that follow the
            data header. If the field first is true, this will be the total
            number of bytes in the telemetry data record. Otherwise, it will
            be the number of bytes in the data telemetry record following the
            bytes written to previous segment files.

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

Operations
---------
Initialization for Writing
~~~~~~~~~~~~~~~~~~~~~~~~~~
Create a list of existing segment files, sorted by name. 

If the segment file format is VARIABLE_TSRN, the record number to use for
writing the next record must be determined. To do this,
open in each segment file the order of the segment number.
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

When creating a new segment file, use a segment number of 0 if there were
no other segment files. Otherwise, use a segment number one greater than
largest the segment number of the opened number.
If the segment number of the largest opened segment file's segment number
is already SegNum.MAX, the user function log_full() will be called. Log_full()
can call the clear() function to reset the segment number to zero.

Creating a new segment file involves calling the check_overflow() user function.
It returns only when there are fewer than n\ :sub:`seg` segment files. It
is up to the check_overflow() function how to ensure this. It might, for
example, do one of the following:

o   Delete one of the files named in the send FIFO.

o   Wait until the first file named in the send FIFO has been sent.

o   Compress and copy some number of files in the send FIFO to some form
    of secondary storage.

When check_overflow() returns, a segment file is created using the current
segment number. The segment number is then incremented.

Writing Telemetry Data
~~~~~~~~~~~~~~~~~~~~~~
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
~~~~~~~~~~~~~~~~~~~~~
When there current segment file fills, i.e. its length is size :sub:`max`,
the file is closed and the LogWrite::send() function is called with the
name of the file.

When send() returns there must not be a file
with the name it was passed. This can mean:

o   The file was downlinked and deleted.

o   The file was renamed for later downlinking

o   Etc.

Before a new segment file is created, the segment number is checked to
verify it is not SegNum\ :sub:`max`. If it is, a TcslogError value is returned
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
~~~~~~~~~~~~~~
When a segment file is created, a TcslogError value specific to the create
operation is returned, containing the Error 
code returned by the that operation. Unless the segment number is already
SegNum\ :sub:`max`, the segment number is incremented.

If an operation related to the contents of a segment file, such as getting
the length (though this should normally be maintained internally by Tcslog),
write, seek, etc. a TcslogError value identifying that operation and
containing the error returned by that function, should be returned. The file
is closed and the
file name is placed on the send FIFO. The segment number is incremented
unless it is already SegNum\ :sub:`max`.

Reading Telemetry Data
~~~~~~~~~~~~~~~~~~~~~~
Each time a new segment file is opened, the segment header is read. It does
the following checks:

o   The type is "tcslogsg".

o   The version string is "0010", corresponding to version 0.1.0.

It can then start reading data records.

To read a data record, first try to read a data header. If an end of file
is encountered, close the segment file and open the one with the next
segment number, do the file header verification, and try to read the
a data header again. Keep doing this until no more segment files are
available.


Once we read a data header, read the number of bytes specified by n.
Copy as many as will fit into the user's buffer. If the buffer is too
small, set a flag indicating this so we can return an overflow status.
Keep

If we can't open the segment file, and we found more
segment files, skip this one and open the next.

Initialization for Reading
~~~~~~~~~~~~~~~~~~~~~~~~~~
Given prefix and suffix strings, create a list of matching segment file
names in a specific directory. This list should be sorted from the smallest
segment number to the largest segment numbers. Some segment numbers between
the smallest and largest may not have corresponding segment filesl

It is an error if no matching segmennt file names are found.

Reading a Record
~~~~~~~~~~~~~~~~
If there is no current segment file, remove the segment file name with the
smallest segment number from the list of matching segment file names
and attempt to open it. If the open fails and the match segment file name
list is now empty, we have reached to end of the log and return an
approprite status. Otherwise, extract the segment number from the segment
file name as a SegNum value.

Read the segment file header. If

Data Structures
---------------

Restrictions
------------
Values written to segment files are packed, that is, there are no padding
values. All numerical values are written in little-endian format, so
all objects whose values are read and written to and from the file must
have to_le_bytes() and from_le_bytes() functions.

It is an error if size\ :sub:`max` is less than or equal to the number of
bytes in the file used for the segment header and the number of bytes
used for one data header.

No memory allocations may be done after calls to LogRead::new() and
LogWrite::new() until those objects are dropped.

***************************************************************************
Each log is
stricty limited according a user specified size. The log is
comprised of multiple physical files, each of which contains a segment of
the logical file. This approach has several advantages:

-  If filesystem corruption causes a segment file to become unavailable, the
   remaining segment files can still be read.

-  The log can be downlinked segment at a time.

General
=======
Data in segment files is always packed, i.e. has no padding

All integer values in logs is in little endian format,
necessitating to_le_bytes() and from_le_bytes() functions for multi-byte data
values.

Each segment file begins with a packed segment file header, i.e. the value from
SegFileHeader::to_le_bytes(), followed by zero or more data records. 

Data records consist of a packed data header, i.e. the value from
DataHeader::to_le_bytes(), followed by zero or more bytes of data. Following
the data header is the record data, which has up to
max_seg - LogHeader::PACKED_LEN bytes. The data from a call to write_str() or
write_bytes() may span multiple segment files.

Data Structures
===============
Filename
````````
    Name of a segment file. This is the concatonation of the prefix, the
    segment file number as a zero-filled hex value, and the suffix.

Timestamp
`````````
    Time in nanoseconds This is an i64 value expressed as an offset from the
    UNIX EPOCH.

RecNum
``````
    Record number. This is a u64 value.

RecSize
-------
    Record size. This is a u32 value.

SegNum
``````
    Segment number. This is a wrapping u64 value.

LogRead
```````
    Interface used for read from Tcslog logs.

LogError
````````````
    Enum for error values returned.

LogWrite
`````````````
   Interface for writing data to logs whose size is strictly limited
   according the parameters passwed. Each log is comprised of
   multiple segments

Constants
~~~~~~~~~
    MIN_DATA_SECTION
        Minumum number of bytes in the data section of a segment file after
        the LogFileHeader and DataHeader have been written. In this version
        of tcslog, this has the value one.

Elements
~~~~~~~~
    name
        The name of the current segment file, including the directory name.

    seg_num
        Segment number for the segment file to which writing is currently
        being done. This increments each time a segment number is created.

    seg_files
        Array with file\ :sub:`max` elements of type Option<SegFile>.

    completed_q
        FIFO where each element is a reference to an element in seg_files.
        This is used by the thread that calls seg_complete().

    avail_q
        FIFO with SegFile elements. It holds references to elements in
        seg_files which are available for writing.

Functions
~~~~~~~~~
   fn new(dir: &str, prefix: &str, suffix: &str, max_file: u64,
      max_seg: u64) -> Result<LogFileWrite, LogError>;

      dir
        Directory in which the log segments files, will be
        created.

      prefix
        First part of segment file names.

      suffix
        End of the segment file names

      max_file
        Maximum size of the segment file. This is referred to as file\ :sub:`max`
        in the rest of this document.

      max_seg
        Maximum size of each segment file.

   fn clear(&self) -> Result<(), LogError>;
        Remove all segments of the current log.

   fn write_str(msg: &str) -> LogError;
      Writes the msg to the log. It does this by passing msg to
      write_bytes().

   fn write_bytes(msg: &u8[]) -> LogError;
      Writes a buffer of u8 values to the log

   fn seg_complete(name: &str);
      Function called when a segment is complete. This is generally used to
      transmit the segment file. It must either move or delete the file before
      returning

   fn log_full(name: &str);
      Called when there are max_seg segment file segments existing. The name is
      that of the next segment file to be used. Possible actions:
      
      o The function could wait for the segment file to be sent.

      o The function could remove the segment file.

      The function must remove the named file before returning.

LogFileRead
```````````
Interface for reading from logs.

Elements
~~~~~~~~
    name
        The name of the current segment file, including the directory name.

    seg_num
        Segment number for the segment file from which reading is currently
        being done. This increments each time a segment file is opened.

Functions
~~~~~~~~~
   fn new(dir: &str, prefix: &str, suffix: &str, max_file: u64,
      max_seg: u64) -> Result<LogFileRead, LogError>;

      dir
        Directory in which the log, i.e. all segment files, will be
        created.

      prefix
        First part of segment file names.

      suffix
        End of the segment file names

      max_file
        Maximum size of the simultaneously existing segment files.
        This is referred to as file\ :sub:`max`
        in the rest of this document.

      max_seg
        Maximum size of each segment file.

    fn to_le_bytes(&self) -> u8[Self::PACKED_LEN];
        Returns a buffer with the values in SegFileHeader with no padding and
        integer values in little endian order,

    fn from_le_bytes(packed: &u8[Self::PACKED_LEN] -> SegFileHeader;
        Returns a SegFileHeader value which is the reverse of the operation
        performed by to_le_bytes().

SegFileHeader
`````````````
This holds information for the segment file header. This appears as the first
thing in each segment file.

Constants
~~~~~~~~~
    PACKED_LEN
        The sum of bytes required in each element of SegFileHeader.  This is
        the number of bytes returned by to_le_bytes() and passed to
        from_le_bytes()

Data
~~~~
    timestamp
        Creation time for this segment

Functions
~~~~~~~~~
    fn new(first: bool) -> Result<SegFileHeader, LogError>;

    fn to_le_bytes(&self) -> u8[Self::PACKED_LEN];

    fn from_le_bytes(packed: &u8[Self::PACKED_LEN] -> SegFileHeader;

DataHeader
``````````
Holds information on a data record and is written before the actual data

Constants
~~~~~~~~~
    PACKED_LEN
        The sum of bytes required in each element of DataHeader.  This is
        the number of bytes returned by to_le_bytes() and passed to
        from_le_bytes()

Data
~~~~
    first
        Boolean that is true if this is the first part of a data record abd
        false otherwise

    num
        Record number. This is a RecNum value.

    len
        The remaining number of bytes of data. This may be greater than the
        space in the segment file, in which case the data record will be
        continued in the next segment file.

    timestamp
        Time at which the record was written.

Functions
~~~~~~~~~
    fn new(first: bool) -> DataHeader;

    fn to_le_bytes(&self) -> u8[Self::PACKED_LEN];
        Returns a buffer with the values in the DataHeader with no padding and
        integer values in little endian order.

    fn from_le_bytes(packed: &u8[Self::PACKED_LEN] -> SegFileHeader;
        Returns a DataHeader value which is the reverse of the operation
        performed by to_le_bytes().

SegFile
```````
Contains information on a segment file.

Data
~~~~
    name
        Name of the segment file, including the directory.

Functions
~~~~~~~~~

Timestamp
`````````
This is like the standard Rust Duration type but with to_le_bytes() and
from_le_bytes() functions.

Data
~~~~

Constants
~~~~~~~~~
    PACKED_LEN
        The sum of bytes required in each element of Timestamp.  This is
        the number of bytes returned by to_le_bytes() and passed to
        from_le_bytes()

Data
~~~~
    Timestamp
        The time when the Timestamp object was created. This is of type
        Duration.

Functions
~~~~~~~~~
    fn new() -> Duration;

    fn to_le_bytes(&self) -> u8[Self::PACKED_LEN];

    fn from_le_bytes(packed: &u8[Self::PACKED_LEN] -> SegFileHeader;

Logic
=====
LogFileWrite Initialization
````````````````````````````

Threads and FIFOs
~~~~~~~~~~~~~~~~~
There are two threads used. The main thread does the bulk of the processing,
whereas the secondary thread is used to process segment files once they
have been completed.

The FIFO LogFileWrite::completed contains references to Mutex<Lock<SegFile>>
objects. It is read by the secondary thread, whose purpose is to call
LogFile::completed() and, when done, enqueue the reference from the FIFO
to LogFileWrite::available.

LogFile::available is read by the main thread in order to get the name of
the next segment file to use. When the segment file is completed, the
reference is added to the LogFile::completed FIFO. Before dequeuing the next
element from LogFileWrite::available, the main thread checks to see if any
elements are available. If not, it call LogFileWrite::need_segment_file().
That function must either wait until it can dequeue an element from
LogFileWrite::available or return an error.

Scanning For Existing Segment Files
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
The new() function for LogFileWrite iterates through segment file names,
staring with a segment number of zero and going up to seg_num\ :sub`max`.
For each segment file, it gets the timestamp from the SegFileHeader.
segment file name. It then goes through this list

If there is an existing segment file, find the oldest segment file. Let i be
the segment number for that segment file. Let n
be the number of existing segment files. Verify that there are n existing
segment files with segment numbers from i to i + n - 1. Also, verify that
there are no segment files with segment numbers from i + n to i - 1. If
the verification fails, returns LogFile::CorruptedLogFile. The user can use
the LogFile::clean() function to correct this error.

Next, add names for all existing segment files to the
LogFileWrite::completed FIFO.

Once any names of existing segment files have been added to
LogFileWrite::completed, add names for segment files that do not exist
to LogFileWrite::avail.

Then, start the secondary thread. It will call LogFileWrite::completed to
allow any previously existing segment files to be processed.

Next, get the name of a segment file from LogFileWrite::available, checking
to see one is available, as described above. If one cannot be obtained,
return the error from LogFileWrite::need_segment_file(). Otherwise,
create the file and a corresponding SegFile object.

Create and return a LogFileWrite object.

Writing Data
````````````
Create a new LogFileWrite object.

The writing of data is a loop through the size of the passed data. Before
entering the loop, set i to zero. This is the amount of data written, which
is incremented in the loop described in the following steps.

o   If there is enough room in the segment file to write a data header and
    all of the remaining data in the data section, write both items and exit
    the loop.

o   If there is sufficient room in the segment file to write a data header
    and at least one byte, write a data header and as much data as will will
    fit in the data section. Then start a new segment file, which will become
    the current segment file.

o   Otherwise, there is not enough room to write even one byte of data, so
    start a new segment file, which becomes the segment file.

The data header written in the above loop should have its timestamp set to
the current time and its rec_num set to the rec_num from the LogFileWrite.
The first time data is written to the loop, first should be set to true and
to false for subsequent writes. The actual data written should the the
value returns from DataHeader::to_le_bytes().

At the end of the loop, increment LogFileWrite rec_num.

Reading Data
````````````
Create a new LogFileRead object.

Restrictions
============
o   The only function in LogFileWrite that can call memory allocation functions
    is new().

o   The value of max_seg minus the sum of LogFileWrite::PACKED_LEN and
    DataHeader::PACKED_LEN must be at least equal to
    LogFileWrite::MIN_DATA_SECTION. If this is not true, the error
    LogError::SegSizeTooSmall is returned.

o   The value of max_seg passed to LogFileRead::new() or LogFileWrite::new()
    is less than twice the value of max_file passed to LogFileRead::new()
    or LogFileRead::new(), the value LogError::TooFewSegs will be
    returned from the two new() functions.

o   Prefix and suffix values must not contain the path delimiters. If they
    do, the return value must be LogError::PathDelimiterNotAllowed
