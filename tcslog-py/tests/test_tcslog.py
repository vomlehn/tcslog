"""Drives the Python interface through the scenarios tcslog-c/examples/smoke.c
drives the C/C++ one, so that the three languages are held to the same
behaviour.

Run by `make python-test` from the repository root, which points
TCSLOG_LIB at the library cargo just built.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import List

import tcslog

PREFIX = "seg-"
SUFFIX = ".log"


def seg_size(data_bytes: int) -> int:
    return tcslog.segment_file_header_len() + data_bytes


def segment_files(d: str) -> List[str]:
    return sorted(
        n for n in os.listdir(d) if n.startswith(PREFIX) and n.endswith(SUFFIX)
    )


class Counters(tcslog.WriteCallbacks):
    """Counts each callback, and refuses send when asked to."""

    def __init__(self, refuse_send: bool = False) -> None:
        """Starts every count at zero.

        Args:
            refuse_send: Whether ``send`` raises OSError rather than
                         counting quietly.

        """
        self.sends = 0
        self.completes = 0
        self.adjustments = 0
        self.refuse_send = refuse_send

    def send(self, path: str) -> None:
        self.sends += 1
        if self.refuse_send:
            raise OSError(f"refusing {path}")

    def record_complete(self, fd: int) -> None:
        self.completes += 1

    def timer_resolution_adjusted(self, resolution_ns: int) -> None:
        self.adjustments += 1


class Scenarios(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory(prefix="tcslog-py-")
        self.dir = self._tmp.name

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_version_and_header_len(self) -> None:
        major, minor, patch = tcslog.format_version()
        self.assertEqual((major, minor), (1, 0))
        self.assertEqual(tcslog.segment_file_header_len(), 53)
        self.assertEqual(
            tcslog.status_str(tcslog.Status.EOF), tcslog.Eof().args[0]
        )

    def test_round_trip(self) -> None:
        # send counts but does not move the file, so the reader still
        # finds it, as in the C smoke test.
        c = Counters()
        msgs = [
            b"attitude nominal",
            b"battery 71 percent",
            b"downlink window in 420s",
        ]
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_TS_RC,
            c,
        ) as w:
            for m in msgs:
                # A VariableTsRc record carries a sixteen-byte data
                # header.
                self.assertEqual(w.write(m), len(m) + 16)
            self.assertGreater(w.timer_resolution(), 0)
            w.flush()
        self.assertEqual(c.completes, len(msgs))
        self.assertEqual(c.sends, 1)

        buf = bytearray(256)
        got = []
        last = 0
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            while True:
                try:
                    res = r.read(buf)
                except tcslog.Eof:
                    break
                except tcslog.SessionEnd:
                    continue
                got.append(bytes(buf[: res.n]))
                self.assertEqual(res.meta, tcslog.Meta.VARIABLE_TS_RC)
                self.assertGreater(res.timestamp, 0)
                self.assertEqual(res.record_count, last + 1)
                last = res.record_count
            self.assertGreaterEqual(r.segments_opened(), 1)
        self.assertEqual(got, msgs)

    def test_write_accepts_any_bytes_like(self) -> None:
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_SIMPLE,
        ) as w:
            w.write(b"bytes")
            w.write(bytearray(b"bytearray"))
            w.write(memoryview(b"read-only view"))
            w.write_str("text")
            w.write(b"")
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            got = [rec.payload for rec in r]
        self.assertEqual(
            got, [b"bytes", b"bytearray", b"read-only view", b"text", b""]
        )

    def test_fixed_format(self) -> None:
        with tcslog.LogWrite(
            self.dir, PREFIX, SUFFIX, seg_size(4096), tcslog.Format.fixed(8)
        ) as w:
            self.assertEqual(w.write(b"01234567"), 8)
            with self.assertRaises(tcslog.FixedLenMismatch):
                w.write(b"0123456")
            with self.assertRaises(tcslog.FixedLenMismatch):
                w.write(b"012345678")
        buf = bytearray(256)
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            res = r.read(buf)
        self.assertEqual(bytes(buf[: res.n]), b"01234567")
        self.assertEqual(res.meta, tcslog.Meta.FIXED)
        self.assertEqual((res.timestamp, res.record_count), (0, 0))

    def test_invalid_format(self) -> None:
        with self.assertRaises(tcslog.InvalidFormat):
            tcslog.LogWrite(
                self.dir, PREFIX, SUFFIX, seg_size(4096), tcslog.Format(3)
            )
        with self.assertRaises(tcslog.SegSizeTooSmall):
            tcslog.LogWrite(
                self.dir, PREFIX, SUFFIX, 1, tcslog.Format.VARIABLE_SIMPLE
            )
        with self.assertRaises(tcslog.PathDelimiterNotAllowed):
            tcslog.LogWrite(
                self.dir,
                "a/b",
                SUFFIX,
                seg_size(4096),
                tcslog.Format.VARIABLE_SIMPLE,
            )

    def test_no_log_to_read(self) -> None:
        with self.assertRaises(tcslog.NoSegmentFiles):
            tcslog.LogRead(self.dir, PREFIX, SUFFIX)

    def test_read_overflow(self) -> None:
        payload = bytes(ord("a") + i % 26 for i in range(200))
        nxt = b"the record after it"
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_SIMPLE,
        ) as w:
            w.write(payload)
            w.write(nxt)
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            small = bytearray(16)
            with self.assertRaises(tcslog.ReadOverflow) as cm:
                r.read(small)
            self.assertEqual(cm.exception.n, 16)
            self.assertEqual(
                cm.exception.result.meta, tcslog.Meta.VARIABLE_SIMPLE
            )
            self.assertEqual(bytes(small), payload[:16])
            # The rest was skipped, so this is the next record.
            big = bytearray(256)
            res = r.read(big)
            self.assertEqual(bytes(big[: res.n]), nxt)
            with self.assertRaises(tcslog.Eof):
                r.read(big)

    def test_read_needs_writable_buffer(self) -> None:
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_SIMPLE,
        ) as w:
            w.write(b"x")
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r, self.assertRaises(
            TypeError
        ):
            r.read(bytes(16))

    def _write_many(self, count: int) -> None:
        """Writes a log of numbered records into 32-byte segment files.

        Args:
            count: How many records to write, at most 100, each payload
                   being ``record`` and the record's two-digit number.

        """
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(32),
            tcslog.Format.VARIABLE_SIMPLE,
        ) as w:
            for i in range(count):
                w.write(b"record%02d" % i)

    def test_truncation(self) -> None:
        count = 12
        self._write_many(count)
        names = segment_files(self.dir)
        self.assertGreaterEqual(len(names), 4)
        # The second file, so the loss is in the middle.
        os.unlink(os.path.join(self.dir, names[1]))

        records = truncations = lost = 0
        buf = bytearray(256)
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            while True:
                try:
                    r.read(buf)
                    records += 1
                except tcslog.Eof:
                    break
                except tcslog.SessionEnd:
                    pass
                except tcslog.ReadTruncated as e:
                    truncations += 1
                    lost += e.lost
        self.assertGreaterEqual(truncations, 1)
        self.assertEqual(lost, 1)
        self.assertGreater(records, 0)
        self.assertLess(records, count)

    def test_iter(self) -> None:
        payload = b"z" * 100
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_TS_RC,
        ) as w:
            w.write(b"whole")
            w.write(payload)
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            recs = list(r.iter())
        self.assertEqual([x.payload for x in recs], [b"whole", payload])
        self.assertFalse(any(x.truncated for x in recs))
        self.assertEqual([x.record_count for x in recs], [1, 2])

    def test_send_failure(self) -> None:
        c = Counters(refuse_send=True)
        w = tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(32),
            tcslog.Format.VARIABLE_SIMPLE,
            c,
        )
        with self.assertRaises(tcslog.IoError) as cm:
            for i in range(32):
                w.write(b"record%02d" % i)
        self.assertIsInstance(cm.exception.__cause__, OSError)
        self.assertGreaterEqual(c.sends, 1)
        w.close()

    def test_clear(self) -> None:
        self._write_many(12)
        self.assertTrue(segment_files(self.dir))
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(32),
            tcslog.Format.VARIABLE_SIMPLE,
        ) as w:
            w.clear()
            self.assertEqual(segment_files(self.dir), [])

    def test_timer_resolution_callback(self) -> None:
        c = Counters()
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_SIMPLE,
            c,
        ) as w:
            w.write(b"telemetry")
        self.assertEqual(c.adjustments, 0)

    def test_two_contexts(self) -> None:
        da = os.path.join(self.dir, "a")
        db = os.path.join(self.dir, "b")
        os.mkdir(da)
        os.mkdir(db)
        a, b = Counters(), Counters()
        wa = tcslog.LogWrite(
            da, PREFIX, SUFFIX, seg_size(4096), tcslog.Format.VARIABLE_SIMPLE, a
        )
        wb = tcslog.LogWrite(
            db, PREFIX, SUFFIX, seg_size(4096), tcslog.Format.VARIABLE_SIMPLE, b
        )
        wa.write(b"one")
        wb.write(b"one")
        wb.write(b"two")
        wa.close()
        wb.close()
        self.assertEqual((a.completes, b.completes), (1, 2))
        self.assertEqual((a.sends, b.sends), (1, 1))

    def test_buffer_is_pinned_and_reused(self) -> None:
        with tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_SIMPLE,
        ) as w:
            src = bytearray(b"pinned")
            w.write(src)
            pin = w._pin
            w.write(src)
            self.assertIs(
                w._pin, pin, "writing the same buffer again makes no new view"
            )
            del pin  # the test's own reference would keep the buffer pinned
            with self.assertRaises(BufferError):
                src.extend(b"!")
        src.extend(b"!")  # released by the close

        buf = bytearray(64)
        r = tcslog.LogRead(self.dir, PREFIX, SUFFIX)
        r.read(buf)
        pin = r._pin
        r.read(buf)
        self.assertIs(
            r._pin, pin, "reading into the same buffer again makes no new view"
        )
        del pin
        with self.assertRaises(BufferError):
            buf.extend(b"x")
        other = bytearray(64)
        with self.assertRaises(tcslog.Eof):
            r.read(other)
        buf.extend(b"x")  # released when another buffer was passed
        r.close()
        other.extend(b"x")  # and by the close

    def test_queries(self) -> None:
        with tcslog.LogWrite(
            self.dir, PREFIX, SUFFIX, seg_size(32), tcslog.Format.VARIABLE_TS_RC
        ) as w:
            # Enough files that taking the headers drains them from C in
            # more than one chunk.
            for _ in range(40):
                w.write(b"telemetry")
            session = w.session_id()
            self.assertGreater(w.current_segment_id(), session)
            meta = w.last_meta()
            self.assertEqual(meta.meta, tcslog.Meta.VARIABLE_TS_RC)
            self.assertEqual(meta.record_count, 40)

        # The session identifier is the first segment file's name.
        names = segment_files(self.dir)
        self.assertEqual(names[0], PREFIX + str(session) + SUFFIX)
        self.assertEqual(tcslog.SegId.parse(str(session)), session)
        with self.assertRaises(tcslog.InvalidPathname):
            tcslog.SegId.parse("not-an-identifier")
        self.assertEqual(
            tcslog.format_timestamp(0), "1970-01-01T00:00:00.000000000Z"
        )

        buf = bytearray(256)
        with tcslog.LogRead(self.dir, PREFIX, SUFFIX) as r:
            self.assertIsNone(r.current_header())
            r.collect_opened_headers(True)
            res = r.read(buf)
            header = r.current_header()
            assert header is not None
            self.assertEqual(header.session_id, session)
            self.assertEqual(header.max_size, seg_size(32))
            self.assertEqual(header.format, tcslog.Format.VARIABLE_TS_RC)
            self.assertEqual(r.last_meta().record_count, res.record_count)
            for _ in r:
                pass
            headers = r.take_opened_headers()
            self.assertEqual(len(headers), r.segments_opened())
            self.assertEqual(len(headers), len(names))
            self.assertGreater(len(headers), 16)
            self.assertEqual(
                [h.sequence for h in headers], list(range(len(headers)))
            )
            self.assertEqual(r.take_opened_headers(), [])

    def test_closed_handles(self) -> None:
        w = tcslog.LogWrite(
            self.dir,
            PREFIX,
            SUFFIX,
            seg_size(4096),
            tcslog.Format.VARIABLE_SIMPLE,
        )
        w.write(b"x")
        w.close()
        w.close()
        with self.assertRaises(ValueError):
            w.write(b"y")
        r = tcslog.LogRead(self.dir, PREFIX, SUFFIX)
        r.close()
        r.close()
        with self.assertRaises(ValueError):
            r.read(bytearray(8))


@unittest.skipIf(
    sys.platform in ("darwin", "win32") or not os.environ.get("TCSLOG_LIB"),
    "the installed layout is ELF's, and needs TCSLOG_LIB to copy from",
)
class InstalledLayout(unittest.TestCase):
    """The package finds the library in the tree `make install` made."""

    def test_finds_library_three_directories_up(self) -> None:
        # The tree make install makes under its PREFIX, built in a
        # temporary directory: the package in lib/python3/site-packages
        # and the library in lib under its soname. Nothing tells the
        # copy where the library is, so finding it is the package's own
        # doing.
        with tempfile.TemporaryDirectory(prefix="tcslog-py-") as d:
            lib = Path(d) / "lib"
            site = lib / "python3" / "site-packages"
            package = Path(tcslog.__file__).resolve().parent
            shutil.copytree(
                package,
                site / "tcslog",
                ignore=shutil.ignore_patterns("__pycache__"),
            )
            installed = lib / tcslog._INSTALLED_NAME
            shutil.copy(os.environ["TCSLOG_LIB"], installed)
            env = {
                k: v
                for k, v in os.environ.items()
                if k not in ("TCSLOG_LIB", "LD_LIBRARY_PATH")
            }
            out = subprocess.run(
                [
                    sys.executable,
                    "-I",
                    "-c",
                    "import sys; sys.path.insert(0, sys.argv[1]); "
                    "import tcslog; print(tcslog._lib._name)",
                    str(site),
                ],
                env=env,
                cwd=d,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            self.assertEqual(out, str(installed))


if __name__ == "__main__":
    unittest.main()
