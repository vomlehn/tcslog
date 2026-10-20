"""Python 3 interface to tcslog, a segmented telemetry log.

This module presents the same interface as the Rust crate and the C/C++
binding, through :mod:`ctypes` over ``libtcslog_c``, the shared library
``tcslog-c`` builds. It has no dependencies beyond the standard library
and is not compiled: it calls the very functions a C or C++ program
calls, so a log written from any of the three languages reads in the
other two.

Names follow the Rust crate. Records are written with :class:`LogWrite`
and read with :class:`LogRead`; failures are raised as subclasses of
:class:`LogError`, one per variant of the Rust ``LogError`` and of the C
``TcslogStatus``, carrying what the variant carries. As in Rust,
:class:`Eof`, :class:`SessionEnd`, :class:`ReadTruncated` and
:class:`ReadOverflow` are news about the telemetry rather than failures,
and a read loop continues past all but the first.

Allocation
----------
The library underneath allocates nothing to write or read a record, and
this module keeps to that as far as Python allows:

* :meth:`LogRead.read` places a record in a buffer the caller owns and
  reuses, rather than returning a new ``bytes``.
* Each handle keeps the out-parameters it hands to C for its whole life,
  rather than making them per call.
* The ctypes view of the caller's buffer is made the first time a buffer
  is passed and kept until a different one is, so reading into, or
  writing from, the same buffer again makes no new view. While a handle
  holds that view the buffer cannot be resized, which is what keeps the
  address C was given valid; :meth:`LogRead.close` and
  :meth:`LogWrite.close` release it.
* ``bytes`` passed to :meth:`LogWrite.write` go to C in place.

What remains is unavoidable in Python, and is documented here as the
rest of tcslog documents its exceptions:

* Every value a call returns is a Python object: the
  :class:`ReadResult` a read returns, the integers in it, and the integer
  a write returns. CPython allocates these, there being no way to return
  a value without an object to hold it.
* Every exception is an object, so each :class:`Eof`,
  :class:`SessionEnd`, :class:`ReadTruncated` or :class:`ReadOverflow`
  allocates one. These arrive once per session, loss, or log rather
  than once per record.
* A read-only buffer other than ``bytes``, such as a ``memoryview`` of
  one, is copied before a write, since C must be given a stable address
  and ctypes can take one only from ``bytes`` or a writable buffer.
* Calling a :class:`WriteCallbacks` method from C makes the Python
  objects its arguments need: the path ``send`` is given is a ``str``.
* Opening a handle allocates it, as it does in C/C++ and Rust, along
  with the ctypes callback objects for the methods the handler overrides.
* :meth:`LogRead.iter`, whose records own their payloads, allocates a
  ``bytes`` per record and a 64 KiB buffer per iterator, as in Rust.

The library is found by looking, in order, at the ``TCSLOG_LIB``
environment variable, which names the file; beside this module; on the
system loader's search path, which ``LD_LIBRARY_PATH`` extends; and in
``target/release`` then ``target/debug`` of the repository this module
sits in.
"""

import ctypes
import ctypes.util
import enum
import os
import sys
from pathlib import Path
from typing import NamedTuple, Optional

__all__ = [
    "ClockError",
    "Eof",
    "FixedLenMismatch",
    "Format",
    "FormatTag",
    "InvalidFormat",
    "InvalidHeader",
    "InvalidPathname",
    "IoError",
    "LogError",
    "LogRead",
    "LogWrite",
    "Meta",
    "NoSegmentFiles",
    "NotUtf8",
    "NullArgument",
    "Panic",
    "PathDelimiterNotAllowed",
    "PayloadTooLarge",
    "ReadOverflow",
    "ReadResult",
    "ReadTruncated",
    "Record",
    "SegSizeTooSmall",
    "SessionEnd",
    "Status",
    "TimerResolutionZero",
    "VersionMismatch",
    "WriteCallbacks",
    "format_version",
    "segment_file_header_len",
    "status_str",
]

# Size of the buffer LogRead.iter reads into, as in the Rust crate.
_ITER_BUF_LEN = 64 * 1024


def _lib_name() -> str:
    if sys.platform == "darwin":
        return "libtcslog_c.dylib"
    if sys.platform == "win32":
        return "tcslog_c.dll"
    return "libtcslog_c.so"


