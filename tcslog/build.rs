use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let default_ns: u64 = 1_000_000;
    let ns: u64 = env::var("TCSLOG_TIMER_RESOLUTION_NS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default_ns);

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let path = out_dir.join("timer_resolution.rs");
    let contents = format!(
        "/// System-dependent timer resolution, in nanoseconds. Overridable at\n\
         /// build time by setting the TCSLOG_TIMER_RESOLUTION_NS environment\n\
         /// variable to a non-zero, non-negative integer.\n\
         pub const TIMER_RESOLUTION_NS: u64 = {ns};\n"
    );
    fs::write(&path, contents).expect("failed to write timer_resolution.rs");

    println!("cargo:rerun-if-env-changed=TCSLOG_TIMER_RESOLUTION_NS");
    println!("cargo:rerun-if-changed=build.rs");
}
