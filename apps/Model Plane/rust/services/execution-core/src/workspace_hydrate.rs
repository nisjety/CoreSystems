//! Hydrate a Space's durable workspace onto local disk before a run starts,
//! and diff + upload what changed back to CAS afterward. See
//! apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
//! §3 and §8 item 3.5.C.
//!
//! Driven by `sandbox_lease.rs`, the only module that calls `hydrate` and
//! `diff_and_upload`. The first Space-scoped `code_interpreter` call or
//! `process_start` of a run reaches `hydrate` through `ensure_hydrated_workspace`
//! (the other `process_*` tools reuse an existing lease), and
//! `release_sandbox_lease_if_any` reaches `diff_and_upload` when the run ends:
//! from `CancelRun` or the `RunAgent` finalise. A run driven only by
//! `ExecuteStep` calls has no run-end signal, so its edits are not uploaded
//! (see the module doc of `sandbox_lease.rs`).

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use mp_contracts::attachments::ATTACHMENTS_DIR;
use sha2::{Digest, Sha256};

use crate::code_interpreter::{Language, MAX_FILE_BYTES, MPL_CONFIG_SUBDIR, TEMP_SUBDIR};
use crate::scrub;
use crate::workspace_cas::CasClient;

/// One file a caller wants hydrated — this run's baseline view of a Space's
/// durable workspace, as sandbox-manager's `GetWorkspaceManifest` RPC
/// reports it (`WorkspaceManifestEntry` on the wire; this is `hydrate`'s own
/// caller-facing shape, kept distinct from the generated proto type the same
/// way every other module in this crate wraps its wire types).
#[derive(Debug, Clone)]
pub struct WorkspaceFileEntry {
    pub path: String,
    pub content_hash: String,
}

/// One file [`diff_and_upload`] found changed or newly created, ready to
/// report to `SnapshotSandbox`'s `changed_files` field as this run's overlay
/// (and, eventually, to sandbox-manager's `PromoteWorkspace` RPC — S3.3 step
/// 4, not built yet — which is what actually merges it into the Space).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    /// The Space-level hash this path had when `hydrate` observed it —
    /// `baseline`'s own value for this path, carried through rather than
    /// re-read at upload time. `None` means the path did not exist yet when
    /// this run hydrated. Step 4's `PromoteWorkspace` compare-and-swap merge
    /// needs what this run actually saw, not whatever the Space row says by
    /// the time of upload (which could have changed if another run promoted
    /// first).
    pub base_hash: Option<String>,
}

/// Fetches every entry in `manifest` from CAS and writes it under
/// `target_dir`, returning the `path -> content_hash` baseline
/// [`diff_and_upload`] later compares against.
///
/// Rejects any entry whose `path` could escape `target_dir` (an absolute
/// path, or one with a `..` component) without touching the filesystem for
/// it — a manifest is data from a durable store, not something this
/// function trusts as an already-safe filesystem operation by construction.
///
/// # Errors
/// The first unsafe path, CAS fetch failure, or filesystem error stops
/// hydration entirely; a caller wanting "best effort" partial hydration
/// must do so explicitly, entry by entry, rather than get it silently from
/// this function.
pub async fn hydrate(
    cas: &CasClient,
    manifest: &[WorkspaceFileEntry],
    target_dir: &Path,
) -> Result<HashMap<String, String>, String> {
    let mut baseline = HashMap::with_capacity(manifest.len());
    for entry in manifest {
        let relative = safe_relative_path(&entry.path)?;
        let content = cas.get(&entry.content_hash).await?;
        let destination = target_dir.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("hydrate: could not create '{}': {error}", parent.display()))?;
        }
        std::fs::write(&destination, &content).map_err(|error| {
            format!("hydrate: could not write '{}': {error}", destination.display())
        })?;
        baseline.insert(entry.path.clone(), entry.content_hash.clone());
    }
    Ok(baseline)
}