def _load() -> ctypes.CDLL:
    env = os.environ.get("TCSLOG_LIB")
    if env:
        return ctypes.CDLL(env)
    name = _lib_name()
    here = Path(__file__).resolve().parent
    beside = here / name
    if beside.exists():
        return ctypes.CDLL(str(beside))
    try:
        return ctypes.CDLL(ctypes.util.find_library("tcslog_c") or name)
    except OSError:
        pass
    # tcslog-py/tcslog/__init__.py, so the repository is two up.
    root = here.parent.parent
    for profile in ("release", "debug"):
        built = root / "target" / profile / name
        if built.exists():
            return ctypes.CDLL(str(built))
    raise OSError(
        f"cannot find {name}: build it with `cargo build -p tcslog-c`, "
        "install it with `make install`, or name it in TCSLOG_LIB"
    )


_lib = _load()


class Status(enum.IntEnum):
    """What a call reported: the C ``TcslogStatus``, with the same values.

    Callers rarely see one of these directly, since every status but
    ``OK`` is raised as the :class:`LogError` subclass of the same name.
    """

    OK = 0
    EOF = 1
    SESSION_END = 2
    READ_TRUNCATED = 3
    READ_OVERFLOW = 4
    CLOCK_ERROR = 5
    FIXED_LEN_MISMATCH = 6
    INVALID_HEADER = 7
    INVALID_PATHNAME = 8
    IO_ERROR = 9
    NO_SEGMENT_FILES = 10
    PATH_DELIMITER_NOT_ALLOWED = 11
    PAYLOAD_TOO_LARGE = 12
    SEG_SIZE_TOO_SMALL = 13
    TIMER_RESOLUTION_ZERO = 14
    VERSION_MISMATCH = 15
    NULL_ARGUMENT = 16
    NOT_UTF8 = 17
    PANIC = 18
    INVALID_FORMAT = 19


class Meta(enum.IntEnum):
    """Which metadata a record carried: the Rust ``Meta`` and C ``TcslogMeta``.

    With ``VARIABLE_TS_RC`` the timestamp and record count are in the
    ``timestamp`` and ``record_count`` of the :class:`ReadResult` or
    :class:`Record`; with the other two those are zero.
    """

    FIXED = 0
    VARIABLE_SIMPLE = 1
    VARIABLE_TS_RC = 2


class FormatTag(enum.IntEnum):
    """The record layouts a log may use: the C ``TcslogFormat``."""

    FIXED = 0
    VARIABLE_SIMPLE = 1
    VARIABLE_TS_RC = 2


class Format(NamedTuple):
    """A log's record layout: the Rust ``Format``.

    Use :meth:`fixed` for fixed-length records, and the class attributes
    ``Format.VARIABLE_SIMPLE`` and ``Format.VARIABLE_TS_RC`` for the
    other two.
    """

    tag: FormatTag
    fixed_len: int = 0

    @classmethod
    def fixed(cls, n: int) -> "Format":
        """Every record holds exactly ``n`` payload bytes and no header."""
        return cls(FormatTag.FIXED, n)


# NamedTuple forbids class attributes of its own type in the body.
Format.VARIABLE_SIMPLE = Format(FormatTag.VARIABLE_SIMPLE)
Format.VARIABLE_TS_RC = Format(FormatTag.VARIABLE_TS_RC)


class ReadResult(NamedTuple):
    """What one successful read produced: the Rust ``ReadResult``.

    ``n`` payload bytes were placed at the front of the caller's buffer.
    """

    n: int
    meta: Meta
    timestamp: int = 0
    record_count: int = 0


class Record(NamedTuple):
    """One record's payload with its metadata, as :meth:`LogRead.iter` yields.

    ``truncated`` is true when ``payload`` is the front of a record whose
    remainder was lost or did not fit the iterator's buffer, so that a
    partial record cannot be mistaken for a whole one.
    """

    meta: Meta
    payload: bytes
    truncated: bool
    timestamp: int = 0
    record_count: int = 0


class LogError(Exception):
    """Base of every error tcslog raises: the Rust ``LogError``.

    ``status`` is the C ``TcslogStatus`` the error corresponds to.
    """

    status = Status.OK

    def __init__(self, *args):
        super().__init__(*args or (status_str(self.status),))


class ClockError(LogError):
    """The real-time clock does not read later than the UNIX epoch."""

    status = Status.CLOCK_ERROR


class Eof(LogError):
    """The log has no more records. Not a failure: it ends a read loop."""

    status = Status.EOF


class FixedLenMismatch(LogError):
    """A fixed-format payload is not the record length, or that length is 0."""

    status = Status.FIXED_LEN_MISMATCH


