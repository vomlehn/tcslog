//! Gives the shared library the soname it is installed under.
//!
//! Cargo names the library after the crate, `libtcslog_c.so`, and that is
//! the name the tests and examples in this repository load it by. `make
//! install` installs it as `libtcslog.so.<abi>`, with `libtcslog.so`
//! linked to it. A program linked against it records the soname as what
//! it needs, not the file it was linked with, so the soname has to be
//! `libtcslog.so.<abi>`: that is the name the loader looks for, and a
//! library whose ABI has changed is then not mistaken for the one the
//! program was built against.
//!
//! `<abi>` is the whole number in `abi-version`, beside this file, which
//! the Makefile reads for the installed names as well, so that the two
//! cannot disagree. It is the ABI's version, not the package's, and moves
//! only when the ABI changes incompatibly: when a function's signature
//! or behaviour changes incompatibly, when a function is removed, or
//! when a struct or enum the header exposes changes its layout or its
//! values. Adding a function, or a status after the last, changes
//! nothing a program already built relies on, and does not move it.
//!
//! Only ELF platforms have sonames. Apple's have install names instead,
//! and Windows has neither, so nothing is said to the linker there; the
//! file is still checked, so that a bad one fails every build alike.

use std::env;
use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=abi-version");

    let text = fs::read_to_string("abi-version")
        .expect("tcslog-c/abi-version must exist and be readable");
    let abi: u32 = text.trim().parse().unwrap_or_else(|_| {
        panic!(
            "tcslog-c/abi-version holds {:?}; it must hold one whole \
             number, the ABI version, such as 1",
            text.trim()
        )
    });

    let family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let vendor = env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    if !family.split(',').any(|f| f == "unix") || vendor == "apple" {
        return;
    }
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libtcslog.so.{abi}");
}
