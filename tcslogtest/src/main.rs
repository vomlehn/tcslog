use tcslog::{BLOCK_SIZE, MAX_RECORD_SIZE, TcsLog, TcsLogError, Timestamp, Timestampable};

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
    println!("MAX_RECORD_SIZE {}", MAX_RECORD_SIZE);
    testit()
}

fn testit<'a>() {
    let result = test_minimal();
    println!("result: {:?}", result);

/*
    let result = test_fill_minimal();
    println!("result: {:?}", result);
*/
}

fn test_minimal<'a>() -> Result<TcsLog<'a>, TcsLogError> {
    let over_four = MAX_RECORD_SIZE / 4;
    write_recs(over_four, 1)
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
        println!("write record {}", i);
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
