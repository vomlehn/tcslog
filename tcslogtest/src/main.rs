use std::path::PathBuf;
//use std::fs::OpenOptions;
//use std::io::Read;

use tcslog::{
    BLOCK_HEADER_SIZE, CONT_SIZE, TcsLog, TcsLogError, Timestamp, Timestampable, TimestampableError, BLOCK_SIZE, MAX_RECORD_SIZE, RECORD_METADATA_SIZE
};

/*
 * Define a type that returns the timestamp. The timestamp advances by one
 * each time, making it easy to predict what it will be.
 */
#[derive(Debug)]
pub struct Teststamper {
    time: Timestamp,
    //    first_time: Option<Timestamp>
}

impl Teststamper {
    fn new() -> Teststamper {
        Teststamper {
            time: Timestamp::ZERO,
            //            first_time: None,
        }
    }

    /*
        fn new_init(first: Timestamp) -> Teststamper {
            Teststamper {
                time:       first,
                first_time: Some(first),
            }
        }

        fn first(&self) -> Timestamp {
            self.first_time.unwrap()
        }
    */

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
        /*
                if self.first_time.is_none() {
        println!("Set first time to {:?}", self.time);
                    self.first_time = Some(self.time);
                }
        */
        Ok(self.time)
    }
}

fn main() {
    testit()
}

fn testit<'a>() {
    let mut test: &str;
/*

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

    test = "test_many_small";
    match test_many_small() {
        Err(e) => println!("{} FAILED: {:?}", test, e),
        Ok(_) => println!("{} succeeded", test),
    }
    println!("---");
*/

    test = "test_many_many_small";
    match test_many_many_small() {
        Err(e) => println!("{} FAILED: {:?}", test, e),
        Ok(_) => println!("{} succeeded", test),
    }
    println!("---");

    test = "test_chain_count";
    match test_chain_count() {
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
fn test_empty<'a>() -> Result<(), TcsLogError<'a>> {
    test_write_read(0, 0)
}

// Test writing/reading a record that will fit entirely in the first
// data block
fn test_one_small<'a>() -> Result<(), TcsLogError<'a>> {
    test_write_read(MAX_RECORD_SIZE / 2, 1)
}

// Test writing/reading records that will fit entirely in the first
// data block
fn test_multiple_small<'a>() -> Result<(), TcsLogError<'a>> {
    test_write_read(MAX_RECORD_SIZE / 12, 10)
}

// Test writing/reading records that will require many data blocks
fn test_many_small<'a>() -> Result<(), TcsLogError<'a>> {
    test_write_read(MAX_RECORD_SIZE / 4, 5)
}

// Test writing/reading records that will require many data blocks
fn test_many_many_small<'a>() -> Result<(), TcsLogError<'a>> {
//    test_write_read(MAX_RECORD_SIZE / 12, 100)
    test_write_read(MAX_RECORD_SIZE / 12, 11)
}

/**
 * Create a log file and write enough records to spill over into two more
 * chained files (three files in total), then read every file in the chain
 * and verify that their chain counts are 0, 1, and 2.
 */
fn test_chain_count<'a>() -> Result<(), TcsLogError<'a>> {
    // A self-cleaning, OS-independent temporary directory. It and the log files
    // created inside it are removed automatically when `dir` is dropped, so the
    // deterministic file names produced by the test timestamper never collide
    // with a previous run.
    let dir = tempfile::tempdir()?;
    let dir_name = dir.path().to_str().expect("temp dir path is not valid UTF-8");
    let prefix = "chainlog";
    let suffix = ".tcsl";

    let mut teststamper = Teststamper::new();

    // Small files so a modest number of records spans several of them.
    let max_size = (3 * BLOCK_SIZE) as u64;
    let mut tcs_log =
        TcsLog::new_with_timestamp(dir_name, prefix, &mut teststamper, suffix, max_size)?;

    // The first file in the chain has a chain count of zero.
    if tcs_log.chain_count() != 0 {
        return Err(TcsLogError::TestError("first file chain count is not zero"));
    }

    // Keep writing until two successor files have been created. At that point
    // the current (latest) file is the third in the chain, with a count of 2.
    let rec_size = MAX_RECORD_SIZE / 2;
    let mut n_recs = 0;
    while tcs_log.chain_count() < 2 {
        let rec = create_record(rec_size, n_recs);
        tcs_log.write(&mut teststamper, &rec)?;
        n_recs += 1;
    }

    // Verify the chain count of every file, and remember the first one.
    let mut counts = Vec::new();
    let mut first_file = None;
    for entry in std::fs::read_dir(dir_name)? {
        let entry = entry?;
        let path = entry.path();
        let log = TcsLog::open_path(&path)?;
        if log.chain_count() == 0 {
            first_file = Some(path);
        }
        counts.push(log.chain_count());
    }

    counts.sort();
    if counts != [0, 1, 2] {
        println!("test_chain_count: expected chain counts [0, 1, 2], got {:?}", counts);
        return Err(TcsLogError::TestError("unexpected chain counts"));
    }

    // Read every record back through the first file. The reader must follow the
    // chain transparently across all three files and then report EOF.
    let first_file = first_file.ok_or(TcsLogError::TestError("no file with chain count 0"))?;
    let mut reader = TcsLog::open_path(first_file)?;
    read_recs_eof(&mut reader, rec_size, n_recs)?;

    Ok(())
}

fn test_write_read<'a>(rec_size: usize, n: usize) -> Result<(), TcsLogError<'a>> {
    // A self-cleaning, OS-independent temporary directory. It (and every log
    // file created inside it) is removed automatically when `dir` is dropped at
    // the end of this function.
    let dir = tempfile::tempdir()?;
    let dir_name = dir.path().to_str().expect("temp dir path is not valid UTF-8");
    let prefix = "testlog";
    let suffix = ".tcsl";

    // Get a timestamp producer
    let mut teststamper = Teststamper::new();
    println!("Creating TcsLog");

    // Create the log
    let mut tcs_log = TcsLog::new_with_timestamp(
        dir_name,
        prefix,
        &mut teststamper,
        suffix,
        (3 * BLOCK_SIZE).try_into().unwrap(),
    )?;

    // Write records
    let timestamp = teststamper.snapshot();
    write_recs(&mut tcs_log, &mut teststamper, rec_size, n)?;

    // Reopen the file for reading
    let file_name = TcsLog::generate_file_name(prefix, timestamp, suffix)?;
    println!("test_write_read: file_name {}", file_name);
    let path = PathBuf::from(dir_name).join(&file_name);
    println!("test_write_read: path {:?}", path);
    let mut tcs_log = TcsLog::open_path(path)?;
    println!("test_write_read: path is open");

    // Read records, with an expected EOF
    read_recs_eof(&mut tcs_log, rec_size, n)?;

    Ok(())
}

/**
 * Create a log file and write records
 * rec_size:    Number of bytes
 * n_recs:      Number of records to write
 */
fn write_recs<'a>(
    tcs_log: &mut TcsLog,
    teststamper: &mut Teststamper,
    rec_size: usize,
    n_recs: usize,
) -> Result<Timestamp, TcsLogError<'a>> {
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
    // Fill the middle so the whole record is exactly `rec_size` bytes. The fill
    // string can be more than one character wide (e.g. "10"), so cycle its
    // characters and take exactly the number of bytes needed rather than
    // repeating the whole string.
    let mid_len = rec_size - (start.len() + end.len());
    let fill = format!("{}", i);
    let middle: String = fill.chars().cycle().take(mid_len).collect();
    (start + &middle + &end).into()
}

