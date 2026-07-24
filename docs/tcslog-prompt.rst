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
LogFileHeader::to_le_bytes(), followed by zero or more data records. Data
records consist of a packed data header, i.e. the value from
DataHeader::to_le_bytes(), followed by zero or more bytes of data. Data
may be split across multiple segment files. If the number of bytes in the
segment file minus the max_seg is less than the size of the packed data, i.e.
header DataHeader::PACKED_LEN, a new segment file is created


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
        The name of the current

    seg_num
        Segment number for the segment file to which writing is currently
        being done. This increments each time a segment number is created.

Functions
~~~~~~~~~
   fn new(dir: &str, prefix: &str, suffix: &str, max_file: u64,
      max_seg: u64) -> Result<LogFileWrite, LogFileError>;

   fn log_str(msg: &str) -> LogFileError;
      Writes the msg to the log file

   fn log_bytes(msg: &u8[]) -> LogFileError;
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
Interface for reading from log files

LogFileHeader
`````````````
This holds information for the log file header. This appears as the first
thing in each segment file.

Constants
~~~~~~~~~
    PACKED_LEN
        Number of bytes returned by to_le_bytes() and passed to from_le_bytes()

Data
~~~~
    first
        Boolean that is true for the first segment in a log file, false
        otherwise

    timestamp
        Creation time for this segment

Functions
~~~~~~~~~
    fn new(first: bool) -> Result<LogFileHeader, LogFileError>;

    fn to_le_bytes(&self) -> u8[Self::PACKED_LEN];
        Returns a buffer with the values in LogFileHeader with no padding and
        integer values in little endian order,

    fn from_le_bytes(packed: &u8[Self::PACKED_LEN] -> LogFileHeader;
        Returns a LogFileHeader value which is the reverse of the operation
        performed by to_le_bytes().

DataHeader
``````````
    Holds information on a data record and is written before the actual data

    first
        Boolean that is true if this is the first part of a data record abd
        false otherwise

    num
        Record number. This is a RecNum value.

    timestamp
        Time at which the record was written.


Functions
~~~~~~~~~
    fn new(first: bool) -> DataHeader;

    fn to_le_bytes(&self) -> u8[Self::PACKED_LEN];
        Returns a buffer with the values in the DataHeader with no padding and
        integer values in little endian order.

    fn from_le_bytes(packed: &u8[Self::PACKED_LEN] -> LogFileHeader;
        Returns a DataHeader value which is the reverse of the operation
        performed by to_le_bytes().

Timestamp
`````````
This is like the standard Rust Duration type but with to_le_bytes() and
from_le_bytes() functions.

Logic
=====
Initialization
``````````````
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
