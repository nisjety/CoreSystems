//! Contract tests for the workspace edit engine (WS24-S2), through its public API
//! and the run's workspace lock, as the Do-loop tools will call it.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use execution_core::state::StateStore;
use execution_core::workspace_edit::{self, EditError, Version, WorkspaceLock, WorkspaceRoot};
use futures::future::join_all;

/// A fresh directory under the OS temp dir, removed on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!("verevon-ws-contract-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        Self { dir }
    }

    fn root(&self) -> WorkspaceRoot {
        WorkspaceRoot::new(&self.dir).expect("root")
    }

    fn write(&self, rel: &str, bytes: &[u8]) {
        fs::write(self.dir.join(rel), bytes).expect("write fixture");
    }

    fn bytes(&self, rel: &str) -> Vec<u8> {
        fs::read(self.dir.join(rel)).expect("read back")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn an_edit_version_after_is_the_token_a_later_read_returns() {
    let scratch = Scratch::new("version-after");
    scratch.write("notes.txt", b"alpha\nbeta\n");
    let root = scratch.root();
    let read_before = workspace_edit::read(&root, "notes.txt", 1, None).expect("read");
    let outcome = workspace_edit::edit(&root, "notes.txt", "beta", "gamma", false, Some(&read_before.version))
        .expect("edit");
    let read_after = workspace_edit::read(&root, "notes.txt", 1, None).expect("read again");
    assert_eq!(outcome.version_after, read_after.version);
    assert_eq!(outcome.version_after.as_str(), workspace_edit::Version::of(&scratch.bytes("notes.txt")).as_str());
}

#[test]
fn a_stale_edit_is_refused_and_leaves_the_file_as_it_was() {
    let scratch = Scratch::new("stale");
    scratch.write("notes.txt", b"alpha\n");
    let root = scratch.root();
    let seen = workspace_edit::read(&root, "notes.txt", 1, None).expect("read").version;
    scratch.write("notes.txt", b"alpha moved on\n");
    let error = workspace_edit::edit(&root, "notes.txt", "alpha", "beta", false, Some(&seen)).expect_err("stale");
    assert_eq!(error.code(), "stale_version");
    assert_eq!(scratch.bytes("notes.txt"), b"alpha moved on\n");
}

#[test]
fn escapes_absolute_paths_and_backslashes_are_refused() {
    let scratch = Scratch::new("escapes");
    let root = scratch.root();
    for rel in ["../outside.txt", "/etc/passwd", "sub\\file.txt", "sub/../../x.txt"] {
        let error = workspace_edit::put(&root, rel, "x", None).expect_err(rel);
        assert_eq!(error.code(), "outside_root", "{rel}");
    }
    assert!(!scratch.dir.parent().expect("parent").join("outside.txt").exists());
}

#[cfg(unix)]
#[test]
fn a_symlink_is_refused_for_reads_and_writes() {
    let scratch = Scratch::new("symlink");
    scratch.write("real/notes.txt", b"alpha\n");
    std::os::unix::fs::symlink(scratch.dir.join("real"), scratch.dir.join("link")).expect("symlink");
    let root = scratch.root();
    assert_eq!(workspace_edit::read(&root, "link/notes.txt", 1, None).expect_err("read").code(), "symlink_refused");
    assert_eq!(
        workspace_edit::put(&root, "link/new.txt", "x", None).expect_err("put").code(),
        "symlink_refused"
    );
}

#[test]
fn a_binary_file_is_refused_as_not_utf8() {
    let scratch = Scratch::new("binary");
    scratch.write("blob.bin", &[0x89, 0x50, 0x00, 0x47]);
    let root = scratch.root();
    let error = workspace_edit::edit(
        &root,
        "blob.bin",
        "x",
        "y",
        false,
        Some(&Version::of(&scratch.bytes("blob.bin"))),
    )
    .expect_err("binary");
    assert_eq!(error, EditError::NotUtf8);
}

#[test]
fn crlf_is_preserved_through_an_edit() {
    let scratch = Scratch::new("crlf");
    scratch.write("win.txt", b"first\r\nsecond\r\nthird\r\n");
    let root = scratch.root();
    let seen = workspace_edit::read(&root, "win.txt", 1, None).expect("read").version;
    workspace_edit::edit(&root, "win.txt", "second", "middle\nlines", false, Some(&seen)).expect("edit");
    assert_eq!(scratch.bytes("win.txt"), b"first\r\nmiddle\r\nlines\r\nthird\r\n");
}

#[tokio::test]
async fn concurrent_edits_from_one_version_serialise_with_exactly_one_winner() {
    let scratch = Scratch::new("concurrent");
    scratch.write("shared.txt", b"alpha\n");
    let root = scratch.root();
    let seen = workspace_edit::read(&root, "shared.txt", 1, None).expect("read").version;
    let lock: WorkspaceLock = StateStore::new().workspace_lock("run-1");
    let attempts = (0..8).map(|n| {
        let lock = Arc::clone(&lock);
        let root = root.clone();
        let seen = seen.clone();
        async move {
            let _guard = lock.write().await;
            workspace_edit::edit(&root, "shared.txt", "alpha", &format!("beta{n}"), false, Some(&seen))
        }
    });
    let results = join_all(attempts).await;
    let winners = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(winners, 1, "exactly one edit from the same version may land");
    assert!(results
        .iter()
        .filter_map(|result| result.as_ref().err())
        .all(|error| error.code() == "stale_version"));
}
