//! Links the system libraries FFmpeg's static build depends on.
//! ffmpeg-sys-next links the libav* archives; this adds what their
//! configure step recorded in EXTRALIBS (see scripts/build-ffmpeg.sh).
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    let dir = env::var("FFMPEG_DIR")
        .expect("FFMPEG_DIR is not set: run scripts/build-ffmpeg.sh, then `source scripts/env.sh`");
    let path = PathBuf::from(dir).join("extralibs.txt");
    println!("cargo:rerun-if-changed={}", path.display());
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut seen = BTreeSet::new();
    let mut tokens = text.split_whitespace();
    while let Some(token) = tokens.next() {
        let directive = if token == "-framework" {
            tokens.next().map(|f| format!("framework={f}"))
        } else if let Some(lib) = token.strip_prefix("-l") {
            Some(lib.to_string())
        } else {
            token.strip_suffix(".lib").map(str::to_string)
        };
        if let Some(d) = directive
            && seen.insert(d.clone())
        {
            println!("cargo:rustc-link-lib={d}");
        }
    }
}
