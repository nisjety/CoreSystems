//! Read-only `/skills` mounts for `code_interpreter` (build plan WS19-S8).
//!
//! A turn's matched skills may carry a bundle an admin uploaded (WS19-S6): the
//! scripts and reference files the skill's instructions refer to. `model-gateway`
//! names those skills in `ExecuteStepRequest.skill_mounts`, taken from the skills
//! the turn matched and never from the model's arguments. This module fetches each
//! named bundle from capability-core, checks every digest it can (the manifest
//! digest the gateway named, the manifest digest recomputed from the manifest's own
//! rows, and the digest and size of every blob), and writes the files into a
//! directory of the call's own: directories 0555, files 0444 and scripts 0555.
//!
//! [`crate::code_interpreter`] binds that directory read-only at `/skills` inside
//! the sandbox. A program can run a bundle's scripts and read its files, a write
//! fails with `EROFS` from the kernel, and the sandbox has no network. The directory
//! is removed when the call returns, on every path, as the code workspace is.
//!
//! Bundles are org configuration, not conversation content, so nothing here is
//! persisted: the blob cache is process memory, keyed by `(org, sha256)` and
//! bounded, and a tree lives only as long as the call that holds it. A step that
//! names no mounts never reaches this module, and under zero retention it is
//! refused by [`validate`] before any read.

use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use mp_contracts::model_plane::v1::capability_core_client::CapabilityCoreClient;
use mp_contracts::model_plane::v1::{
    GetSkillBlobRequest, GetSkillBundleManifestRequest, SkillBundleFile, SkillBundleManifest,
    SkillMount,
};
use mp_contracts::skill_bundle::validate_bundle_path;
use mp_contracts::zdr_refusal::Refusal;
use sha2::{Digest, Sha256};
use tonic::transport::Channel;
use tonic::{Code, Request, Status};

use crate::capability_policy::ServiceTokenProvider;
use crate::code_interpreter::{set_mode, SKILLS_DIR};

/// Most bundles one call mounts. The gateway injects at most three skills a turn.
pub const MAX_SKILL_MOUNTS: usize = 3;
/// capability-core's upload caps, checked again here: a digest says what a file is,
/// not how large a tree may grow.
const MAX_BUNDLE_FILES: usize = 256;
const MAX_BUNDLE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Longest directory name a skill may mount under.
const MAX_NAME_BYTES: usize = 48;
/// Names the sandbox already gives a meaning to, which no skill may take.
const RESERVED_NAMES: [&str; 2] = [SKILLS_DIR, "attachments"];
/// Blob bytes kept between calls. Bounded in entries and in bytes.
const BLOB_CACHE_ENTRIES: usize = 64;
const BLOB_CACHE_BYTES: usize = 16 * 1024 * 1024;
/// Prefix of a tree directory's name under the temp directory.
const TREE_PREFIX: &str = "verevon-skills";
/// The reason a read token is minted for. Auth Core audits it.
const TOKEN_REASON: &str = "skill bundle mount for code_interpreter";

/// Why a request's bundle mounts were refused. Each token is closed, and it reaches
/// the caller in place of any detail about the bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountRefusal {
    /// The step runs under zero retention or a waiver, which never reads a bundle.
    ZeroRetention,
    /// The mounts ride on a tool other than `code_interpreter`.
    UnsupportedTool,
    TooMany,
    SkillIdMissing,
    NameInvalid,
    NameReserved,
    NameDuplicate,
    DigestInvalid,
}

impl MountRefusal {
    /// The closed token a refusal carries.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::ZeroRetention => "skill_mounts_zdr",
            Self::UnsupportedTool => "skill_mounts_unsupported_tool",
            Self::TooMany => "skill_mounts_too_many",
            Self::SkillIdMissing => "skill_mounts_skill_id_missing",
            Self::NameInvalid => "skill_mounts_name_invalid",
            Self::NameReserved => "skill_mounts_name_reserved",
            Self::NameDuplicate => "skill_mounts_name_duplicate",
            Self::DigestInvalid => "skill_mounts_digest_invalid",
        }
    }
}