class InvalidHeader(LogError):
    """A segment file's header could not be read."""

    status = Status.INVALID_HEADER


class InvalidPathname(LogError):
    """The directory, prefix, or suffix cannot name a log."""

    status = Status.INVALID_PATHNAME


class IoError(LogError):
    """The operating system, or a callback, reported an error.

    When a :class:`WriteCallbacks` method raised, that exception is this
    one's ``__cause__``.
    """

    status = Status.IO_ERROR


class NoSegmentFiles(LogError):
    """The directory holds no segment file of this log."""

    status = Status.NO_SEGMENT_FILES


class PathDelimiterNotAllowed(LogError):
    """A prefix or suffix contains the path separator."""

    status = Status.PATH_DELIMITER_NOT_ALLOWED


class PayloadTooLarge(LogError):
    """The payload is larger than a record can hold."""

    status = Status.PAYLOAD_TOO_LARGE


class ReadOverflow(LogError):
    """The record was larger than the buffer.

    ``n`` bytes of real telemetry reached the front of the buffer, and
    ``result`` describes them as a :class:`ReadResult` would. The rest of
    the record was skipped, so the next read starts at the record after
    it.
    """

    status = Status.READ_OVERFLOW

    def __init__(self, result: ReadResult):
        super().__init__()
        self.result = result
        self.n = result.n


class ReadTruncated(LogError):
    """Telemetry was lost; read again to continue.

    ``n`` bytes recovered of a record cut short are at the front of the
    buffer, described by ``result``, and ``lost`` segment files were
    found missing.
    """

    status = Status.READ_TRUNCATED

    def __init__(self, result: ReadResult, lost: int):
        super().__init__()
        self.result = result
        self.n = result.n
        self.lost = lost


class SegSizeTooSmall(LogError):
    """``seg_size_max`` leaves no room for a data record."""

    status = Status.SEG_SIZE_TOO_SMALL


class SessionEnd(LogError):
    """Writing stopped and started again here; read again to continue."""

    status = Status.SESSION_END


class TimerResolutionZero(LogError):
    """The library was built without ``TIMER_RESOLUTION``."""

    status = Status.TIMER_RESOLUTION_ZERO


class VersionMismatch(LogError):
    """A segment file was written by a version this build cannot read."""

    status = Status.VERSION_MISMATCH


class NullArgument(LogError):
    """A required argument was missing. Indicates a bug in this module."""

    status = Status.NULL_ARGUMENT


class NotUtf8(LogError):
    """A directory, prefix, or suffix is not valid UTF-8."""

    status = Status.NOT_UTF8


class Panic(LogError):
    """The library panicked; the handle it happened on is unusable."""

    status = Status.PANIC


class InvalidFormat(LogError):
    """The format tag is not one of the three formats."""

    status = Status.INVALID_FORMAT


_ERRORS = {
    cls.status: cls
    for cls in (
        ClockError,
        Eof,
        FixedLenMismatch,
        InvalidHeader,
        InvalidPathname,
        IoError,
        NoSegmentFiles,
        PathDelimiterNotAllowed,
        PayloadTooLarge,
        SegSizeTooSmall,
        SessionEnd,
        TimerResolutionZero,
        VersionMismatch,
        NullArgument,
        NotUtf8,
        Panic,
        InvalidFormat,
    )
}


class _CallbacksStruct(ctypes.Structure):
    pass


_SendFn = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_char_p)
_RecordCompleteFn = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_int)
_TimerResolutionAdjustedFn = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_uint64)

_CallbacksStruct._fields_ = [
    ("send", _SendFn),
    ("record_complete", _RecordCompleteFn),
    ("timer_resolution_adjusted", _TimerResolutionAdjustedFn),
    ("ctx", ctypes.c_void_p),
]


class _ReadResultStruct(ctypes.Structure):
    _fields_ = [
        ("n", ctypes.c_uint32),
        ("meta", ctypes.c_int32),
        ("timestamp", ctypes.c_uint64),
        ("record_count", ctypes.c_uint64),
        ("lost", ctypes.c_uint64),
    ]


_P = ctypes.c_void_p
_Status = ctypes.c_int32


def _declare(name, restype, *argtypes):
    f = getattr(_lib, name)
    f.restype = restype
    f.argtypes = argtypes
    return f