/// Walks `target_dir` recursively, uploads every file whose content differs
/// from (or is absent from) `baseline` to CAS — redacting known secret
/// patterns first via the same [`scrub::scrub_string`] this crate already
/// uses for checkpoint/step payloads — and returns the changed set.
/// Unchanged files are neither re-uploaded (`CasClient::put` already
/// deduplicates by content hash, but this avoids even that round trip) nor
/// included in the result. Symlinks are neither followed nor uploaded — an
/// uploaded symlink target could point outside the workspace.
///
/// # Errors
/// The first filesystem read error or CAS upload failure stops the diff; no
/// partial "changed" list is returned on failure.
pub async fn diff_and_upload(
    cas: &CasClient,
    target_dir: &Path,
    baseline: &HashMap<String, String>,
) -> Result<Vec<ChangedFile>, String> {
    let mut relative_paths = Vec::new();
    collect_relative_paths(target_dir, target_dir, &mut relative_paths)?;

    let mut changed = Vec::new();
    for relative in relative_paths {
        let path_str = relative.to_string_lossy().replace('\\', "/");
        let absolute = target_dir.join(&relative);
        let raw = std::fs::read(&absolute)
            .map_err(|error| format!("diff: could not read '{}': {error}", absolute.display()))?;
        let redacted = redact(&raw);
        let content_hash = sha256_digest(&redacted);

        let base_hash = baseline.get(&path_str).cloned();
        if base_hash.as_ref() == Some(&content_hash) {
            continue;
        }
        let uploaded_hash = cas.put(&redacted).await?;
        changed.push(ChangedFile {
            path: path_str,
            size_bytes: redacted.len() as u64,
            content_hash: uploaded_hash,
            base_hash,
        });
    }
    Ok(changed)
}

/// Redacts known secret patterns in `raw` if it decodes as valid UTF-8 text,
/// leaving it byte-for-byte unchanged otherwise.
///
/// Rust's `String`/`&str` must be valid UTF-8 — unlike Go's byte-oriented
/// string type, which is how sandbox-manager's own `ExcludeCredentials`
/// (`internal/snapshot/exclude.go`) gets away with the same round-trip on
/// arbitrary bytes. Lossily converting a genuinely binary file (an image, a
/// compiled artifact) via `String::from_utf8_lossy` would silently corrupt
/// it by replacing invalid byte sequences with U+FFFD — content that must
/// stay byte-identical to be useful. Binary content therefore passes
/// through untouched rather than risk that; it also isn't the kind of
/// content these patterns are meant to catch.
pub(crate) fn redact(raw: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(raw) {
        Ok(text) => scrub::scrub_string(text).into_bytes(),
        Err(_) => raw.to_vec(),
    }
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// The version token of one file's content: the `sha256:<hex>` digest of its
/// redacted bytes. It is the CAS hash [`diff_and_upload`] uploads those bytes
/// under, so a version handed out now is the one a later diff compares against.
#[must_use]
pub fn content_version(raw: &[u8]) -> String {
    sha256_digest(&redact(raw))
}

/// Whether redaction would change `raw`, that is, the file holds a known secret
/// pattern. Binary content is never redacted and so is never reported.
#[must_use]
pub fn would_redact(raw: &[u8]) -> bool {
    redact(raw) != raw
}

/// Validates `path` is a safe, relative path with no `..`/absolute/prefix
/// components, returning it as a `PathBuf` a caller can safely `.join()`
/// onto a target directory.
pub(crate) fn safe_relative_path(path: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(path);
    if candidate.as_os_str().is_empty() {
        return Err("workspace path must not be empty".to_owned());
    }
    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("workspace path '{path}' is not a safe relative path"));
    }
    Ok(candidate)
}

pub(crate) fn collect_relative_paths(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("diff: could not list '{}': {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("diff: could not read a directory entry in '{}': {error}", dir.display())
        })?;
        let file_type = entry.file_type().map_err(|error| {
            format!("diff: could not stat '{}': {error}", entry.path().display())
        })?;
        if file_type.is_dir() {
            collect_relative_paths(root, &entry.path(), out)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| format!("diff: path outside its own root: {error}"))?
                .to_path_buf();
            out.push(relative);
        }
    }
    Ok(())
}