fn refusal_status(refusal: MountRefusal) -> Status {
    if refusal == MountRefusal::ZeroRetention {
        Refusal::feature_not_supported("skill_mounts").grpc_status(Code::FailedPrecondition)
    } else {
        Status::invalid_argument(refusal.token())
    }
}

/// Judges a request's mounts before anything is read. An empty list is always
/// accepted. `tool_name` and `zdr_or_waived` come from the authorized request.
///
/// # Errors
/// The first [`MountRefusal`] that applies.
pub fn validate(
    mounts: &[SkillMount],
    tool_name: &str,
    zdr_or_waived: bool,
) -> Result<(), MountRefusal> {
    if mounts.is_empty() {
        return Ok(());
    }
    if zdr_or_waived {
        return Err(MountRefusal::ZeroRetention);
    }
    if tool_name != "code_interpreter" {
        return Err(MountRefusal::UnsupportedTool);
    }
    if mounts.len() > MAX_SKILL_MOUNTS {
        return Err(MountRefusal::TooMany);
    }
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for mount in mounts {
        if mount.skill_id.trim().is_empty() {
            return Err(MountRefusal::SkillIdMissing);
        }
        if !is_sha256_hex(&mount.manifest_sha256) {
            return Err(MountRefusal::DigestInvalid);
        }
        if RESERVED_NAMES.contains(&mount.name.as_str()) {
            return Err(MountRefusal::NameReserved);
        }
        if !name_is_valid(&mount.name) {
            return Err(MountRefusal::NameInvalid);
        }
        if !names.insert(mount.name.as_str()) {
            return Err(MountRefusal::NameDuplicate);
        }
    }
    Ok(())
}

