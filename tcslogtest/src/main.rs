use std::path::PathBuf;
use std::fs::OpenOptions;
use std::io::Read;

use tcslog::{BLOCK_SIZE, FILE_TYPE, Header, HEADER_SIZE, MAX_RECORD_SIZE, TcsLog, TcsLogError, Timestamp, Timestampable, VERSION};

/*
 * Define a type that returns the timestamp. In this implementation,
 * we return the time since the UNIX epoch.
 */
#[derive(Debug)]
pub struct Teststamper {
    time:       Timestamp,
    first_time: Option<Timestamp>
}

impl Teststamper {
    fn new() -> Teststamper {
        Teststamper {
            time:       0,
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
}

impl Timestampable for Teststamper {
    /// Returns the current timestamp in nanoseconds since UNIX epoch.
    fn timestamp(&mut self) -> Timestamp {
        self.time += 1;
        if self.first_time.is_none() {
            self.first_time = Some(self.time);
        }
        self.time
    }
}

fn main() {
    testit()
}


fn testit<'a>() {
    let over_four = MAX_RECORD_SIZE / 4;
    let result = test_write_minimal(over_four);
    match &result {
        Err(e) => println!("test_write_minimal: FAILED: {:?}", result),
        Ok(timestamp) => {
            let result = test_read_minimal(*timestamp, over_four);
            match &result {
                Err(e) => println!("test_write_minimal: FAILED: {:?}", result),
                Ok(()) => println!("test_read_minimal: success"),
            }
        }
    }

    let result = test_fill_minimal();
    println!("Test {}", if result.is_ok() { "successful" } else { "FAILED" });
}

fn test_write_minimal(over_four: usize) -> Result<Timestamp, TcsLogError> {

    let mut teststamper = Teststamper::new();
    let mut tcs_log = TcsLog::new_with_timestamp("/tmp", "testlog", &mut teststamper, (3 * BLOCK_SIZE).try_into().unwrap())?;
    write_recs(&mut tcs_log, &mut teststamper, over_four, 1)?;

    let mut teststamper = Teststamper::new();
    let timestamp = teststamper.timestamp();

    let prefix = "testlog";
    let file_name = TcsLog::generate_file_name(prefix, timestamp);
println!("file_name {}", file_name);

    // Create header
    let index_offset: u64 = BLOCK_SIZE.try_into().unwrap();
    let data_offset: u64 = (2 * BLOCK_SIZE).try_into().unwrap();
//    let header = Header::new(timestamp, &file_name, index_offset, data_offset);

    // Open the file
    
    let dir_name = "/tmp";
    let path = PathBuf::from(dir_name).join(&file_name);
println!("path {:?}", path);

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
    offset += VERSION.len();
println!("Version okay");

    Ok(teststamper.timestamp())
}

fn test_fill_minimal<'a>() -> Result<Timestamp, TcsLogError> {
    let over_four = MAX_RECORD_SIZE / 4;
    let mut teststamper = Teststamper::new();

    let mut tcs_log = TcsLog::new_with_timestamp("/tmp", "testlog", &mut teststamper, (3 * BLOCK_SIZE).try_into().unwrap())?;
    write_recs(&mut tcs_log, &mut teststamper, over_four + 4, 5)
}

fn test_read_minimal(timestamp: Timestamp, over_four: usize) -> Result<(), TcsLogError> {
    let mut testtamper = Teststamper::new_init(timestamp);
    let timestamp = testtamper.timestamp();
    let mut tcs_log = TcsLog::open("/tmp", "testlog", timestamp)?;

    let mut vec: Vec<u8> = Vec::with_capacity(over_four);
    let mut buf = &vec;


    Ok(())
}

/**
 * Create a log file and write records
 * rec_size:    Number of bytes
 * n_recs:      Number of records to write
 */
fn write_recs<'a>(tcs_log: &mut TcsLog, teststamper: &mut Teststamper, rec_size: usize, n_recs: usize) -> Result<Timestamp, TcsLogError> {

    for i in 0..n_recs {
        let rec = create_record(rec_size, i);
        tcs_log.write(teststamper, &rec)?;
    }

    Ok(teststamper.timestamp())
}

fn create_record(rec_size: usize, i: usize) -> Vec<u8> {
    let start = format!("<<<Record {} ", i);
    let end = format!(" #{} >>>", i);
    let fill = format!("{}", i);
    let middle = fill.repeat(rec_size - (start.len() + end.len()));
    (start + &middle + &end).into()
}
