//! The document-parser child as a stand-alone executable.
//!
//! `quarry-edge-rs --doc-parse <pdf|docx>` is the same thing inside the edge
//! binary and is what runs in production; this one exists so the runtime crate's
//! tests can start a real child process without building the edge.

fn main() {
    let kind = std::env::args().nth(1);
    std::process::exit(quarry_runtime::content_route::run_child(kind.as_deref()));
}