_format_version = _declare(
    "tcslog_format_version",
    None,
    ctypes.POINTER(ctypes.c_uint32),
    ctypes.POINTER(ctypes.c_uint32),
    ctypes.POINTER(ctypes.c_uint32),
)
_segment_file_header_len = _declare("tcslog_segment_file_header_len", ctypes.c_uint32)
_status_str = _declare("tcslog_status_str", ctypes.c_char_p, _Status)
_write_open = _declare(
    "tcslog_write_open",
    _Status,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_uint32,
    ctypes.c_uint32,
    ctypes.c_uint32,
    ctypes.POINTER(_CallbacksStruct),
    ctypes.POINTER(_P),
)
_write_record = _declare(
    "tcslog_write_record", _Status, _P, _P, ctypes.c_size_t, ctypes.POINTER(ctypes.c_uint32)
)
_write_flush = _declare("tcslog_write_flush", _Status, _P)
_write_clear = _declare("tcslog_write_clear", _Status, _P)
_write_timer_resolution = _declare(
    "tcslog_write_timer_resolution", _Status, _P, ctypes.POINTER(ctypes.c_uint64)
)
_write_close = _declare("tcslog_write_close", None, _P)
_read_open = _declare(
    "tcslog_read_open",
    _Status,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.c_char_p,
    ctypes.POINTER(_P),
)
_read_record = _declare(
    "tcslog_read_record",
    _Status,
    _P,
    _P,
    ctypes.c_size_t,
    ctypes.POINTER(_ReadResultStruct),
)
_read_segments_opened = _declare(
    "tcslog_read_segments_opened", _Status, _P, ctypes.POINTER(ctypes.c_uint64)
)
_read_close = _declare("tcslog_read_close", None, _P)


def format_version() -> tuple:
    """The stored format version this build reads and writes, as
    ``(major, minor, patch)``."""
    major, minor, patch = ctypes.c_uint32(), ctypes.c_uint32(), ctypes.c_uint32()
    _format_version(ctypes.byref(major), ctypes.byref(minor), ctypes.byref(patch))
    return (major.value, minor.value, patch.value)


def segment_file_header_len() -> int:
    """The bytes a segment file's own header occupies."""
    return _segment_file_header_len()


def status_str(status: int) -> str:
    """A short description of a :class:`Status`."""
    return _status_str(status).decode()


def _raise(status: int) -> None:
    if status != Status.OK:
        cls = _ERRORS.get(status)
        if cls is None:
            raise LogError(f"{status_str(status)} ({status})")
        raise cls()


class _Pin:
    """The ctypes view of a caller's buffer, and the address C is given.

    Holding the view keeps the buffer exported, so it cannot be resized
    or moved; that is what lets the address be reused while the same
    buffer keeps being passed.
    """

    __slots__ = ("obj", "view", "ptr", "cap")

    def __init__(self, obj, mv):
        self.obj = obj
        self.cap = mv.nbytes
        self.view = ctypes.c_char.from_buffer(mv) if self.cap else None
        self.ptr = ctypes.addressof(self.view) if self.cap else None


def _pin_buffer(current: Optional[_Pin], obj) -> _Pin:
    """The pin for ``obj``, reusing ``current`` when it is the same buffer."""
    if current is not None and current.obj is obj:
        return current
    mv = memoryview(obj).cast("B")
    if mv.readonly:
        raise TypeError("a writable buffer, such as a bytearray, is needed")
    return _Pin(obj, mv)


def _path_arg(p) -> bytes:
    p = os.fspath(p)
    return p if isinstance(p, bytes) else p.encode()


class WriteCallbacks:
    """What a writer calls back into: the Rust ``WriteCallbacks`` trait.

    Subclass it and override the methods wanted; a method left as it is
    here is not installed at all, so it costs nothing. Raising from
    ``send`` or ``record_complete`` makes the write that triggered it
    raise :class:`IoError`, with the exception as its ``__cause__``.
    """

    def send(self, path: str) -> None:
        """A segment file at ``path`` is being handed over.

        When this returns there must be no file left at ``path`` and none
        matching the log's naming pattern.
        """

    def record_complete(self, fd: int) -> None:
        """A record ended in the segment file open on ``fd``.

        The library owns ``fd``: do not close it.
        """

    def timer_resolution_adjusted(self, resolution_ns: int) -> None:
        """The build-time timer resolution was too small for this machine
        and was widened to ``resolution_ns``, the figure to build with
        next time."""


