use std::path::PathBuf;
use std::fs::OpenOptions;

use tcslog::{BLOCK_SIZE, Header, MAX_RECORD_SIZE, TcsLog, TcsLogError, Timestamp, Timestampable};

/*
 * Define a type that returns the timestamp. In this implementation,
 * we return the time since the UNIX epoch.
 */
#[derive(Debug)]
pub struct Teststamper {
    time:   u128
}

impl Teststamper {
    fn new() -> Teststamper {
        Teststamper {
            time: 0
        }
    }
}

impl Timestampable for Teststamper {
    /// Returns the current timestamp in nanoseconds since UNIX epoch.
    fn timestamp(&mut self) -> Timestamp {
        self.time += 1;
        self.time
    }
}

fn main() {
    testit()
}

fn testit<'a>() {
    let result = test_minimal();
    println!("Test {}", if result.is_ok() { "successful" } else { "FAILED" });

    let result = test_fill_minimal();
    println!("Test {}", if result.is_ok() { "successful" } else { "FAILED" });
}

fn test_minimal<'a>() -> Result<TcsLog<'a>, TcsLogError> {
    let over_four = MAX_RECORD_SIZE / 4;
    let tcs_log = write_recs(over_four, 1);

    let mut teststamper = Teststamper::new();
    let timestamp = teststamper.timestamp();

    let prefix = "testlog";
    let file_name = TcsLog::generate_file_name(prefix, timestamp);
println!("file_name {}", file_name);

    // Create header
    let index_offset = BLOCK_SIZE.try_into().unwrap();
    let data_offset = (2 * BLOCK_SIZE).try_into().unwrap();
    let header = Header::new(timestamp, &file_name, index_offset, data_offset);

    // Create the file
    
    let dir_name = "/tmp";
    let path = PathBuf::from(dir_name).join(&file_name);
println!("path {:?}", path);


    match OpenOptions::new()
        .read(true)
        .open(&path)
        {
        Err(e) => panic!("open {:?} failed: {:?}", path, e),
        Ok(f) => {},
    }
    tcs_log
}

fn test_fill_minimal<'a>() -> Result<TcsLog<'a>, TcsLogError> {
    let over_four = MAX_RECORD_SIZE / 4;
    write_recs(over_four + 4, 5)
}

/**
 * Create a log file and write records
 * rec_size:    Number of bytes
 * n_recs:      Number of records to write
 */
fn write_recs<'a>(rec_size: usize, n_recs: usize) -> Result<TcsLog<'a>, TcsLogError> {

    let mut timestamper = Teststamper::new();
    let mut tcs_log = TcsLog::new_with_timestamp("/tmp", "testlog", &mut timestamper, (3 * BLOCK_SIZE).try_into().unwrap())?;

    for i in 0..n_recs {
        let rec = create_record(rec_size, i);
        tcs_log.write(&mut timestamper, &rec)?;
    }

    Ok(tcs_log)
}

fn create_record(rec_size: usize, i: usize) -> Vec<u8> {
    let start = format!("<<<Record {} ", i);
    let end = format!(" #{} >>>", i);
    let fill = format!("{}", i);
    let middle = fill.repeat(rec_size - (start.len() + end.len()));
    (start + &middle + &end).into()
}
