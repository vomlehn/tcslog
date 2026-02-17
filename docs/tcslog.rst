======
TcsLog
======
.. contents:: Table of Contents
   :depth: 4
   :local:

Introduction
============
TcsLog is an outgrowth of Tcspecial for logging telemetry information. It features:

* Fixed length logs with automatic switching to new logs when old ones fill.

* Arbitrary record sizes

* Self-identified log files (the name is in the header)

* Indexed by automatically supplied timestamps with nanosecond resolution (if supported
  by host operating system).

* Metadata all in little-endian form.

* Written in Rust

General Structure
=================
Log files are comprised of BLOCK_SIZE byte file blocks. Every file block begins on
a BLOCK_SIZE boundary. Log files have three section, in order they appear in the file:

Header
    A single file block witih version, description of the location of data
    in the file, file name, and other housekeeping information. The
    size of this file section in bytes is HEADER_SIZE.
    
Index
    This is a binary-search oriented index at the beginning of the
    file. This occupies INDEX_SIZE bytes, which is a multiple of
    BLOCK_SIZE  and is aligned on a BLOCK_SIZE
    boundary.

Data
    Logged data. This starts immediately after the Index section and
    extends to the end of the file. It has a length, in bytes,
    given by DATA_SIZE.

The entire file must not be longer than
FILE_SIZE bytes.

**High-Level View of Log File**

.. code-block:: text

   +=================+=================+
   | Header          | version         |
   |                 +-----------------+
   |                 | max_size        |
   |                 +-----------------+
   |                 | index pointer   |--+
   |                 +-----------------+  |
   |                 | data pointer    |--+--+
   +=================+=================+  |  |
   | Index           | Index level 0   |<-+  |
   +=================+=================+     |
   | Data            |Data block 0     |<----+
   +=================+=================+


Theory of Operation
===================
A simple log file would consist of records with a length, followed by the
data. One obvious extension adds a header block at the beginning of the
file, indicating the format version, maximum length, and a string identifying
the name, and possibly other information.

Adding Headers to Data File Blocks
----------------------------------

This simple log file is enhanced by adding a header in each BLOCK_SIZE data
file block. This header is a u64 in little-endian form, as follows:

**Block Header**

+---------------------+--------+-------------------------------------+-------------+
| Upper 56 bits       | Lower  | Description                         | Name        |
|                     | 8 bits |                                     |             |
+=====================+========+=====================================+=============+
| 0x0000_0000_0000_00 | 0x00   | Pointer does not reference a file   | TCSLOG_NULL |
+---------------------+--------+-------------------------------------+-------------+
| 0x0000_0000_0000_00 | 0x01   | Next record starts at end of header | TCSLOG_REC  |
+---------------------+--------+-------------------------------------+-------------+
| Non-zero            | 0x00   | Offset to end of current record     | n/a         |
+---------------------+--------+-------------------------------------+-------------+
| Non-zero            | >0x00  | Invalid value                       | n/a         |
+---------------------+--------+-------------------------------------+-------------+

Invalid values should not appear. When attempting to recover a corrupted
log file, advancing to the next file block and inspecting its header may
allow progress in recovery efforts.

Note that the TCSLOG_NULL and TCSLOG_REC offset values are chosen to point
to the file header.  This is neither an index nor data block and so the offsets
won't be mistake for such blocks.

Adding Logging Facility-Supplied Timestamps
-------------------------------------------
The next addition is a logging facility-supplied timestamp. This is added
after the data record length. Note that individual devices and software
may provide their own timestamp, which can generally be expected to differ
from the one supplied by the logging facility. In many cases, after locating
a logging record with a given time, it may be necessary to back up some
file blocks to find a device or software logging record with a corresponding
timestap.

Log File Indices
----------------
.. note::
   This takes advantage of filesystems where unwritten blocks read as
   zeroed blocks. If ported to an operating system where this is not
   true, the code should provide a similar capability, perhaps with
   a bit vector identifying the blocks that haven't been written.

A log file may be quite large. Because of this, an index is maintained
that allows quick searching for the start of a record after a given time.

Indices consist of one or more index blocks. Index blocks are
comprised of BLOCK_SIZE / (2 * size_of(u64)) structures. Each 
index structure has two u64 elements:

offset
    The offset of a file block within the file. This will be FILE_NULL if
    the structure does not refer to any file block. If pointing to a data
    block, the data block starts with the beginning of a log record
    with a logging facilitated timestamp greater or equal to the timestamp
    in this index structure. If pointing to an index block, points to the
    beginning of a index block with the timestamp in this index structure.

