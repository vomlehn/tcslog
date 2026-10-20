#!/usr/bin/env python3
"""Writes telemetry through the Python interface the way a vehicle would,
then reads back what is left in the log.

This is tcslog-c/examples/downlink.c in Python, and does what it does:
the first writer hands each filled segment file to a `send` callback
that renames it into a "downlinked" subdirectory, out of the log's
namespace; the second has no `send` at all, so its filled files stay in
the log, as on a vehicle that has not reached a ground station; and a
reader then reads what remains.

The record buffer is allocated once and reused for every read, which is
how the Rust and C/C++ interfaces are used too.

Build and run it with bin/run-python-example, which gives it a
temporary directory.
"""

import os
import sys
from typing import List

import tcslog

# Small enough that a few records fill a segment file, so the example
# actually exercises a handover rather than describing one.
SEG_DATA_BYTES = 128
RECORD_COUNT = 40
BUF_LEN = 256


class Quiet(tcslog.WriteCallbacks):
    """Reports a widened timer resolution, and has no `send`."""

    def __init__(self) -> None:
        self.downlinked = 0

    def timer_resolution_adjusted(self, resolution_ns: int) -> None:
        # The figure to build with next time, so a deployed system
        # records it and carries on.
        print(
            f"note: timer resolution widened to {resolution_ns} ns; build with "
            f"TIMER_RESOLUTION={resolution_ns} next time",
            file=sys.stderr,
        )


class Downlink(Quiet):
    """As Quiet, and takes each filled segment file out of the log."""

    def __init__(self, downlink_dir: str) -> None:
        super().__init__()
        self.downlink_dir = downlink_dir

    def send(self, path: str) -> None:
        # Raising makes the write that triggered this raise IoError,
        # which is the right answer: if the file cannot be taken, the
        # storage bound the log was given has stopped holding.
        dest = os.path.join(self.downlink_dir, os.path.basename(path) + ".sent")
        os.rename(path, dest)
        self.downlinked += 1


def write_telemetry(log_dir: str, handler: Quiet) -> None:
    with tcslog.LogWrite(
        log_dir,
        "seg-",
        ".tcslog",
        tcslog.segment_file_header_len() + SEG_DATA_BYTES,
        tcslog.Format.VARIABLE_TS_RC,
        handler,
    ) as w:
        print(f"timer resolution in force: {w.timer_resolution()} ns")
        for i in range(1, RECORD_COUNT + 1):
            # A write that fails is worth stopping for: unlike a read,
            # there is no later call that recovers the record, so the
            # exception is left to propagate.
            w.write_str(f"batt={24 - i // 20}.{(i * 7) % 100:02d}V temp={-40 + i}C")
        w.flush()
        # Closing, at the end of the with, flushes the segment file being
        # written and hands it to `send` too if it holds any records.
    print(
        f"wrote {RECORD_COUNT} records; {handler.downlinked} segment file(s) handed to send so far"
    )


def read_remaining(log_dir: str) -> None:
    try:
        r = tcslog.LogRead(log_dir, "seg-", ".tcslog")
    except tcslog.NoSegmentFiles:
        print("nothing left in the log: every segment file was downlinked")
        return

    buf = bytearray(BUF_LEN)
    records = sessions = lost_files = 0
    with r:
        # The rule is to read again until Eof. Everything else is news
        # about the telemetry rather than a failure of the reader.
        while True:
            try:
                res = r.read(buf)
            except tcslog.Eof:
                break
            except tcslog.SessionEnd:
                # Writing stopped and started again here, so record
                # numbering restarts. Nothing is wrong.
                sessions += 1
                continue
            except tcslog.ReadTruncated as e:
                # The bytes that did arrive are real measurements.
                lost_files += e.lost
                print(f"  -- {e.n} byte(s) of a record recovered; {e.lost} file(s) lost")
                continue
            except tcslog.ReadOverflow as e:
                # The rest of the record was skipped, so reading on
                # returns the record after it.
                print(
                    f"  -- {e.n} byte(s) of a record too large for a {BUF_LEN}-byte "
                    "buffer; the rest was skipped"
                )
                continue
            records += 1
            text = buf[: res.n].decode()
            print(f"  #{res.record_count} {text} ({res.n} bytes, ts={res.timestamp})")
        opened = r.segments_opened()

    line = f"read {records} record(s) from {opened} segment file(s)"
    if sessions:
        line += f", across {sessions} session boundary/boundaries"
    if lost_files:
        line += f", with {lost_files} file(s) lost"
    print(line)


def main(argv: List[str]) -> int:
    if len(argv) != 2:
        print(f"usage: {argv[0]} <directory>", file=sys.stderr)
        return 2
    log_dir = argv[1]
    sent = os.path.join(log_dir, "downlinked")
    os.mkdir(sent)

    major, minor, patch = tcslog.format_version()
    print(f"tcslog stored format {major}.{minor}.{patch}")

    print("\n-- with a send callback: filled files leave the log")
    downlink = Downlink(sent)
    write_telemetry(log_dir, downlink)
    print(f"{downlink.downlinked} segment file(s) are in {sent}, out of the log's namespace")

    print("\n-- with no send callback: filled files stay in the log")
    write_telemetry(log_dir, Quiet())

    print("\n-- reading what is still in the log")
    read_remaining(log_dir)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
