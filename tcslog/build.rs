use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Default timer resolution when the caller does not override it.
    // Chosen to match a typical modern OS clock tick.
    let default_ns: u64 = 1_000_000;

    let ns: u64 = match env::var("TCSLOG_TIMER_RESOLUTION_NS") {
        Ok(s) => {
            // Set-but-invalid must be an error: a compile-time panic is
            // the required behavior when the user has explicitly asked
            // for a specific timer resolution and it is unusable.
            let parsed: u64 = s.parse().unwrap_or_else(|_| {
                panic!(
                    "TCSLOG_TIMER_RESOLUTION_NS={s:?} is not a non-negative integer"
                )
            });
            if parsed == 0 {
                panic!("TCSLOG_TIMER_RESOLUTION_NS must be greater than zero");
            }
            parsed
        }
        Err(_) => default_ns,
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let path = out_dir.join("timer_resolution.rs");
    let contents = format!(
        "/// System-dependent timer resolution, in nanoseconds. Overridable at\n\
         /// build time by setting the TCSLOG_TIMER_RESOLUTION_NS environment\n\
         /// variable to a strictly positive integer.\n\
         pub const TIMER_RESOLUTION_NS: u64 = {ns};\n"
    );
    fs::write(&path, contents).expect("failed to write timer_resolution.rs");

    println!("cargo:rerun-if-env-changed=TCSLOG_TIMER_RESOLUTION_NS");
    println!("cargo:rerun-if-changed=build.rs");
}