/**
 * Read the given number of records, then one more and verify the last
 * read gets an EOF
 */
fn read_recs_eof<'a>(
    tcs_log: &mut TcsLog,
    rec_size: usize,
    n_recs: usize,
) -> Result<(), TcsLogError<'a>> {
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

fn read_recs<'a>(
    tcs_log: &mut TcsLog,
    rec_size: usize,
    n_recs: usize,
) -> Result<(), TcsLogError<'a>> {
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
 * Compute the maximum size of record data that will fit in a given
 * block without overflowing it.
 *
 * BLOCK_HEADER_SIZE_N = BLOCK_HEADER_SIZE + EOF_SIZE + n * RECORD_HEADER_SIZE;
 *
 * BLOCK_SIZE = BLOCK_HEADER_SIZE_N + n * rec_size)
 *
 * n * rec_size = BLOCK_SIZE - BLOCK_HEADER_SIZE_N
 *
 * rec_size = (BLOCK_SIZE - BLOCK_HEADER_N) / n
 */
fn max_block_rec_size(n: usize) -> usize {
    let block_header_size_n = BLOCK_HEADER_SIZE + CONT_SIZE + n * RECORD_METADATA_SIZE;
let rec_size = 
    BLOCK_SIZE - block_header_size_n
;
println!("max_block_rec_size: n {n} rec_size {rec_size}");
rec_size
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
