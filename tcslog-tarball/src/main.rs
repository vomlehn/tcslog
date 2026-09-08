//! Package `docs/tcslog-prompt.rst` into `tcslog.tar.gz` at the workspace root.

use std::env;
use std::fs::File;
use std::path::PathBuf;
use std::process;

use flate2::write::GzEncoder;
use flate2::Compression;

fn main() {
    let workspace_root = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|p| {
            p.parent()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| env::VarError::NotPresent)
        })
        .unwrap_or_else(|_| env::current_dir().expect("cwd"));

    let prompt = workspace_root.join("docs/tcslog-prompt.rst");
    let out = workspace_root.join("tcslog.tar.gz");

    if let Err(e) = build(&workspace_root, &prompt, &out) {
        eprintln!("tcslog-tarball: {e}");
        process::exit(1);
    }
    println!("wrote {}", out.display());
}

fn build(root: &PathBuf, prompt: &PathBuf, out: &PathBuf) -> std::io::Result<()> {
    let gz = GzEncoder::new(File::create(out)?, Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.append_path_with_name(
        prompt,
        prompt
            .strip_prefix(root)
            .unwrap_or_else(|_| std::path::Path::new("tcslog-prompt.rst")),
    )?;
    tar.into_inner()?.finish()?;
    Ok(())
}
