//! Holds the library to its promise that writing a record and reading a
//! record allocate nothing, rolling from one segment file to the next
//! included.
//!
//! A counting global allocator stands in for the system's. It counts
//! only on a thread that has asked it to, so the test harness's own
//! threads and the opening of a log, which is documented to allocate,
//! are not counted. Kept in a test binary of its own, since a global
//! allocator is per binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use tcslog::{Format, LogError, LogRead, LogWrite, SEGMENT_FILE_HEADER_LEN};

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn note() {
    // A thread being torn down has no locals left to count with, and
    // nothing of the test's is running on it then.
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = COUNT.try_with(|n| n.set(n.get() + 1));
        }
    });
}

// SAFETY: every call is passed straight to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's guarantees are the system allocator's.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Runs `f` and returns how many allocations it made on this thread.
fn allocations(f: impl FnOnce()) -> usize {
    COUNT.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    f();
    COUNTING.with(|on| on.set(false));
    COUNT.with(Cell::get)
}

const PREFIX: &str = "seg-";
const SUFFIX: &str = ".log";

/// Writes `records` records of 30 bytes into segment files of 64 data
/// bytes, so that the writer rolls every couple of records, and reads
/// them back, counting the allocations each side makes once its log is
/// open.
fn write_then_read(format: Format, records: usize) -> (usize, usize) {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().to_str().expect("a UTF-8 temporary path");
    let seg_size_max = SEGMENT_FILE_HEADER_LEN + 64;
    let payload = [0x5au8; 30];
    let payload = match format {
        Format::Fixed(n) => &payload[..n as usize],
        _ => &payload[..],
    };

    let mut w = LogWrite::new(path, PREFIX, SUFFIX, seg_size_max, format, ())
        .expect("a writer on a fresh directory");
    let writing = allocations(|| {
        for _ in 0..records {
            w.write(payload).expect("a record the format accepts");
        }
        w.flush().expect("a flush");
    });
    drop(w);

    let mut r = LogRead::new(path, PREFIX, SUFFIX).expect("a readable log");
    let mut buf = [0u8; 64];
    let mut read = 0;
    let reading = allocations(|| loop {
        match r.read(&mut buf) {
            Ok(_) => read += 1,
            Err(LogError::Eof) => break,
            Err(e) => panic!("an intact log read with {e:?}"),
        }
    });
    assert_eq!(read, records, "every record written is read back");
    (writing, reading)
}

#[test]
fn writing_and_reading_allocate_nothing_in_any_format() {
    for format in [
        Format::Fixed(30),
        Format::VariableSimple,
        Format::VariableTsRc,
    ] {
        let (writing, reading) = write_then_read(format, 40);
        assert_eq!(
            writing, 0,
            "{format:?}: writing 40 records across rolls allocated"
        );
        assert_eq!(
            reading, 0,
            "{format:?}: reading 40 records across files allocated {reading} times"
        );
    }
}

#[test]
fn the_counter_counts() {
    // Without this, a counter that never counted would pass the test
    // above.
    let n = allocations(|| drop(std::hint::black_box(vec![1u8; 16])));
    assert!(n > 0, "a Vec allocation was not counted");
}

#[test]
fn reading_past_a_lost_segment_file_allocates_nothing() {
    // Recovery is the reader's purpose, so the promise has to hold
    // while it is recovering: resynchronizing, reporting the loss, and
    // carrying on.
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().to_str().expect("a UTF-8 temporary path");
    let seg_size_max = SEGMENT_FILE_HEADER_LEN + 64;
    {
        let mut w = LogWrite::new(
            path,
            PREFIX,
            SUFFIX,
            seg_size_max,
            Format::VariableSimple,
            (),
        )
        .expect("a writer on a fresh directory");
        for _ in 0..40 {
            w.write(&[0x5au8; 30]).expect("a record");
        }
    }
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("a readable directory")
        .map(|e| e.expect("an entry").path())
        .collect();
    names.sort();
    std::fs::remove_file(&names[names.len() / 2]).expect("a removable file");

    let mut r = LogRead::new(path, PREFIX, SUFFIX).expect("a readable log");
    let mut buf = [0u8; 64];
    let (mut read, mut losses) = (0, 0);
    let n = allocations(|| loop {
        match r.read(&mut buf) {
            Ok(_) => read += 1,
            Err(LogError::ReadTruncated { .. }) => losses += 1,
            Err(LogError::Eof) => break,
            Err(e) => panic!("a damaged log read with {e:?}"),
        }
    });
    assert!(
        losses > 0 && read > 0 && read < 40,
        "the loss was met and read past"
    );
    assert_eq!(n, 0, "reading past the loss allocated {n} times");
}