def _overrides(handler, name: str) -> bool:
    method = getattr(type(handler), name, None)
    if method is None:
        return callable(getattr(handler, name, None))
    return method is not getattr(WriteCallbacks, name)


class LogWrite:
    """A writer: the Rust ``LogWrite`` and C ``TcslogWrite``.

    Opens a log in ``dir`` whose segment files are named ``prefix`` +
    identifier + ``suffix``, each at most ``seg_size_max`` bytes, holding
    records laid out as ``format``. ``handler``, a :class:`WriteCallbacks`,
    is what the writer calls back into, and may be omitted for none.

    Close it with :meth:`close`, or use it as a context manager. A close
    flushes the segment file being written and hands it to ``send``.
    """

    def __init__(
        self,
        dir,
        prefix: str,
        suffix: str,
        seg_size_max: int,
        format: Format,
        handler: Optional[WriteCallbacks] = None,
    ):
        self._h = _P()
        # Out-parameters, made once and reused by every call.
        self._written = ctypes.c_uint32()
        self._u64 = ctypes.c_uint64()
        # An exception a callback raised, re-raised once C returns.
        self._pending: Optional[BaseException] = None
        self._handler = handler
        self._pin: Optional[_Pin] = None

        cb = _CallbacksStruct()
        if handler is not None:
            if _overrides(handler, "send"):
                cb.send = _SendFn(self._on_send)
            if _overrides(handler, "record_complete"):
                cb.record_complete = _RecordCompleteFn(self._on_record_complete)
            if _overrides(handler, "timer_resolution_adjusted"):
                cb.timer_resolution_adjusted = _TimerResolutionAdjustedFn(
                    self._on_timer_resolution_adjusted
                )
        # The C side copies the structure, but the function pointers in
        # it are only valid while these objects live.
        self._cb = cb

        status = _write_open(
            _path_arg(dir),
            _path_arg(prefix),
            _path_arg(suffix),
            seg_size_max,
            int(format.tag),
            format.fixed_len,
            ctypes.byref(cb),
            ctypes.byref(self._h),
        )
        self._check(status)

    def _on_send(self, _ctx, path):
        try:
            self._handler.send(path.decode())
            return 0
        except BaseException as e:  # noqa: BLE001 -- must not unwind into C
            self._pending = e
            return 1

    def _on_record_complete(self, _ctx, fd):
        try:
            self._handler.record_complete(fd)
            return 0
        except BaseException as e:  # noqa: BLE001
            self._pending = e
            return 1

    def _on_timer_resolution_adjusted(self, _ctx, resolution_ns):
        try:
            self._handler.timer_resolution_adjusted(resolution_ns)
        except BaseException as e:  # noqa: BLE001
            self._pending = e

    def _check(self, status: int) -> None:
        pending, self._pending = self._pending, None
        if status == Status.OK:
            if pending is not None:
                # Only timer_resolution_adjusted can fail without the
                # call failing, having no way to report it.
                raise pending
            return
        if pending is not None and not isinstance(pending, Exception):
            raise pending  # KeyboardInterrupt and the like, unwrapped
        try:
            _raise(status)
        except LogError as e:
            raise e from pending

    def _handle(self) -> _P:
        if not self._h:
            raise ValueError("I/O operation on a closed LogWrite")
        return self._h

    @property
    def handler(self) -> Optional[WriteCallbacks]:
        """The callbacks this writer was given."""
        return self._handler

    def write(self, msg) -> int:
        """Writes ``msg``, any bytes-like object, as one record.

        Returns the bytes the record occupied in the log, its data header
        included. ``bytes`` and writable buffers are passed to the
        library in place, the latter pinned as the module documentation
        describes; any other read-only buffer is copied first.
        """
        h = self._handle()
        if isinstance(msg, bytes):
            data, n = msg, len(msg)
        elif self._pin is not None and self._pin.obj is msg:
            data, n = self._pin.ptr, self._pin.cap
        else:
            mv = memoryview(msg).cast("B")
            if mv.readonly:
                data, n = bytes(mv), mv.nbytes
            else:
                self._pin = _Pin(msg, mv)
                data, n = self._pin.ptr, self._pin.cap
        self._check(_write_record(h, data, n, ctypes.byref(self._written)))
        return self._written.value

    def write_str(self, msg: str) -> int:
        """Writes the UTF-8 bytes of ``msg`` as one record."""
        return self.write(msg.encode())

    def flush(self) -> None:
        """Flushes the segment file now being written."""
        self._check(_write_flush(self._handle()))

    def clear(self) -> None:
        """Removes every segment file of this log, leaving it empty."""
        self._check(_write_clear(self._handle()))

    def timer_resolution(self) -> int:
        """The timer resolution this writer is working with, in nanoseconds."""
        self._check(_write_timer_resolution(self._handle(), ctypes.byref(self._u64)))
        return self._u64.value

    def close(self) -> None:
        """Closes the writer. Closing twice does nothing.

        A close cannot report a failure; call :meth:`flush` first to
        know the last records reached storage.
        """
        self._pin = None
        if self._h:
            h, self._h = self._h, _P()
            _write_close(h)
            # The close's send has no way to report, as in C and Rust.
            self._pending = None

    def __enter__(self) -> "LogWrite":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def __del__(self):
        h = getattr(self, "_h", None)
        if h:
            self.close()


