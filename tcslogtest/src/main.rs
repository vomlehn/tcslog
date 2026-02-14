use tcslog::{TcsLog, TcsLogError};

fn main() {
    let result = testit();
    println!("result: {:?}", result);
}

fn testit<'a>() -> Result<TcsLog<'a>, TcsLogError> {
    let mut tcs_log = TcsLog::new("/tmp", "testlog", 2 * 1024 * 1024)?;

    tcs_log.write(&"this is record 1".as_bytes())?;

    Ok(tcs_log)
}
