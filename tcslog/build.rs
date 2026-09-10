use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=TIMER_RESOLUTION");
    println!("cargo:rerun-if-changed=build.rs");

    // TIMER_RESOLUTION is only needed by the write path. When the
    // `write` feature is off, reader-only consumers can build without
    // it. Cargo sets CARGO_FEATURE_WRITE when the feature is enabled.
    if env::var_os("CARGO_FEATURE_WRITE").is_none() {
        return;
    }

    // The tcslog spec requires TIMER_RESOLUTION to be supplied on the
    // command line (typically through config.mk) and forbids a default
    // value. Unparseable or zero values must be rejected either at
    // compile time or at LogWrite::new() time. We reject unset and
    // unparseable values here; a zero value is also refused so that
    // create_segment_file() cannot spin.
    let raw = match env::var("TIMER_RESOLUTION") {
        Ok(s) => s,
        Err(_) => panic!(
            "TIMER_RESOLUTION must be set (nanoseconds, positive integer). \
             Provide it via the `TIMER_RESOLUTION` environment variable \
             (`TIMER_RESOLUTION=1 cargo build`) or by copying \
             `.cargo/config.toml.example` to `.cargo/config.toml` and \
             editing the `[env]` value there."
        ),
    };

    let ns: u64 = raw.parse().unwrap_or_else(|_| {
        panic!(
            "TIMER_RESOLUTION={raw:?} is not a non-negative integer"
        )
    });
    if ns == 0 {
        panic!("TIMER_RESOLUTION must be greater than zero");
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let path = out_dir.join("timer_resolution.rs");
    let contents = format!(
        "/// System-dependent timer resolution, in nanoseconds. Supplied at\n\
         /// build time via the `TIMER_RESOLUTION` environment variable; no\n\
         /// default value is provided.\n\
         pub const TIMER_RESOLUTION_NS: u64 = {ns};\n"
    );
    fs::write(&path, contents).expect("failed to write timer_resolution.rs");
}
