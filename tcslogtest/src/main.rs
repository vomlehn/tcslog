use std::path::PathBuf;
use std::fs::OpenOptions;
use std::io::Read;

use tcslog::{BLOCK_SIZE, MAX_RECORD_SIZE, TcsLog, TcsLogError, Timestamp, Timestampable, TimestampableError};

/*
 * Define a type that returns the timestamp. The timestamp advances by one
 * each time, making it easy to predict what it will be.
 */
#[derive(Debug)]
pub struct Teststamper {
    time:       Timestamp,
    first_time: Option<Timestamp>
}

impl Teststamper {
    fn new() -> Teststamper {
        Teststamper {
            time:       Timestamp::ZERO,
            first_time: None,
        }
    }

    fn new_init(first: Timestamp) -> Teststamper {
        Teststamper {
            time:       first,
            first_time: Some(first),
        }
    }

    fn first(&self) -> Timestamp {
        self.first_time.unwrap()
    }

    fn snapshot(&self) -> Timestamp {
        self.time
    }
}

impl Timestampable for Teststamper {
    /// Returns the current timestamp in nanoseconds since UNIX epoch.
    fn timestamp(&mut self) -> Result<Timestamp, TimestampableError> {
        let mut time_ns = self.time.as_nanos();
        time_ns += 1;
        self.time = Timestamp::from_nanos(time_ns);
        if self.first_time.is_none() {
println!("Set first time to {:?}", self.time);
            self.first_time = Some(self.time);
        }
        Ok(self.time)
    }
}

fn main() {
    testit()
}


fn testit<'a>() {
    let mut test: &str;

    test = "test_empty";
    match test_empty() {
        Err(e) => println!("{} FAILED: {:?}", test, e),
        Ok(_) => println!("{} succeeded", test),
    }
println!("---");

    test = "test_one_small";
    match test_one_small() {
        Err(e) => println!("{} FAILED: {:?}", test, e),
        Ok(_) => println!("{} succeeded", test),
    }
println!("---");

    test = "test_multiple_small";
    match test_multiple_small() {
        Err(e) => println!("{} FAILED: {:?}", test, e),
        Ok(_) => println!("{} succeeded", test),
    }
println!("---");

/*
    let over_four = MAX_RECORD_SIZE / 4;
    let result = test_write_one(over_four);
    match &result {
        Err(e) => println!("test_write_one: FAILED: {:?}", e),
        Ok(timestamp) => {
            let result = test_read_one(*timestamp, over_four);
            match &result {
                Err(e) => println!("test_read_one: FAILED: {:?}", e),
                Ok(n) => println!("test_read_one: success"),
            }
        }
    }
println!("---");

    let result = test_fill_one();
    println!("Test {}", if result.is_ok() { "successful" } else { "FAILED" });
*/
}

// Test reading from a log file with no information
fn test_empty<'a>() -> Result<TcsLog<'a>, TcsLogError<'a>> {
    test_write_read(0, 0)
}

// Test writing/reading a record that will fit entirely in the first
// data block
fn test_one_small<'a>() -> Result<TcsLog<'a>, TcsLogError<'a>> {
    test_write_read(MAX_RECORD_SIZE / 2, 1)
}

// Test writing/reading records that will fit entirely in the first
// data block
fn test_multiple_small<'a>() -> Result<TcsLog<'a>, TcsLogError<'a>> {
    test_write_read(MAX_RECORD_SIZE / 12, 10)
}

fn test_write_read<'a>(rec_size: usize, n: usize) -> Result<TcsLog<'a>, TcsLogError<'a>> {
    let dir_name = "/tmp";
    let prefix = "testlog";

    // Get a timestamp producer
    let mut teststamper = Teststamper::new();
println!("Creating TcsLog");

    // Create the log
    let mut tcs_log = TcsLog::new_with_timestamp(dir_name, prefix, &mut teststamper, (3 * BLOCK_SIZE).try_into().unwrap())?;

    // Write records
    let timestamp = teststamper.snapshot();
    write_recs(&mut tcs_log, &mut teststamper, rec_size, n)?;

    // Reopen the file for reading
    let file_name = TcsLog::generate_file_name(prefix, timestamp)?;
println!("test_write_read: file_name {}", file_name);
    let path = PathBuf::from(dir_name).join(&file_name);
println!("test_write_read: path {:?}", path);
    let mut tcs_log = TcsLog::open_path(path)?;
println!("test_write_read: path is open");

    // Read records, with an expected EOF
    read_recs_eof(&mut tcs_log, rec_size, n)?;

    Ok(tcs_log)
}

/**
 * Create a log file and write records
 * rec_size:    Number of bytes
 * n_recs:      Number of records to write
 */
fn write_recs<'a>(tcs_log: &mut TcsLog, teststamper: &mut Teststamper, rec_size: usize, n_recs: usize) -> Result<Timestamp, TcsLogError<'a>> {

    for i in 0..n_recs {
        let rec = create_record(rec_size, i);
        tcs_log.write(teststamper, &rec)?;
    }

    match teststamper.timestamp() {
        Err(e) => Err(TcsLogError::TimestampableError(e)),
        Ok(timestamp) => Ok(timestamp),
    }
}