/// Whether `name` is one directory name the sandbox can hold: lowercase ASCII
/// letters, digits, `-` and `_`, starting with a letter or a digit, and at most
/// [`MAX_NAME_BYTES`] bytes.
#[must_use]
pub fn name_is_valid(name: &str) -> bool {
    let starts_well = name
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    starts_well
        && name.len() <= MAX_NAME_BYTES
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The manifest digest capability-core computes at upload: SHA-256 over one line per
/// file, in path order, `path TAB sha256 TAB size TAB executable LF` with the
/// executable flag spelled `1` or `0`. It is recomputed here, so a manifest cannot
/// name a digest its own rows do not produce.
fn manifest_digest(files: &[SkillBundleFile]) -> String {
    let mut ordered: Vec<&SkillBundleFile> = files.iter().collect();
    ordered.sort_by(|a, b| a.path.cmp(&b.path));
    let mut hash = Sha256::new();
    for file in ordered {
        let executable = if file.executable { "1" } else { "0" };
        hash.update(
            format!(
                "{}\t{}\t{}\t{}\n",
                file.path, file.sha256, file.size_bytes, executable
            )
            .as_bytes(),
        );
    }
    format!("{:x}", hash.finalize())
}

fn io_status(error: std::io::Error) -> Status {
    tracing::warn!(%error, "skill bundle could not be written");
    Status::internal("skill_mounts_write_failed")
}

/// Refuses a manifest that is not the one the gateway named, or whose rows do not add
/// up to its digest and the caps. A path is checked the way `skill_file` checks one.
fn check_manifest(manifest: &SkillBundleManifest, mount: &SkillMount) -> Result<(), Status> {
    let mismatch = || Status::data_loss("skill_mounts_manifest_mismatch");
    let too_large = || Status::data_loss("skill_mounts_bundle_too_large");
    if manifest.skill_id != mount.skill_id || manifest.manifest_sha256 != mount.manifest_sha256 {
        return Err(mismatch());
    }
    if manifest.files.len() > MAX_BUNDLE_FILES {
        return Err(too_large());
    }
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    let mut total: u64 = 0;
    for file in &manifest.files {
        if validate_bundle_path(&file.path).is_err()
            || !paths.insert(file.path.as_str())
            || !is_sha256_hex(&file.sha256)
        {
            return Err(mismatch());
        }
        let size = u64::try_from(file.size_bytes)
            .ok()
            .filter(|size| *size <= MAX_FILE_BYTES)
            .ok_or_else(too_large)?;
        total += size;
    }
    if total > MAX_BUNDLE_BYTES {
        return Err(too_large());
    }
    if manifest_digest(&manifest.files) != manifest.manifest_sha256 {
        return Err(mismatch());
    }
    Ok(())
}

/// Where a bundle is read from: capability-core in production, a fake in tests.
#[tonic::async_trait]
pub trait SkillBundleSource: Send + Sync {
    /// The manifest of `skill_id`'s bundle, read for `org_id`.
    async fn manifest(&self, org_id: &str, skill_id: &str) -> Result<SkillBundleManifest, Status>;
    /// The bytes that hash to `sha256` in `skill_id`'s bundle, read for `org_id`.
    async fn blob(&self, org_id: &str, skill_id: &str, sha256: &str) -> Result<Vec<u8>, Status>;
}

/// capability-core over gRPC. Each read carries a `capability:read` token minted for
/// the org, the credential the dispatch policy already uses for its own reads.
pub struct GrpcSkillBundleSource {
    channel: Channel,
    tokens: ServiceTokenProvider,
}

impl GrpcSkillBundleSource {
    /// Builds the source from execution-core's service-token configuration.
    ///
    /// # Errors
    /// Returns an error when that configuration is missing or invalid.
    pub fn from_env(channel: Channel) -> anyhow::Result<Self> {
        Ok(Self {
            channel,
            tokens: ServiceTokenProvider::from_env()?,
        })
    }

    async fn authorized<T>(&self, org_id: &str, value: T) -> Result<Request<T>, Status> {
        let bearer = self
            .tokens
            .token_with_scopes(org_id, &["capability:read"], TOKEN_REASON)
            .await
            .map_err(|error| {
                tracing::warn!(%error, "skill bundle credential unavailable");
                Status::unavailable("skill bundle credential unavailable")
            })?;
        let mut request = Request::new(value);
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {bearer}")
                .parse()
                .map_err(|_| Status::internal("skill bundle credential is not forwardable"))?,
        );
        Ok(request)
    }
}

#[tonic::async_trait]
impl SkillBundleSource for GrpcSkillBundleSource {
    async fn manifest(&self, org_id: &str, skill_id: &str) -> Result<SkillBundleManifest, Status> {
        let request = self
            .authorized(
                org_id,
                GetSkillBundleManifestRequest {
                    skill_id: skill_id.to_owned(),
                },
            )
            .await?;
        CapabilityCoreClient::new(self.channel.clone())
            .get_skill_bundle_manifest(request)
            .await
            .map(tonic::Response::into_inner)
    }

    async fn blob(&self, org_id: &str, skill_id: &str, sha256: &str) -> Result<Vec<u8>, Status> {
        let request = self
            .authorized(
                org_id,
                GetSkillBlobRequest {
                    skill_id: skill_id.to_owned(),
                    sha256: sha256.to_owned(),
                },
            )
            .await?;
        CapabilityCoreClient::new(self.channel.clone())
            .get_skill_blob(request)
            .await
            .map(|response| response.into_inner().content)
    }
}

/// Blob bytes kept between calls, so a bundle a run reads again is not fetched again.
/// Bounded in entries and bytes, least recently used first out, and keyed by
/// `(org, sha256)`: one org's bytes are never served to another, whatever the digest.
pub struct BlobCache {
    max_entries: usize,
    max_bytes: usize,
    inner: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    /// Least recently used first.
    order: VecDeque<(String, String)>,
    blobs: HashMap<(String, String), Arc<Vec<u8>>>,
    bytes: usize,
}

