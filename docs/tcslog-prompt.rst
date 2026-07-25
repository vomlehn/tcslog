=============
TcsLog Prompt
=============
.. contents:: Table of Contents
   :depth: 4
   :local:

Introduction
============
Create a Rust library named tcslog for logging data. Each logical log file is
stricty limited according a user specified size. The logical log file is
comprised of multiple physical files, each of which contains a segment of
the logical file. This approach has several advantages:

-  If filesystem corruption causes a segment file to become unavailable, the
   remaining segment files can still be read.

-  The log file can be downlinked one at a time.

General
=======
Data in log files is always packed, i.e. has no padding

All integer values in log files is in little endian format,
necessitating to_le_bytes() and from_le_bytes() functions for multi-byte data
values.

Each segment file begins with a packed log file header, i.e. the value from
SegFileHeader::to_le_bytes(), followed by zero or more data records. 

Data records consist of a packed data header, i.e. the value from
DataHeader::to_le_bytes(), followed by zero or more bytes of data. Following
the data header is the record data, which has up to
max_seg - LogHeader::PACKED_LEN bytes. The data from a call to write_str() or
write_bytes() may span multiple segment files.

Writing Data
````````````
Create a new LogFileWrite object.

The writing of data is a loop through the size of the passed data. Before
entering the loop, set i to zero. This is the offset within the data being
written.
Let remaining_size be the difference between the offset in the current segment
file and file\ :sup:`max`. 

If remaining_size is less than or equal to DataHeader::PACKED_LEN, get a new
name segment name from avail_q and create a new segment file. Call
LogFileWrite::seg_completion() with the name of the segment file.

Set to_write to the size of data passed to write_bytes minus i. Set
avail_size to remaining_size minus DataHeader::PACKED_LEN.
If avail_size is less than or equal
to_write:

o   Create a DataHeader with:
    
    rec_num
        Set to LogFileWrite::rec_num

    first
        Set to true if this is the first write for this call to write_bytes()

    len
        Set to to_write

o   Otherwise, create a DataHeader with:
    
    rec_num
        Set to LogFileWrite::rec_num

    first
        Set to true if this is the first write for this call to write_bytes()

    len
        Set to avail_size.

Write the result of DataHeader::to_le_bytes()

Increment i by the number of bytes written. If this is equal the the number
of bytes in the data, exit the loop. Otherwise, go to the top of the loop.
Go back through the loop until all data is written.

Finally increment LogFileWrite::rec_num.

Reading Data
````````````
Create a new LogFileRead object.

Data Structures
===============
Filename
````````
    Name of a segment file. This is the concatonation of the prefix, the
    segment file number as a zero-filled hex value, and the suffix.

Timestamp
`````````
    Time in nanoseconds

RecNum
``````
    Record number. This is a u64 value.

SegNum
``````
    Segment number. This is a u8 value.

LogFileError
````````````
    Enum for error values returned.

LogFileWrite
`````````````
   Interface for writing data to log files whose size is strictly limited
   according the parameters passwed. Each log file is comprised of
   multiple segments

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
      max_seg: u64) -> Result<LogFileWrite, LogFileError>;

      dir
        Directory in which the log file, i.e. all segment files, will be
        created.

      prefix
        First part of segment file names.

      suffix
        End of the segment file names

      max_file
        Maximum size of the log file. This is referred to as file\ :sub:`max`
        in the rest of this document.

      max_seg
        Maximum size of each segment file.

   fn write_str(msg: &str) -> LogFileError;
      Writes the msg to the log file. It does this by passing msg to
      write_bytes().

   fn write_bytes(msg: &u8[]) -> LogFileError;
      Writes a buffer of u8 values to the log file

   fn seg_complete(name: &str);
      Function called when a segment is complete. This is generally used to
      transmit the log file. It must either move or delete the file before
      returning

   fn log_full(name: &str);
      Called when there are max_seg log file segments existing. The name is
      that of the next segment file to be used. Possible actions:
      
      o The function could wait for the segment file to be sent.

      o The function could remove the segment file.

      The function must remove the named file before returning.

LogFileRead
```````````
Interface for reading from log files.

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
      max_seg: u64) -> Result<LogFileRead, LogFileError>;

      dir
        Directory in which the log file, i.e. all segment files, will be
        created.

      prefix
        First part of segment file names.

      suffix
        End of the segment file names

      max_file
        Maximum size of the log file. This is referred to as file\ :sub:`max`
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
This holds information for the log file header. This appears as the first
thing in each segment file.

Constants
~~~~~~~~~
    PACKED_LEN
        The sum of bytes required in each element of SegFileHeader.  This is
        the number of bytes returned by to_le_bytes() and passed to
        from_le_bytes()

Data
~~~~
    first
        Boolean that is true for the first segment in a log file, false
        otherwise

    timestamp
        Creation time for this segment

Functions
~~~~~~~~~
    fn new(first: bool) -> Result<SegFileHeader, LogFileError>;

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
        Name of the log file, including the directory.

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
Initialization
``````````````
Use two FIFOs, each containing a reference to 
Create an array to hold information on all log file segments. It then goes
through all segments, reading the log file creation timestamp if the file
exist. 

Start a thread that will be responsible for calling TcsLogWrite::seg_complete().

Restrictions
============
o   The only function in LogFileWrite that can call memory allocation functions
    is new().

o   It is an error (LogFileError::SegTooSmall) if the sum of
    LogFileWrite::PACKED_LEN, DataHeader::PACKED_LEN, and 128 is greater
    than the value of max_seg passed to LogFileWrite::new() or
    LogFileRead::new().


o   The value of max_seg passed to LogFileRead::new() or LogFileWrite::new()
    is less than twice the value of max_file passed to LogFileRead::new()
    or LogFileRead::new(), the value LogFileError::TooFewSegs will be
    returned from the two new() functions.

o   Prefix and suffix values must not contain the path delimiters. If they
    do, the return value must be LogFileError::PathDelimiterNotAllowed