class LogRead:
    """A reader: the Rust ``LogRead`` and C ``TcslogRead``.

    Opens the log in ``dir`` whose segment files are named ``prefix`` +
    identifier + ``suffix``. Close it with :meth:`close`, or use it as a
    context manager.
    """

    def __init__(self, dir, prefix: str, suffix: str):
        self._h = _P()
        self._result = _ReadResultStruct()
        self._u64 = ctypes.c_uint64()
        self._pin: Optional[_Pin] = None
        _raise(
            _read_open(
                _path_arg(dir), _path_arg(prefix), _path_arg(suffix), ctypes.byref(self._h)
            )
        )

    def _handle(self) -> _P:
        if not self._h:
            raise ValueError("I/O operation on a closed LogRead")
        return self._h

    def read(self, buf) -> ReadResult:
        """Reads the next record into ``buf``, a writable bytes-like object.

        ``buf`` stays pinned, unable to be resized, until a different
        buffer is passed or the reader is closed; see the module
        documentation.

        Returns a :class:`ReadResult` whose ``n`` bytes at the front of
        ``buf`` are the record. Raises :class:`Eof` at the end of the
        log; :class:`SessionEnd`, :class:`ReadTruncated` and
        :class:`ReadOverflow` are followed by reading again, and the
        last two carry what reached ``buf``.
        """
        h = self._handle()
        pin = self._pin = _pin_buffer(self._pin, buf)
        r = self._result
        status = _read_record(h, pin.ptr, pin.cap, ctypes.byref(r))
        if status == Status.OK:
            return ReadResult(r.n, Meta(r.meta), r.timestamp, r.record_count)
        if status == Status.READ_OVERFLOW:
            raise ReadOverflow(ReadResult(r.n, Meta(r.meta), r.timestamp, r.record_count))
        if status == Status.READ_TRUNCATED:
            raise ReadTruncated(ReadResult(r.n, Meta(r.meta), r.timestamp, r.record_count), r.lost)
        _raise(status)
        raise AssertionError("unreachable")

    def read_str(self, buf) -> ReadResult:
        """As :meth:`read`, for callers whose payloads are UTF-8 text.

        Takes the same byte buffer, since a payload recovered in part
        need not be valid UTF-8.
        """
        return self.read(buf)

    def iter(self):
        """An iterator over the remaining records, yielding :class:`Record`.

        Skips session boundaries and records of which no byte survived,
        and marks a partial record ``truncated``. Ends at the end of the
        log and also at an :class:`IoError`, which it cannot report; a
        caller that must tell the two apart uses :meth:`read`.
        """
        buf = bytearray(_ITER_BUF_LEN)
        while True:
            try:
                r = self.read(buf)
                truncated = False
            except (ReadOverflow, ReadTruncated) as e:
                if e.n == 0:
                    continue
                r, truncated = e.result, True
            except SessionEnd:
                continue
            except LogError:
                return
            payload = bytes(memoryview(buf)[: r.n])
            yield Record(r.meta, payload, truncated, r.timestamp, r.record_count)

    def __iter__(self):
        return self.iter()

    def segments_opened(self) -> int:
        """The number of segment files this reader has opened."""
        _raise(_read_segments_opened(self._handle(), ctypes.byref(self._u64)))
        return self._u64.value

    def close(self) -> None:
        """Closes the reader. Closing twice does nothing."""
        self._pin = None
        if self._h:
            h, self._h = self._h, _P()
            _read_close(h)

    def __enter__(self) -> "LogRead":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def __del__(self):
        h = getattr(self, "_h", None)
        if h:
            self.close()
