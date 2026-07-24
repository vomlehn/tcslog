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

Data Structures
===============
LogFileWrite
`````````````
   Interface for writing data to log files whose size is strictly limited
   according the parameters passwed. Each log file is comprised of
   multiple segments

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

Functions
~~~~~~~~~
   fn new(dir: &str, prefix: &str, suffix: &str, max_file: u64,
      max_seg: u64) -> Result<LogFileWrite, LogFileError>;


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

Starting at the oldest log file segment, call the LogFileWrite seg_complete()
function.