impl BlobCache {
    #[must_use]
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            max_entries,
            max_bytes,
            inner: Mutex::new(CacheState::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, CacheState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get(&self, org_id: &str, sha256: &str) -> Option<Arc<Vec<u8>>> {
        let key = (org_id.to_owned(), sha256.to_owned());
        let mut state = self.lock();
        let blob = state.blobs.get(&key).cloned()?;
        if let Some(position) = state.order.iter().position(|entry| *entry == key) {
            if let Some(entry) = state.order.remove(position) {
                state.order.push_back(entry);
            }
        }
        Some(blob)
    }

    fn put(&self, org_id: &str, sha256: &str, bytes: Arc<Vec<u8>>) {
        if bytes.len() > self.max_bytes {
            return;
        }
        let key = (org_id.to_owned(), sha256.to_owned());
        let mut state = self.lock();
        if state.blobs.contains_key(&key) {
            return;
        }
        state.bytes += bytes.len();
        state.blobs.insert(key.clone(), bytes);
        state.order.push_back(key);
        while state.order.len() > self.max_entries || state.bytes > self.max_bytes {
            let Some(oldest) = state.order.pop_front() else {
                break;
            };
            if let Some(evicted) = state.blobs.remove(&oldest) {
                state.bytes -= evicted.len();
            }
        }
    }
}

/// The bytes of one file: from the cache when this org already read them, otherwise
/// from the source, checked against the size and digest the manifest gives.
async fn blob_for(
    source: &dyn SkillBundleSource,
    cache: &BlobCache,
    org_id: &str,
    mount: &SkillMount,
    file: &SkillBundleFile,
) -> Result<Arc<Vec<u8>>, Status> {
    if let Some(bytes) = cache.get(org_id, &file.sha256) {
        return Ok(bytes);
    }
    let bytes = Arc::new(source.blob(org_id, &mount.skill_id, &file.sha256).await?);
    let size_matches = u64::try_from(file.size_bytes).ok() == u64::try_from(bytes.len()).ok();
    if !size_matches || sha256_hex(&bytes) != file.sha256 {
        return Err(Status::data_loss("skill_mounts_blob_mismatch"));
    }
    cache.put(org_id, &file.sha256, Arc::clone(&bytes));
    Ok(bytes)
}

/// One call's bundle tree: a directory holding each mounted skill's files. Removed
/// when it is dropped, whichever path the call took.
#[derive(Debug)]
pub struct SkillTree {
    root: PathBuf,
}

impl SkillTree {
    fn create(base: &Path) -> std::io::Result<Self> {
        let root = base.join(format!("{TREE_PREFIX}-{}", mp_ids::new_ulid()));
        std::fs::create_dir(&root)?;
        Ok(Self { root })
    }

    /// The host directory bound read-only at [`crate::code_interpreter::SKILLS_MOUNT`].
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for SkillTree {
    fn drop(&mut self) {
        remove_tree(&self.root);
    }
}

/// Removes a tree in whatever modes its directories were left in. A directory is made
/// writable before its entries go, since a read-only directory cannot lose an entry.
/// A missing tree is the normal case and is not an error.
fn remove_tree(root: &Path) {
    make_writable(root);
    if let Err(error) = std::fs::remove_dir_all(root) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, tree = %root.display(), "skill bundle cleanup failed");
        }
    }
}

/// Walks without following links (`symlink_metadata`), so a link is never chmod-ed
/// through into a host directory this module did not create.
fn make_writable(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if !metadata.is_dir() {
        return;
    }
    let _ = set_mode(path, 0o755);
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            make_writable(&entry.path());
        }
    }
}