/**
 * Create a single record with a serial number
 */
fn create_record(rec_size: usize, i: usize) -> Vec<u8> {
    let start = format!("<<<Record {} ", i);
    let end = format!(" #{} >>>", i);
    let fill = format!("{}", i);
    let middle = fill.repeat(rec_size - (start.len() + end.len()));
    (start + &middle + &end).into()
}

/**
 * Read the given number of records, then one more and verify the last
 * read gets an EOF
 */
fn read_recs_eof<'a>(tcs_log: &mut TcsLog, rec_size: usize, n_recs: usize) -> Result<(), TcsLogError<'a>> {
    read_recs(tcs_log, rec_size, n_recs)?;

    let mut timestamp = Timestamp::new(0, 0);
    let mut buf = vec![0u8; rec_size];

println!("read_recs_eof: rec_size {} size {}", rec_size, buf.len());
    // Read one more record to verify EOF
    let eof = tcs_log.read(&mut timestamp, &mut buf);
    match eof {
        Err(TcsLogError::EOF) => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Ok(()),
    }
}

fn read_recs<'a>(tcs_log: &mut TcsLog, rec_size: usize, n_recs: usize) -> Result<(), TcsLogError<'a>> {
    let mut timestamp = Timestamp::new(0, 0);
    let mut buf = vec![0u8; rec_size];

    for i in 0..n_recs {
println!("read_rec: i {i} rec_size {} size {}", rec_size, buf.len());
        let n = tcs_log.read(&mut timestamp, &mut buf)?;

        let rec = create_record(rec_size, i);
        if buf[..n] != rec {
            println!("Failed reading record {i}");
            return Err(TcsLogError::TestError("record mismatch"));
        }
    }

    Ok(())
}

/*
/**
 * Write one record, returning the timestamp used to create the log file or
 * an error.
 */
fn test_write_one(over_four: usize) -> Result<Timestamp, TcsLogError> {

    let mut teststamper = Teststamper::new();
println!("Creating TcsLog");
    let mut tcs_log = TcsLog::new_with_timestamp("/tmp", "testlog", &mut teststamper, (3 * BLOCK_SIZE).try_into().unwrap())?;
    let timestamp = teststamper.snapshot();
    write_recs(&mut tcs_log, &mut teststamper, over_four, 1)?;
    Ok(timestamp)
}

/**
 * Read one record, verifying that the EOF is present
 */
fn test_read_one(timestamp: Timestamp, over_four: usize) -> Result<(), TcsLogError> {
    let prefix = "testlog";
    let file_name = TcsLog::generate_file_name(prefix, timestamp)?;
println!("test_read_one: file_name {}", file_name);

    // Open the file
    
    let dir_name = "/tmp";
    let path = PathBuf::from(dir_name).join(&file_name);
println!("test_read_one: path {:?}", path);
    let mut tcs_log = TcsLog::open_path(path)?;
println!("test_read_one: path is open");

    read_recs_eof(&mut tcs_log, over_four, 1)
}

/**
 * Test overflowing from one log file to another one
 */
fn test_fill_one<'a>() -> Result<Timestamp, TcsLogError> {
    let over_four = MAX_RECORD_SIZE / 4;
    let mut teststamper = Teststamper::new();

    let mut tcs_log = TcsLog::new_with_timestamp("/tmp", "testlog", &mut teststamper, (3 * BLOCK_SIZE).try_into().unwrap())?;
    write_recs(&mut tcs_log, &mut teststamper, over_four + 4, 5)
}
/* FIXME: delete this
    let mut f = match OpenOptions::new()
        .read(true)
        .open(&path)
        {
        Err(e) => panic!("open {:?} failed: {:?}", path, e),
        Ok(f) => f,
    };

    // Read the header
    let mut header_bytes = [0u8; HEADER_SIZE];
    f.read_exact(&mut header_bytes)?;

    // Check fields
    let mut offset = 0;
    assert_eq!(&header_bytes[offset..offset + FILE_TYPE.len()], FILE_TYPE);
    offset += FILE_TYPE.len();
println!("File type okay");

    assert_eq!(&header_bytes[offset..offset + VERSION.len()], VERSION);
//    offset += VERSION.len();
println!("Version okay");

let t = Ok(teststamper.first());
    match teststamper.first() {
        Err(e) => Err(TcsLogError::TimestampableError(e)),
        Ok(timestamp) => Ok(timestamp),
    }
;
println!("test_write_one: returning {:?}", t);
t
*/
/*
    // Create header
    let index_offset: u64 = BLOCK_SIZE.try_into().unwrap();
    let data_offset: u64 = (2 * BLOCK_SIZE).try_into().unwrap();
    let header = Header::new(timestamp, &file_name, index_offset, data_offset);
*/
/*
fn test_read_one(timestamp: Timestamp, over_four: usize) -> Result<(), TcsLogError> {
    let mut testtamper = Teststamper::new_init(timestamp);
    let timestamp = testtamper.first();
    let _tcs_log = TcsLog::open("/tmp", "testlog", timestamp)?;

    let vec: Vec<u8> = Vec::with_capacity(over_four);
    let _buf = &vec;


    Ok(())
}
*/
*/