timestamp
    Nanosecond resolution value relative to the UNIX beginning of epoch.

Computing Index Sizes
---------------------
We know:

* INDEX_SIZE and DATA_SIZE must be at least one.

* FILE_SIZE <= HEADER_SIZE + INDEX_SIZE + DATA_SIZE

* Each index block can reference BLOCK_SIZE / size_of (index structure)
  file blocks.

The index is comprised of at least one level, level 1, up to level n. The
index structures in level n reference data blocks, whereas any lower levels
reference index blocks. The header has a reference to the level 1 index
block and to the first block of the data section.

The index blocks in index level i are referenced
by the header block, if i is one, or by index blocks in index
level i - 1 if i is two or greater. 
The references in index level i either all reference data blocks or
index blocks in index level i + 1.

Each data block must be referenced by no more than one reference in
the highest index level blocks. Each index block may be referenced
by no more than one index block.

The number of index blocks must be minimized.

Log File Names
--------------
Log file names are kept unique by including the timestamp in the 52-character
name. They consist
of an arbirary string of up to 32 characters, followed by a hyphen, followed by the
lowercase hexadecimal timestamp of the file creation with underlines as separators
every 4 hexadecimal characters.

In the case that a log file can't be created, the logging facility will wait
100 microseconds to allow the timestamp to advance, then try again. Other
failures will cause a 1 second pause before trying again.

Log File Header Block
=====================
The file header block has the following elements:

File type
    An eight-byte element containing the string "tcslog  ".

Version
    A eight-byte string with a two-digit major release number, a period, a two-digit
    minor release number, a period, and a two digit patch number.

Timestamp
    A little-endian u64 time value in nanoseconds since the UNIX epoch. Note that the host
    operating system may support less resolution but the maximum available
    resolution should be used.

File name
    The 52-character name plus a NUL-terminator.

Index offset
    A little-endian u64 offset of the beginning of the index section.

data offset
    A little-endian u64 offset of the beginning of the data section.

End of File Markers
-------------------
End of File (EOF) markers are used in the data section to indicate
that the end of log records has been reached and to specify the
name of the next log file. If no EOF marker appears in a file,
that file is the end of the stream of log records.

The EOF marker consists of a u128 field with all bits set and the
NUL-terminated name of the next log file.

Room for at least one one-byte record must be available in a log file
When a log
file is created, there must be enough available space in the data
section for a minimum record and an end of file marker.

Each time a log record is to be written, a check is made to see whether there
is enough space for that records and an end of file marker. If not,
a new log file is created and the name of that file written to the old
log file. It is an error if the new log file does not have enough room
to write the log record and an end of file marker.

Application Programming Interface (API)
=======================================
There are several functions that allow use of the logging facility.

pub fn tcslog_create(prefix: &str) -> Result<TcsLog, TcsLogError>;
------------------------------------------------------------------
Creates a new TcsLog, using the given prefix.

If successful, returns a TcsLog. Otherwise, returns Err(TcsLogError).

pub fn tcslog_open(prefix: &str, timestamp: Timestamp) -> Result<TcsLog, TcsLogError>;
--------------------------------------------------------------------------------------
Opens an existing TcsLog so that the telemetry records it contains
may be read.

If successful, returns a TcsLog. Otherwise, returns Err(TcsLogError).

pub fn tcslog_write(&self, data: &[u8]) -> Result<(), TcsLogError>;
-------------------------------------------------------------------
Writes the telemetry data to the TcsLog. If there is not enough room
in the current log file, another will be created. It is an error to
write more data than will fit in a newly create log file.

Returns () if the data was written, otherwise Err(TcsLogError).

pub fn tcslog_read(&self, timestamp: &mut Timestamp, data: &mut [u8]) -> Result<u8, TcsLogError>;
-------------------------------------------------------------------------------------------------
Reads the next telemetry record from the TcsLog, stopping with an error
if there are no more records to be read. The timestamp is placed in
Timestamp.

Returns the number of bytes placed in data on success, Err(TcsLogError)
otherwise.

pub fn tcslog_timestamp_offset(&self, timestamp: Time) -> Result<u64, TcsLogError>;
-----------------------------------------------------------------------------------

Given a timestamp, determines the offset in the log file
of the first data block containing
a header with that timestamp or greater.

If no error occured, returns the offset. Otherwise, returns Err(TcsLogError) value.