/// Creates the directories between a mount directory and a file, and records each one
/// so it is locked with the rest.
fn ensure_parents(
    mount_dir: &Path,
    target: &Path,
    directories: &mut Vec<PathBuf>,
) -> Result<(), Status> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    let relative = parent
        .strip_prefix(mount_dir)
        .map_err(|_| Status::internal("skill_mounts_path"))?;
    let mut current = mount_dir.to_path_buf();
    for part in relative.components() {
        current.push(part);
        if !current.is_dir() {
            std::fs::create_dir(&current).map_err(io_status)?;
            directories.push(current.clone());
        }
    }
    Ok(())
}

/// Fetches, checks and writes the bundles of `mounts` into a new tree under `base`.
///
/// Nothing is written for a file until its size and digest check out. The tree is
/// the only holder of its directory until it is returned, so any error removes it.
///
/// # Errors
/// `DataLoss` when a manifest or blob is not the one the gateway named or does not
/// match its digest, and the source's own status when a read fails.
pub async fn materialise(
    source: &dyn SkillBundleSource,
    cache: &BlobCache,
    org_id: &str,
    mounts: &[SkillMount],
    base: &Path,
) -> Result<SkillTree, Status> {
    let tree = SkillTree::create(base).map_err(|error| {
        tracing::warn!(%error, "skill bundle tree could not be created");
        Status::internal("skill_mounts_tree_unavailable")
    })?;
    // Locked last and deepest first, so a directory turns read-only only once its
    // contents exist.
    let mut directories = vec![tree.root.clone()];
    for mount in mounts {
        let manifest = source.manifest(org_id, &mount.skill_id).await?;
        check_manifest(&manifest, mount)?;
        let mount_dir = tree.root.join(&mount.name);
        std::fs::create_dir(&mount_dir).map_err(io_status)?;
        directories.push(mount_dir.clone());
        for file in &manifest.files {
            let bytes = blob_for(source, cache, org_id, mount, file).await?;
            let target = mount_dir.join(&file.path);
            ensure_parents(&mount_dir, &target, &mut directories)?;
            std::fs::write(&target, bytes.as_slice()).map_err(io_status)?;
            set_mode(&target, if file.executable { 0o555 } else { 0o444 }).map_err(io_status)?;
        }
    }
    for directory in directories.iter().rev() {
        set_mode(directory, 0o555).map_err(io_status)?;
    }
    Ok(tree)
}

/// The trees the calls of each run hold, keyed by `(org, run)`.
#[derive(Default)]
pub struct TreeRegistry {
    held: Mutex<HashMap<(String, String), Held>>,
}

struct Held {
    mounts: Vec<SkillMount>,
    tree: Arc<SkillTree>,
    holders: usize,
}

/// One call's hold on its run's tree. The hold is released when the guard drops, and
/// the last hold to go removes the tree.
pub struct MountGuard {
    key: (String, String),
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        global().release(&self.key);
    }
}

impl TreeRegistry {
    fn lock(&self) -> MutexGuard<'_, HashMap<(String, String), Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Holds `tree` for the run, or joins the run's tree when the calls name the same
    /// mounts. Parallel calls of one run may share a tree, as they share attachments.
    fn hold(
        &self,
        org_id: &str,
        run_id: &str,
        mounts: &[SkillMount],
        tree: SkillTree,
    ) -> Result<MountGuard, Status> {
        let key = (org_id.to_owned(), run_id.to_owned());
        let mut held = self.lock();
        match held.entry(key.clone()) {
            Entry::Occupied(mut occupied) => {
                if occupied.get().mounts.as_slice() != mounts {
                    return Err(Status::aborted("skill_mounts_in_use"));
                }
                occupied.get_mut().holders += 1;
            }
            Entry::Vacant(vacant) => {
                vacant.insert(Held {
                    mounts: mounts.to_vec(),
                    tree: Arc::new(tree),
                    holders: 1,
                });
            }
        }
        Ok(MountGuard { key })
    }

