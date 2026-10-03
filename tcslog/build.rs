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
//! `.cargo/config.toml`. No figure is guessed here, because a figure
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
//! An absent value is zero rather than a build failure. The crate has
//! to build without one: a dependent taking tcslog from a registry has
//! no `.cargo/config.toml` of this repository's, and a build that
//! stopped would leave them with a failure in somebody else's build
//! script rather than anything they could act on. Zero is instead a
//! number the caller is told about at run time, where it can name
//! itself: `LogWrite::new` refuses to open a log and reports
//! `TimerResolutionZero`. Reading a log is unaffected either way.
//!
//! An unparseable value is still a build failure. Absence means nobody
//! said; a value that is not a nanosecond count means somebody said
//! something wrong, which is worth stopping for rather than silently
//! reading as zero.
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

    // Absent is zero, which `LogWrite::new` reports at run time. An
    // empty or blank setting counts as absent: it says no more than not
    // setting the variable at all.
    let ns: u64 = match env::var("TIMER_RESOLUTION") {
        Err(_) => 0,
        Ok(raw) if raw.trim().is_empty() => 0,
        Ok(raw) => raw.trim().parse().unwrap_or_else(|e| {
            panic!(
                "TIMER_RESOLUTION={raw:?} is not a nanosecond count: {e}. \
                 Give it a count in nanoseconds, or leave it unset -- an \
                 unset value builds, and LogWrite::new then reports \
                 TimerResolutionZero."
            )
        }),
    };

    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR for build scripts");
    let dest = Path::new(&out_dir).join("timer_resolution.rs");
    let text = format!(
        "/// System timer resolution in nanoseconds, from the \
         build-time `TIMER_RESOLUTION`, or zero if it was not set.\n\
         pub const TIMER_RESOLUTION_NS: u64 = {ns};\n"
    );
    fs::write(&dest, text).unwrap_or_else(|e| panic!("cannot write {}: {e}", dest.display()));
}