/// Whether the thread workspace keeps the file at `path` (relative, `/`-separated) between calls
/// (WS22-S2). It keeps neither the program the runtime stages nor the scratch directories it creates
/// at the top of the tree, nor Python's bytecode cache, nor the read-only attachments mount, nor a
/// file over the per-file ceiling, which a call's output cannot carry either.
#[must_use]
pub fn is_syncable(path: &str, size_bytes: u64) -> bool {
    if size_bytes > MAX_FILE_BYTES || path.split('/').any(|part| part == "__pycache__") {
        return false;
    }
    let mut parts = path.split('/');
    let top = parts.next().unwrap_or_default();
    if parts.next().is_none() {
        return top != Language::Python.program_file() && top != Language::Sh.program_file();
    }
    !(top == TEMP_SUBDIR || top == MPL_CONFIG_SUBDIR || top.eq_ignore_ascii_case(ATTACHMENTS_DIR))
}

/// The baseline paths that no longer exist under `target_dir`, in path order (WS22-S2). A path whose
/// state cannot be read is not reported: an unreadable file is not a deleted one.
#[must_use]
pub fn diff_deletions(target_dir: &Path, baseline: &HashMap<String, String>) -> Vec<String> {
    let mut deleted: Vec<String> = baseline
        .keys()
        .filter(|path| {
            matches!(
                std::fs::symlink_metadata(target_dir.join(path.as_str())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            )
        })
        .cloned()
        .collect();
    deleted.sort();
    deleted
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn test_client(server: &MockServer) -> CasClient {
        CasClient::new_with_transport(&server.uri(), "test-bucket", "access-key", &"a".repeat(32), "us-east-1")
            .expect("valid transport config")
    }

    #[tokio::test]
    async fn hydrate_writes_files_and_returns_baseline() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/test-bucket/cas/.+$"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello workspace".to_vec()))
            .mount(&server)
            .await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("hydrate-writes");
        let manifest = vec![
            WorkspaceFileEntry { path: "readme.md".to_owned(), content_hash: format!("sha256:{}", "a".repeat(64)) },
            WorkspaceFileEntry { path: "src/main.py".to_owned(), content_hash: format!("sha256:{}", "b".repeat(64)) },
        ];

        let baseline = hydrate(&cas, &manifest, &target_dir).await.expect("hydrate succeeds");

        assert_eq!(std::fs::read(target_dir.join("readme.md")).unwrap(), b"hello workspace");
        assert_eq!(std::fs::read(target_dir.join("src/main.py")).unwrap(), b"hello workspace");
        assert_eq!(baseline.get("readme.md").map(String::as_str), Some(format!("sha256:{}", "a".repeat(64)).as_str()));
        assert_eq!(baseline.get("src/main.py").map(String::as_str), Some(format!("sha256:{}", "b".repeat(64)).as_str()));

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[tokio::test]
    async fn hydrate_rejects_unsafe_paths_without_touching_cas() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"unused".to_vec()))
            .expect(0)
            .mount(&server)
            .await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("hydrate-rejects");
        for unsafe_path in ["../escape.txt", "/etc/passwd", "a/../../escape.txt"] {
            let manifest = vec![WorkspaceFileEntry {
                path: unsafe_path.to_owned(),
                content_hash: format!("sha256:{}", "a".repeat(64)),
            }];
            let result = hydrate(&cas, &manifest, &target_dir).await;
            assert!(result.is_err(), "path {unsafe_path:?} should have been rejected");
        }

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[tokio::test]
    async fn diff_and_upload_skips_unchanged_files_without_uploading() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;
        Mock::given(method("PUT")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("diff-unchanged");
        std::fs::write(target_dir.join("unchanged.txt"), b"same content").unwrap();
        let baseline = HashMap::from([(
            "unchanged.txt".to_owned(),
            sha256_digest(b"same content"),
        )]);

        let changed = diff_and_upload(&cas, &target_dir, &baseline).await.expect("diff succeeds");
        assert!(changed.is_empty(), "unchanged file should not be reported: {changed:?}");

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[tokio::test]
    async fn diff_and_upload_uploads_changed_and_new_files() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path_regex(r"^/test-bucket/cas/.+$"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path_regex(r"^/test-bucket/cas/.+$"))
            .respond_with(ResponseTemplate::new(200))
            .expect(2)
            .mount(&server)
            .await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("diff-changed");
        std::fs::write(target_dir.join("changed.txt"), b"new value").unwrap();
        std::fs::create_dir_all(target_dir.join("nested")).unwrap();
        std::fs::write(target_dir.join("nested/new.txt"), b"brand new").unwrap();
        let baseline = HashMap::from([("changed.txt".to_owned(), sha256_digest(b"old value"))]);

        let mut changed = diff_and_upload(&cas, &target_dir, &baseline).await.expect("diff succeeds");
        changed.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(changed.len(), 2);
        assert_eq!(changed[0].path, "changed.txt");
        assert_eq!(changed[0].content_hash, sha256_digest(b"new value"));
        assert_eq!(changed[0].base_hash, Some(sha256_digest(b"old value")), "a changed path carries the baseline hash it was compared against");
        assert_eq!(changed[1].path, "nested/new.txt");
        assert_eq!(changed[1].content_hash, sha256_digest(b"brand new"));
        assert_eq!(changed[1].base_hash, None, "a path absent from the baseline did not exist at hydrate time");

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[tokio::test]
    async fn diff_and_upload_redacts_before_uploading() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        let redacted_body = scrub::scrub_string("api_key: \"do-not-leak-this-value\"");
        assert!(!redacted_body.contains("do-not-leak-this-value"), "fixture must actually redact");
        Mock::given(method("PUT"))
            .and(body_string(redacted_body))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("diff-redacts");
        std::fs::write(target_dir.join("secret.txt"), b"api_key: \"do-not-leak-this-value\"").unwrap();

        diff_and_upload(&cas, &target_dir, &HashMap::new())
            .await
            .expect("diff succeeds; the mock only matches the redacted body, so an unredacted PUT would fail here");

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[tokio::test]
    async fn diff_and_upload_preserves_binary_content_unchanged() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        let binary_content: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01, 0x02, 0x80, 0x81];
        Mock::given(method("PUT"))
            .and(body_string_bytes(binary_content.clone()))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("diff-binary");
        std::fs::write(target_dir.join("image.bin"), &binary_content).unwrap();

        let changed = diff_and_upload(&cas, &target_dir, &HashMap::new())
            .await
            .expect("diff succeeds; the mock only matches byte-identical binary content");
        assert_eq!(changed[0].content_hash, sha256_digest(&binary_content));

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[test]
    fn redaction_is_idempotent_over_fixture_corpus() {
        // One sample per pattern in `scrub.rs`. A pattern whose output the same
        // pattern matches again would make a hydrated file look changed at every
        // run end and be re-uploaded.
        let corpus = [
            "Authorization: Bearer abcDEF123.ghi-JKL/mno=",
            "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJlVmFsdWU",
            "openai sk-proj-abcdefghijklmnop1234",
            "aws AKIAIOSFODNN7EXAMPLE",
            "github ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "slack xoxb-1234567890-abcdefghij",
            // Assembled so the file holds no key-shaped literal for a secret scanner to flag.
            concat!("google AI", "za0123456789abcdefghijklmnopqrstuvwxy"),
            "header Basic dXNlcjpwYXNzd29yZA==",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIBOgIBAAJBAKj\n-----END RSA PRIVATE KEY-----",
            "postgres://svc:hunter2pass@db.internal:5432/app",
            "DB_PASSWORD=hunter2hunter2",
            "api_key: \"xyz123456\"",
        ];
        let combined = corpus.join("\n");
        for sample in corpus.iter().copied().chain(std::iter::once(combined.as_str())) {
            let raw = sample.as_bytes();
            assert!(would_redact(raw), "fixture must actually match a pattern: {sample:?}");
            let once = redact(raw);
            assert_eq!(redact(&once), once, "redaction must be idempotent: {sample:?}");
        }
    }

    #[test]
    fn would_redact_is_false_for_clean_and_binary_content() {
        assert!(!would_redact(b"plain notes, nothing secret"));
        assert!(!would_redact(&[0xFF, 0xFE, 0x00, 0x80]));
    }

    #[tokio::test]
    async fn unchanged_hydrated_file_is_never_reuploaded() {
        let server = MockServer::start().await;
        Mock::given(method("PUT")).respond_with(ResponseTemplate::new(200)).expect(0).mount(&server).await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("diff-hydrated-secret");
        let raw = b"api_key: \"xyz123456\"";
        std::fs::write(target_dir.join("config.txt"), raw).unwrap();
        // The baseline is what CAS stored when the file was last uploaded: the
        // version of its redacted bytes, not of the raw bytes on disk.
        let baseline = HashMap::from([("config.txt".to_owned(), content_version(raw))]);

        let changed = diff_and_upload(&cas, &target_dir, &baseline).await.expect("diff succeeds");
        assert!(changed.is_empty(), "a file that redacts to its baseline is unchanged: {changed:?}");

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[tokio::test]
    async fn content_version_equals_cas_put_hash() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        Mock::given(method("PUT")).respond_with(ResponseTemplate::new(200)).mount(&server).await;

        let cas = test_client(&server).await;
        let target_dir = tempfile_dir("content-version");
        let raw = b"api_key: \"xyz123456\" and more text";
        std::fs::write(target_dir.join("notes.txt"), raw).unwrap();

        let changed = diff_and_upload(&cas, &target_dir, &HashMap::new()).await.expect("diff succeeds");
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].content_hash, content_version(raw), "the version a caller reads is the hash CAS stores under");
        assert!(changed[0].content_hash.starts_with("sha256:"));

        std::fs::remove_dir_all(&target_dir).ok();
    }

    #[test]
    fn is_syncable_keeps_user_files_and_drops_runtime_scratch() {
        assert!(is_syncable("notes.txt", 10));
        assert!(is_syncable("reports/q3.csv", 10));
        assert!(is_syncable("big.bin", MAX_FILE_BYTES));
        assert!(!is_syncable("_verevon_main.py", 10));
        assert!(!is_syncable("_verevon_main.sh", 10));
        assert!(!is_syncable(".tmp/scratch.txt", 10));
        assert!(!is_syncable(".mplconfig/fontlist.json", 10));
        assert!(!is_syncable("attachments/input.csv", 10));
        assert!(!is_syncable("Attachments/input.csv", 10));
        assert!(!is_syncable("pkg/__pycache__/m.pyc", 10));
        assert!(!is_syncable("big.bin", MAX_FILE_BYTES + 1));
    }

    #[test]
    fn diff_deletions_names_only_the_baseline_paths_now_gone() {
        let dir = tempfile_dir("diff-deletions");
        std::fs::write(dir.join("kept.txt"), b"still here").unwrap();
        let baseline = HashMap::from([
            ("kept.txt".to_owned(), sha256_digest(b"still here")),
            ("gone.txt".to_owned(), sha256_digest(b"removed")),
        ]);
        assert_eq!(diff_deletions(&dir, &baseline), vec!["gone.txt".to_owned()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn body_string_bytes(expected: Vec<u8>) -> impl wiremock::Match {
        struct BytesMatch(Vec<u8>);
        impl wiremock::Match for BytesMatch {
            fn matches(&self, request: &wiremock::Request) -> bool {
                request.body == self.0
            }
        }
        BytesMatch(expected)
    }

    fn tempfile_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "execution-core-workspace-hydrate-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }
}