    fn release(&self, key: &(String, String)) {
        let mut held = self.lock();
        let last = held.get_mut(key).is_some_and(|entry| {
            entry.holders -= 1;
            entry.holders == 0
        });
        let removed = if last { held.remove(key) } else { None };
        // Dropped outside the lock: removing the tree touches the filesystem.
        drop(held);
        drop(removed);
    }

    /// The run's tree, for as long as a call of the run holds it.
    #[must_use]
    pub fn get(&self, org_id: &str, run_id: &str) -> Option<Arc<SkillTree>> {
        self.lock()
            .get(&(org_id.to_owned(), run_id.to_owned()))
            .map(|held| Arc::clone(&held.tree))
    }
}

static REGISTRY: OnceLock<TreeRegistry> = OnceLock::new();
static BLOBS: OnceLock<BlobCache> = OnceLock::new();

/// The process-wide registry: the gRPC handler holds into it, and the `code_interpreter`
/// arm reads from it.
pub fn global() -> &'static TreeRegistry {
    REGISTRY.get_or_init(TreeRegistry::default)
}

fn blob_cache() -> &'static BlobCache {
    BLOBS.get_or_init(|| BlobCache::new(BLOB_CACHE_ENTRIES, BLOB_CACHE_BYTES))
}

/// Judges, reads and holds a step's bundle mounts for the length of the step.
///
/// `None` when the step names none, which is every step today. The returned guard
/// keeps the run's tree in place until it drops.
///
/// # Errors
/// A [`MountRefusal`] as a status, `failed_precondition` when mounts are named but no
/// bundle source is configured, `aborted` when another call of the run holds different
/// mounts, and the reads' own statuses.
pub async fn stage_for_step(
    org_id: &str,
    run_id: &str,
    tool_name: &str,
    mounts: &[SkillMount],
    zdr_or_waived: bool,
    source: Option<&dyn SkillBundleSource>,
) -> Result<Option<MountGuard>, Status> {
    if mounts.is_empty() {
        return Ok(None);
    }
    validate(mounts, tool_name, zdr_or_waived).map_err(refusal_status)?;
    let source = source.ok_or_else(|| Status::failed_precondition("skill_mounts_unavailable"))?;
    let tree = materialise(source, blob_cache(), org_id, mounts, &std::env::temp_dir()).await?;
    global().hold(org_id, run_id, mounts, tree).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "org-a";

    fn sha(bytes: &[u8]) -> String {
        sha256_hex(bytes)
    }

    fn file(path: &str, body: &[u8], executable: bool) -> SkillBundleFile {
        SkillBundleFile {
            path: path.to_owned(),
            sha256: sha(body),
            size_bytes: i64::try_from(body.len()).unwrap(),
            executable,
            binary: false,
        }
    }

    /// A manifest whose digest is the one capability-core would compute for `files`.
    fn manifest(skill_id: &str, files: Vec<SkillBundleFile>) -> SkillBundleManifest {
        SkillBundleManifest {
            skill_id: skill_id.to_owned(),
            manifest_sha256: manifest_digest(&files),
            files,
        }
    }

    fn mount_for(manifest: &SkillBundleManifest, name: &str) -> SkillMount {
        SkillMount {
            skill_id: manifest.skill_id.clone(),
            name: name.to_owned(),
            manifest_sha256: manifest.manifest_sha256.clone(),
        }
    }

    /// Serves manifests and blobs from memory. `tamper` replaces one blob's bytes.
    #[derive(Default)]
    struct FakeSource {
        manifests: HashMap<String, SkillBundleManifest>,
        blobs: HashMap<String, Vec<u8>>,
        tamper: Option<String>,
    }

    #[tonic::async_trait]
    impl SkillBundleSource for FakeSource {
        async fn manifest(
            &self,
            _org_id: &str,
            skill_id: &str,
        ) -> Result<SkillBundleManifest, Status> {
            self.manifests
                .get(skill_id)
                .cloned()
                .ok_or_else(|| Status::not_found("no bundle"))
        }

        async fn blob(
            &self,
            _org_id: &str,
            _skill_id: &str,
            sha256: &str,
        ) -> Result<Vec<u8>, Status> {
            let bytes = self
                .blobs
                .get(sha256)
                .cloned()
                .ok_or_else(|| Status::not_found("no blob"))?;
            if self.tamper.as_deref() == Some(sha256) {
                return Ok(b"tampered".to_vec());
            }
            Ok(bytes)
        }
    }

    fn scratch_base(label: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("{label}-{}", mp_ids::new_ulid()));
        std::fs::create_dir(&base).unwrap();
        base
    }

    fn source_with(manifest: &SkillBundleManifest, bodies: &[(&str, &[u8])]) -> FakeSource {
        let mut source = FakeSource::default();
        source
            .manifests
            .insert(manifest.skill_id.clone(), manifest.clone());
        for (_, body) in bodies {
            source.blobs.insert(sha(body), body.to_vec());
        }
        source
    }

    #[test]
    fn names_are_reserved_and_checked() {
        let mount = |name: &str| SkillMount {
            skill_id: "skill-1".to_owned(),
            name: name.to_owned(),
            manifest_sha256: "a".repeat(64),
        };
        let refused = |name: &str| validate(&[mount(name)], "code_interpreter", false);
        assert_eq!(refused("skills"), Err(MountRefusal::NameReserved));
        assert_eq!(refused("attachments"), Err(MountRefusal::NameReserved));
        assert_eq!(refused("Deploy"), Err(MountRefusal::NameInvalid));
        assert_eq!(refused("../escape"), Err(MountRefusal::NameInvalid));
        assert_eq!(refused(""), Err(MountRefusal::NameInvalid));
        assert_eq!(refused("docx"), Ok(()));
        let two = [mount("docx"), mount("docx")];
        assert_eq!(
            validate(&two, "code_interpreter", false),
            Err(MountRefusal::NameDuplicate)
        );
    }

    #[test]
    fn zero_retention_other_tools_and_extra_mounts_are_refused() {
        let mount = SkillMount {
            skill_id: "skill-1".to_owned(),
            name: "docx".to_owned(),
            manifest_sha256: "b".repeat(64),
        };
        assert_eq!(
            validate(std::slice::from_ref(&mount), "code_interpreter", true),
            Err(MountRefusal::ZeroRetention)
        );
        assert_eq!(
            validate(std::slice::from_ref(&mount), "shell", false),
            Err(MountRefusal::UnsupportedTool)
        );
        let four = vec![mount.clone(); MAX_SKILL_MOUNTS + 1];
        assert_eq!(
            validate(&four, "code_interpreter", false),
            Err(MountRefusal::TooMany)
        );
        assert_eq!(validate(&[], "shell", true), Ok(()));
    }

    #[tokio::test]
    async fn a_blob_with_a_wrong_digest_is_refused() {
        let bundle = manifest("skill-1", vec![file("SKILL.md", b"hello", false)]);
        let mut source = source_with(&bundle, &[("SKILL.md", b"hello")]);
        source.tamper = Some(sha(b"hello"));
        let base = scratch_base("skill-mount-digest");
        let refused = materialise(
            &source,
            &BlobCache::new(4, 1 << 20),
            ORG,
            &[mount_for(&bundle, "docx")],
            &base,
        )
        .await
        .expect_err("a blob that does not hash to its digest must be refused");
        assert_eq!(refused.code(), Code::DataLoss);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn a_manifest_whose_rows_do_not_match_its_digest_is_refused() {
        let mut bundle = manifest("skill-1", vec![file("SKILL.md", b"hello", false)]);
        bundle.files[0].executable = true;
        let source = source_with(&bundle, &[("SKILL.md", b"hello")]);
        let base = scratch_base("skill-mount-manifest");
        let refused = materialise(
            &source,
            &BlobCache::new(4, 1 << 20),
            ORG,
            &[mount_for(&bundle, "docx")],
            &base,
        )
        .await
        .expect_err("rows that do not produce the digest must be refused");
        assert_eq!(refused.code(), Code::DataLoss);
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[tokio::test]
    async fn mount_dir_is_removed_on_every_path() {
        let good = manifest("skill-1", vec![file("SKILL.md", b"use me", false)]);
        let bad = manifest("skill-2", vec![file("SKILL.md", b"other", false)]);
        let mut source = source_with(&good, &[("SKILL.md", b"use me")]);
        source.manifests.insert(bad.skill_id.clone(), bad.clone());
        source.blobs.insert(sha(b"other"), b"other".to_vec());
        source.tamper = Some(sha(b"other"));
        let cache = BlobCache::new(4, 1 << 20);

        let base = scratch_base("skill-mount-cleanup");
        let tree = materialise(&source, &cache, ORG, &[mount_for(&good, "docx")], &base)
            .await
            .expect("a good bundle materialises");
        assert!(tree.path().join("docx").join("SKILL.md").is_file());
        let root = tree.path().to_path_buf();
        drop(tree);
        assert!(!root.exists(), "the tree is removed when it is dropped");

        let refused = materialise(
            &source,
            &cache,
            ORG,
            &[mount_for(&good, "docx"), mount_for(&bad, "other")],
            &base,
        )
        .await;
        assert!(refused.is_err());
        let leftovers: Vec<_> = std::fs::read_dir(&base).unwrap().collect();
        assert!(leftovers.is_empty(), "a refused call leaves no tree behind");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn lru_never_serves_another_orgs_blob() {
        let cache = BlobCache::new(4, 1 << 20);
        let digest = sha(b"shared bytes");
        cache.put("org-a", &digest, Arc::new(b"shared bytes".to_vec()));
        assert!(cache.get("org-a", &digest).is_some());
        assert!(cache.get("org-b", &digest).is_none());
    }

    #[test]
    fn lru_is_bounded_and_evicts_the_least_recently_used() {
        let cache = BlobCache::new(2, 1 << 20);
        let bodies: [&[u8]; 3] = [b"one", b"two", b"three"];
        for body in bodies {
            cache.put("org-a", &sha(body), Arc::new(body.to_vec()));
            if body == b"two" {
                assert!(cache.get("org-a", &sha(b"one")).is_some());
            }
        }
        assert!(
            cache.get("org-a", &sha(b"one")).is_some(),
            "recently used stays"
        );
        assert!(
            cache.get("org-a", &sha(b"two")).is_none(),
            "least recently used goes"
        );
        assert!(cache.get("org-a", &sha(b"three")).is_some());
    }

    #[tokio::test]
    async fn a_skill_tree_is_read_only_and_names_its_scripts() {
        let bundle = manifest(
            "skill-1",
            vec![
                file("SKILL.md", b"instructions", false),
                file("scripts/run.py", b"print(1)\n", true),
            ],
        );
        let source = source_with(
            &bundle,
            &[
                ("SKILL.md", b"instructions"),
                ("scripts/run.py", b"print(1)\n"),
            ],
        );
        let base = scratch_base("skill-mount-modes");
        let tree = materialise(
            &source,
            &BlobCache::new(4, 1 << 20),
            ORG,
            &[mount_for(&bundle, "docx")],
            &base,
        )
        .await
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |path: PathBuf| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(tree.path().join("docx")), 0o555);
            assert_eq!(mode(tree.path().join("docx").join("SKILL.md")), 0o444);
            assert_eq!(mode(tree.path().join("docx").join("scripts")), 0o555);
            assert_eq!(
                mode(tree.path().join("docx").join("scripts").join("run.py")),
                0o555
            );
        }
        drop(tree);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
