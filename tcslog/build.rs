//! Bakes the system's timer resolution into the crate.
//!
//! The segment ID of a segment file is the time at which it was
//! created, so two segment files created within one tick of the
//! writer's clock would collide. The writer resolves a collision by
//! sleeping for twice the clock's resolution and reading the time
//! again, which is expected to yield a larger value.
//!
//! That resolution is a property of the machine rather than of the
//! code, so it is supplied from outside as the `TIMER_RESOLUTION`
//! environment variable, in nanoseconds -- usually from
//! `.cargo/config.toml`. There is deliberately no default: a value
//! guessed here would be wrong on some machine.
//!
//! The value is a starting point rather than a figure that has to be
//! right. It is genuinely hard to establish from outside -- nothing in
//! the standard library reports it, and what a `thread::sleep` of a
//! given length actually waits is bounded only loosely -- so a writer
//! corrects a value that proves too small. One collision is what the
//! value exists to resolve; a second doubles it, and each collision
//! after that doubles it again, and the writer keeps what it arrived
//! at. `LogWrite::timer_resolution` reports that figure, which is the
//! one to give the next build.
//!
//! A missing or unparseable value is a build failure. A zero value
//! builds, because zero is a number the caller can be told about at
//! run time: `LogWrite::new` rejects it with `TimerResolutionZero`.
//!
//! Only the `write` feature needs the value, so a read-only build does
//! not require one.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TIMER_RESOLUTION");

    if env::var_os("CARGO_FEATURE_WRITE").is_none() {
        return;
    }

    let raw = env::var("TIMER_RESOLUTION").unwrap_or_else(|_| {
        panic!(
            "TIMER_RESOLUTION is not set. It is the system timer \
             resolution in nanoseconds and has no default. Set it in \
             .cargo/config.toml (see .cargo/config.toml.example) or in \
             the environment, e.g. TIMER_RESOLUTION=1 cargo build."
        )
    });

    let ns: u64 = raw
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("TIMER_RESOLUTION={raw:?} is not a nanosecond count: {e}"));

    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR for build scripts");
    let dest = Path::new(&out_dir).join("timer_resolution.rs");
    let text = format!(
        "/// System timer resolution in nanoseconds, from the \
         build-time `TIMER_RESOLUTION`.\n\
         pub const TIMER_RESOLUTION_NS: u64 = {ns};\n"
    );
    fs::write(&dest, text).unwrap_or_else(|e| panic!("cannot write {}: {e}", dest.display()));
}
