//! Embeds every `skills/*.md` into the binary. Generating the list (instead of maintaining it by
//! hand) means a skill file can never be forgotten, and the server never reads skills from disk
//! at runtime, so what was tested is exactly what is served.
//!
//! The build fails loudly rather than shipping fewer skills than exist: a missing or unreadable
//! `skills/` directory, an unreadable entry, or an empty skill set is a build error, and a file that
//! is not `*.md` is reported as a warning (a file named `x.markdown` or `x.MD` is almost certainly a
//! mistake, and would otherwise be skipped without a word).

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let manifest =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let dir = manifest.join("skills");
    println!("cargo:rerun-if-changed={}", dir.display());

    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read the skills directory {}: {e}", dir.display()));
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", dir.display()))
            .path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            files.push(path);
        } else if path.is_file() {
            println!(
                "cargo:warning=skills/{} is not a `.md` file and is not embedded",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            );
        }
    }
    files.sort();
    assert!(
        !files.is_empty(),
        "no skills found in {}: the server must not ship without its usage guides",
        dir.display()
    );

    let mut out =
        String::from("/// (file name, contents) of every embedded skill, sorted by file name.\n");
    out.push_str("pub const EMBEDDED_SKILLS: &[(&str, &str)] = &[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let name = f
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name");
        // A raw string with enough hashes survives any path (this repo's path contains a space).
        assert!(
            !f.display().to_string().contains("\"###"),
            "a skill path must not contain the raw-string terminator"
        );
        writeln!(
            out,
            "    (\"{name}\", include_str!(r###\"{}\"###)),",
            f.display()
        )
        .expect("write to String");
    }
    out.push_str("];\n");

    let target = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"))
        .join("embedded_skills.rs");
    std::fs::write(target, out).expect("write embedded_skills.rs");
}
