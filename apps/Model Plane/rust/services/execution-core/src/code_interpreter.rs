//! `code_interpreter` — REAL code execution in a hermetic per-call workspace.
//!
//! # Why this exists next to `shell`
//!
//! `shell` runs an arbitrary host command under a READ-ONLY policy. That makes
//! it both too weak (a program that cannot write a file cannot produce a
//! spreadsheet, a chart, or a PDF) and too dangerous to run unattended, so it is
//! in [`crate::permission::is_risky_tool`] and pauses for a human under the
//! `ask` posture.
//!
//! `code_interpreter` is the opposite trade: it is allowed to WRITE, but only
//! inside a directory this module created for that one call, with
//!
//! * the rest of the filesystem read-only ([`MpSandboxPolicy::WorkspaceWrite`]),
//! * no network at all ([`MpNetworkPolicy::Disabled`]),
//! * a wall-clock timeout that kills the child (`EXECUTION_CODE_TIMEOUT_SECS`),
//! * secret-scrubbed, size-capped stdout/stderr (see [`crate::executor`]),
//! * and the workspace deleted when the call returns, on every path.
//!
//! Because that shape is hermetic, the tool binds to the LOW-risk
//! `cap.command.sandbox` capability and is deliberately NOT in the risky-tool
//! list — its name contains none of those substrings by design. `shell` keeps
//! `cap.command.shell` and its human gate.
//!
//! # Contract
//!
//! Input JSON:
//! `{"language":"python"|"sh","code":"…","files_in":[{"name":"data.csv","content_b64":"…"}]}`
//! (`files_in` optional, `language` defaults to `python`).
//!
//! Files the person attached to the turn are NOT part of this input: the model
//! cannot add or change them. They arrive from the request that carries the step
//! ([`crate::run_inputs`]) and appear read-only under `attachments/`.
//!
//! Skill bundles the turn's matched skills carry are bound read-only at `/skills` when the
//! call holds them ([`crate::skill_mounts`]); the model names none of them.
//!
//! Output JSON:
//! `{"stdout":"…","stderr":"…","exit_code":0,"files":[{"name","mime","bytes","content_b64"}]}`
//! where an over-cap file is still listed, with `"truncated":true` and an empty
//! `content_b64`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, STANDARD};
use base64::engine::DecodePaddingMode;
use base64::Engine;
use mp_contracts::attachments::{validate_name, ATTACHMENTS_DIR};
use sha2::{Digest, Sha256};

use crate::executor;
use crate::policy::{MpNetworkPolicy, MpSandboxPolicy};
use crate::run_inputs::InputSet;

/// Structural checks of the Office files this module returns. Its own file so the
/// verifier's sandboxed second workspace stays separate from the program's.
#[path = "office_verify.rs"]
mod office_verify;

/// Directory-name prefix for per-call workspaces. Also the marker cleanup and
/// tests match on.
const WORKSPACE_PREFIX: &str = "verevon-code";

/// How many names to try before giving up on allocating a workspace. A
/// collision needs a *fresh* name (see [`Workspace::create`]), and the
/// counter+clock name is unique in practice, so a handful of tries is plenty.
const WORKSPACE_CREATE_ATTEMPTS: u32 = 8;

/// The interpreter this tool invokes. Public so the health probe
/// ([`crate::health_attest`]) measures the SAME binary the tool runs — a probe
/// against a different interpreter would attest health this tool does not have.
pub const PYTHON_PROGRAM: &str = "python3";

/// Workspace subdirectory for the program's temporary files (`TMPDIR`). Inside
/// the workspace because the sandbox's `/tmp` is read-only; a SUBDIRECTORY so
/// stray tempfiles are not mistaken for program output.
pub(crate) const TEMP_SUBDIR: &str = ".tmp";

/// Workspace subdirectory for `matplotlib`'s config + font cache
/// (`MPLCONFIGDIR`). `matplotlib` refuses to start without a writable config
/// directory, and its `fontlist-*.json` must not land next to the real output.
pub(crate) const MPL_CONFIG_SUBDIR: &str = ".mplconfig";

/// Used only when execution-core itself has no usable `PATH`; without one the
/// interpreter cannot be found at all.
const FALLBACK_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Per-file ceiling on returned content (8 `MiB`).
pub(crate) const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Ceiling on the total content returned by one call (16 `MiB`).
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;

/// Where a turn's skill bundles are bound read-only inside the sandbox (WS19-S8). The image
/// creates this directory as an empty mount point (WS19-S4), so the bind never has to create
/// one on the read-only root.
pub const SKILLS_MOUNT: &str = "/skills";
/// The same name without the leading slash. A `files_in` entry may not take it.
pub const SKILLS_DIR: &str = "skills";

/// Padding-tolerant decoder for `content_b64`. Model- and gateway-produced
/// base64 is routinely line-wrapped and occasionally unpadded; both are still
/// unambiguous, so decoding accepts either rather than rejecting a well-formed
/// payload over cosmetics. Encoding always emits canonical padded base64.
static B64_TOLERANT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The language a call runs in. `python` is the default because that is what the
/// runtime image is provisioned for (`pandas`, `openpyxl`, `matplotlib`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    Sh,
}

impl Language {
    /// Parse the wire value. An empty/absent value is `python`; anything not
    /// understood is an explicit error rather than a silent fallback, so a
    /// caller asking for `bash` learns that it gets POSIX `sh` semantics instead
    /// of quietly having its bashisms run under `dash`.
    fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "python" | "python3" | "py" => Ok(Self::Python),
            "sh" => Ok(Self::Sh),
            other => Err(format!(
                "unsupported code_interpreter language '{other}': use 'python' or 'sh'"
            )),
        }
    }

    /// The interpreter binary invoked for this language.
    const fn program(self) -> &'static str {
        match self {
            Self::Python => PYTHON_PROGRAM,
            Self::Sh => "sh",
        }
    }

    /// Name of the generated program file staged in the workspace.
    pub(crate) const fn program_file(self) -> &'static str {
        match self {
            Self::Python => "_verevon_main.py",
            Self::Sh => "_verevon_main.sh",
        }
    }
}

/// One decoded `files_in` entry, staged flat in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A validated `code_interpreter` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRequest {
    pub language: Language,
    pub code: String,
    pub files_in: Vec<InputFile>,
    /// The turn's attached files, staged read-only under `attachments/`. Never
    /// parsed from the tool input: the caller sets it from the request.
    pub ro_inputs: Option<Arc<InputSet>>,
}

/// One file produced by the program, as returned to the model.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct OutputFile {
    name: String,
    mime: &'static str,
    /// The file's REAL size, even when the content is withheld.
    bytes: u64,
    content_b64: String,
    /// Serialized ONLY when the content was withheld (`Some(true)`), so the
    /// model can tell the user "produced, too large to return" instead of
    /// seeing nothing. Absent on the normal path.
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct CodeResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
    files: Vec<OutputFile>,
    /// Advisory structural checks of the returned Office files. Absent unless
    /// `CODE_INTERPRETER_OFFICE_VERIFY` is on, so the result is unchanged by default.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    verification: Vec<mp_contracts::office_verification::OfficeVerification>,
    /// Office files the verifier did not check, so the model can say so.
    #[serde(skip_serializing_if = "is_zero")]
    verification_unchecked: usize,
    /// The verifier's rendered copies of checked files, for the viewer only; the delivered
    /// files stay the sandbox's bytes. Empty unless the render layer ran.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    previews: Vec<office_verify::FilePreview>,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// Parse and validate a `code_interpreter` tool input.
///
/// # Errors
/// Returns a caller-facing message when the JSON is malformed, `code` is empty,
/// `language` is not supported, or a `files_in` entry is unusable — an unsafe
/// name (absolute, containing `..`, or containing a path separator) is REJECTED,
/// never sanitized, so the caller learns its filename was wrong instead of
/// silently getting a different one.
pub fn parse_request(tool_input: &str) -> Result<CodeRequest, String> {
    #[derive(serde::Deserialize)]
    struct RawRequest {
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        code: String,
        #[serde(default)]
        files_in: Vec<RawFile>,
    }
    #[derive(serde::Deserialize)]
    struct RawFile {
        #[serde(default)]
        name: String,
        #[serde(default)]
        content_b64: String,
    }

    let raw: RawRequest = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid code_interpreter input: {error}"))?;
    if raw.code.trim().is_empty() {
        return Err("code_interpreter requires a non-empty 'code' program".to_owned());
    }
    let language = Language::parse(raw.language.as_deref().unwrap_or_default())?;

    let mut files_in: Vec<InputFile> = Vec::with_capacity(raw.files_in.len());
    for file in raw.files_in {
        let name = validate_input_name(&file.name, language)?;
        if files_in.iter().any(|staged| staged.name == name) {
            return Err(format!(
                "duplicate files_in name '{name}': each input file needs a distinct name"
            ));
        }
        let bytes = decode_b64(&file.content_b64)
            .map_err(|error| format!("files_in '{name}' has invalid content_b64: {error}"))?;
        files_in.push(InputFile { name, bytes });
    }
    Ok(CodeRequest {
        language,
        code: raw.code,
        files_in,
        ro_inputs: None,
    })
}

/// Validate one `files_in` name. Input files are written FLAT into the
/// workspace, so the only acceptable value is a bare filename: anything that
/// could address a second directory is an error, which keeps a name from
/// reaching outside the single writable root.
fn validate_input_name(raw: &str, language: Language) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("each files_in entry requires a non-empty 'name'".to_owned());
    }
    if name.starts_with('/') || name.starts_with('\\') || Path::new(name).is_absolute() {
        return Err(format!(
            "files_in name '{name}' must be a bare filename, not an absolute path"
        ));
    }
    if name.contains("..") {
        return Err(format!("files_in name '{name}' must not contain '..'"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(format!(
            "files_in name '{name}' must not contain a path separator; \
             input files are written flat into the workspace"
        ));
    }
    // Catches the residue (`.`, a trailing separator, a platform prefix) without
    // guessing at what the caller meant.
    if Path::new(name)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        != Some(name)
    {
        return Err(format!("files_in name '{name}' is not a plain filename"));
    }
    // The generated program and the runtime's scratch directories already occupy
    // these names; staging over them would fail with a confusing IO error, so say
    // what is wrong instead.
    if name == language.program_file()
        || name == TEMP_SUBDIR
        || name == MPL_CONFIG_SUBDIR
        || name.eq_ignore_ascii_case(ATTACHMENTS_DIR)
        || name.eq_ignore_ascii_case(SKILLS_DIR)
    {
        return Err(format!(
            "files_in name '{name}' is reserved by the sandbox workspace; rename the input file"
        ));
    }
    Ok(name.to_owned())
}

fn decode_b64(raw: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let compact: String = raw.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    B64_TOLERANT.decode(compact)
}

/// Map a filename extension to a content type. Anything unrecognised is
/// `application/octet-stream` — a wrong-but-confident type would make a
/// downstream consumer mis-render the bytes.
fn mime_for(name: &str) -> &'static str {
    let extension = Path::new(name)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "json" => "application/json",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// A per-call workspace directory whose lifetime is tied to this guard: `Drop`
/// removes it recursively.
///
/// Cleanup MUST NOT hang off the happy path. A validation error, a spawn
/// failure, a timeout, or a panic would each leak a directory — and every byte
/// of customer data the program wrote into it — for the lifetime of the
/// container, so the guard is the only cleanup mechanism and it covers every
/// return path by construction.
struct Workspace {
    path: PathBuf,
}

impl Workspace {
    fn create(run_id: &str, step_id: &str) -> std::io::Result<Self> {
        let base = std::env::temp_dir();
        for _ in 0..WORKSPACE_CREATE_ATTEMPTS {
            let candidate = base.join(workspace_name(run_id, step_id));
            match std::fs::create_dir(&candidate) {
                Ok(()) => return Ok(Self { path: candidate }),
                Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                    return Err(error)
                }
                // `create_dir`, never `create_dir_all`: an already-existing
                // directory must NOT be adopted, or this call would inherit
                // (and then return) another call's files. Take a fresh name.
                Err(_) => {}
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique code workspace",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    %error,
                    workspace = %self.path.display(),
                    "code workspace cleanup failed"
                );
            }
        }
    }
}

/// Build a unique workspace directory name from the run/step identity.
///
/// A process-local counter alone repeats after a restart (it resets to zero) and
/// the clock alone can repeat inside a single tick, so the name carries both —
/// unique without any RNG, and still traceable back to the step that made it.
fn workspace_name(run_id: &str, step_id: &str) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    format!(
        "{WORKSPACE_PREFIX}-{}-{}-{sequence}-{nanos}",
        path_slug(run_id),
        path_slug(step_id)
    )
}

/// Reduce an identifier to characters safe in a directory name. Run/step ids
/// arrive from the run context rather than from model input, but they are still
/// never spliced into a path unfiltered — a single `../` would move the
/// workspace, and with it the sandbox's writable root.
///
/// `pub(crate)` so `sandbox_lease.rs` can build the SAME kind of safe
/// directory name for a Space-scoped lease's persistent workspace, rather
/// than duplicating this exact logic a second time.
pub(crate) fn path_slug(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .take(48)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "none".to_owned()
    } else {
        cleaned
    }
}

/// The child-environment allowlist, exposed for S4.2's background process
/// host. Shared rather than re-derived precisely because it is a security
/// surface: execution-core's own environment holds the keys it uses to reach
/// the other planes, and a second hand-written allowlist is a second place to
/// forget that. A background process gets exactly what a `code_interpreter`
/// call gets; [`child_env`] carries the reasoning for every entry.
pub(crate) fn child_env_for(workspace: &Path) -> Vec<(String, String)> {
    child_env(workspace)
}

/// The program's ENTIRE environment — it inherits nothing.
///
/// execution-core's own environment carries the internal keys it uses to reach
/// the other planes (Data Plane, integration gateway, object storage, its own
/// service principal). Inheriting it would make `print(os.environ)` a
/// credential-exfiltration path in the one place that runs model-authored code,
/// and output scrubbing is no substitute — it cannot recognise every opaque
/// value. So the sandbox starts from an empty environment and gets exactly this
/// allowlist (`--clearenv` in the bwrap argv, `env_clear()` on the passthrough).
///
/// Everything here earns its place:
/// * `PATH` — how the interpreter (and bubblewrap itself) is found at all.
/// * `HOME` — the real one lives on the read-only rootfs, so anything writing to
///   `~` would fail; point it at the workspace.
/// * `TMPDIR`, `MPLCONFIGDIR` — SUBDIRECTORIES of the workspace, not the
///   workspace root. They must point somewhere writable: the sandbox's `/tmp` is
///   read-only (measured in the deployed container) and `matplotlib` refuses to
///   start without a writable config dir, so without these there are no charts at
///   all. And they must be subdirectories, because `matplotlib`'s font cache and
///   any tempfile the program makes would otherwise sit next to the real output
///   and be returned as if the program had produced them.
/// * `MPLBACKEND` — headless rendering; the sandbox has no display.
/// * `PYTHONDONTWRITEBYTECODE` — keeps `__pycache__` out of the workspace.
/// * `LANG` — passed through when set, so text I/O keeps the host's encoding.
fn child_env(workspace: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        ("PATH".to_owned(), inherited_or("PATH", FALLBACK_PATH)),
        ("HOME".to_owned(), path_value(workspace)),
        (
            "TMPDIR".to_owned(),
            path_value(&workspace.join(TEMP_SUBDIR)),
        ),
        (
            "MPLCONFIGDIR".to_owned(),
            path_value(&workspace.join(MPL_CONFIG_SUBDIR)),
        ),
        ("MPLBACKEND".to_owned(), "Agg".to_owned()),
        ("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned()),
    ];
    if let Some(lang) = std::env::var("LANG")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        env.push(("LANG".to_owned(), lang));
    }
    env
}

fn path_value(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Pass a variable through from execution-core's environment, falling back when
/// it is unset or blank. Used ONLY for `PATH`: without a usable one nothing can
/// be executed at all.
fn inherited_or(var: &str, fallback: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// Run a `code_interpreter` call: stage a workspace, execute the program in it,
/// return its output plus the files it produced, and delete the workspace.
///
/// `workspace_root` is `Some` only for a Space-scoped lease whose workspace
/// `sandbox_lease::ensure_hydrated_workspace` already hydrated — that
/// directory is REUSED across every call in the run rather than created and
/// dropped here; this function neither creates nor deletes it. `None` (every
/// non-Space call, unchanged from before this parameter existed) keeps the
/// original ephemeral `Workspace::create`/`Drop` lifecycle exactly as is.
///
/// # Errors
/// Returns a caller-facing message when the input is invalid (see
/// [`parse_request`]), the workspace or its files cannot be staged, or the
/// process cannot be spawned / exceeds its timeout. A non-zero exit is NOT an
/// error: it is reported as data in the result JSON.
pub async fn run(
    tool_input: &str,
    run_id: &str,
    step_id: &str,
    workspace_root: Option<&Path>,
    ro_inputs: Option<Arc<InputSet>>,
) -> Result<String, String> {
    run_staged(
        tool_input,
        run_id,
        step_id,
        workspace_root,
        ro_inputs,
        executor::code_exec_timeout(),
    )
    .await
}

/// [`run`] with the turn's skill bundles: `skills`, when present, is the host directory a
/// `crate::skill_mounts` tree was written to, bound read-only at [`SKILLS_MOUNT`] for the
/// call. `None` is exactly [`run`].
pub async fn run_with_skills(
    tool_input: &str,
    run_id: &str,
    step_id: &str,
    workspace_root: Option<&Path>,
    ro_inputs: Option<Arc<InputSet>>,
    skills: Option<&Path>,
) -> Result<String, String> {
    run_staged_with_skills(
        tool_input,
        run_id,
        step_id,
        workspace_root,
        ro_inputs,
        skills,
        executor::code_exec_timeout(),
    )
    .await
}

/// [`run`] with an injected deadline — the testable seam (env mutation is
/// forbidden in this crate, so tests pass the timeout instead of setting it).
#[cfg(test)]
async fn run_with_timeout(
    tool_input: &str,
    run_id: &str,
    step_id: &str,
    workspace_root: Option<&Path>,
    timeout: Duration,
) -> Result<String, String> {
    run_staged(tool_input, run_id, step_id, workspace_root, None, timeout).await
}

/// [`run_with_timeout`] with the turn's attached files.
async fn run_staged(
    tool_input: &str,
    run_id: &str,
    step_id: &str,
    workspace_root: Option<&Path>,
    ro_inputs: Option<Arc<InputSet>>,
    timeout: Duration,
) -> Result<String, String> {
    run_staged_with_skills(
        tool_input,
        run_id,
        step_id,
        workspace_root,
        ro_inputs,
        None,
        timeout,
    )
    .await
}

/// [`run_staged`] with the turn's skill bundles (WS19-S8), bound read-only at [`SKILLS_MOUNT`].
async fn run_staged_with_skills(
    tool_input: &str,
    run_id: &str,
    step_id: &str,
    workspace_root: Option<&Path>,
    ro_inputs: Option<Arc<InputSet>>,
    skills: Option<&Path>,
    timeout: Duration,
) -> Result<String, String> {
    let mut request = parse_request(tool_input)?;
    request.ro_inputs = ro_inputs;

    if let Some(root) = workspace_root {
        return run_in_root(&request, root, timeout, true, skills).await;
    }

    let workspace = Workspace::create(run_id, step_id)
        .map_err(|error| format!("code_interpreter could not create its workspace: {error}"))?;
    run_in_root(&request, workspace.path(), timeout, false, skills).await
    // `workspace` drops here — and on every `?` above — removing the directory.
}

/// Stage `request` into `root`, execute it sandboxed, and collect its output.
/// Shared by both the ephemeral (`persistent = false`, a fresh empty `root`
/// every call) and the Space-scoped persistent (`persistent = true`, `root`
/// may already hold files hydrated or left by an earlier call in the same
/// run) paths — `persistent` selects the two places that distinction
/// actually matters: scratch-directory creation tolerates `AlreadyExists`
/// instead of failing, and the returned `files` excludes anything that
/// already existed, byte-for-byte unchanged, before THIS call started (not
/// just this call's own staged inputs) — otherwise every call in a run would
/// re-report every file any earlier call, or hydrate, ever produced.
async fn run_in_root(
    request: &CodeRequest,
    root: &Path,
    timeout: Duration,
    persistent: bool,
    skills: Option<&Path>,
) -> Result<String, String> {
    let pre_call_baseline = if persistent {
        snapshot_file_hashes(root)
    } else {
        HashMap::new()
    };

    // The program is staged as a FILE rather than passed with `-c`: a file gives
    // real filenames and line numbers in tracebacks (which is what lets the
    // model fix its own code) and sidesteps the per-argument size limit on argv.
    // It lives inside the workspace because that is the only path the sandbox
    // can be sure the interpreter may read AND that gets cleaned up; it is
    // excluded from the returned files below so it never looks like output.
    let program_file = request.language.program_file();
    std::fs::write(root.join(program_file), request.code.as_bytes())
        .map_err(|error| format!("code_interpreter could not stage the program: {error}"))?;
    // Scratch directories the runtime needs writable (see `child_env`). They are
    // directories, and output collection only returns top-level regular files, so
    // whatever lands in them stays out of the tool result. `create_dir_all`
    // (persistent) rather than `create_dir` (ephemeral) because a persistent
    // root's SECOND call finds these already present from the first.
    for scratch in [TEMP_SUBDIR, MPL_CONFIG_SUBDIR] {
        let created = if persistent {
            std::fs::create_dir_all(root.join(scratch))
        } else {
            std::fs::create_dir(root.join(scratch))
        };
        created.map_err(|error| {
            format!("code_interpreter could not create its '{scratch}' directory: {error}")
        })?;
    }
    for file in &request.files_in {
        std::fs::write(root.join(&file.name), &file.bytes).map_err(|error| {
            format!(
                "code_interpreter could not stage files_in '{}': {error}",
                file.name
            )
        })?;
    }

    // The attached files, as a read-only directory inside the writable root. It
    // lives until this function returns, so the output collection below still
    // sees a workspace the program could not have changed them in.
    let staged_inputs = match &request.ro_inputs {
        Some(set) => StagedInputs::stage(root, set)?,
        None => None,
    };
    let mut ro_binds: Vec<(PathBuf, PathBuf)> = staged_inputs
        .iter()
        .map(|inputs| (inputs.dir.clone(), inputs.dir.clone()))
        .collect();
    // The turn's skill bundles, read-only at `/skills` (WS19-S8). Like the attached files they
    // are bound after the writable root, so a writable root can never shadow them.
    if let Some(skills) = skills {
        ro_binds.push((skills.to_path_buf(), PathBuf::from(SKILLS_MOUNT)));
    }

    // WorkspaceWrite over exactly one root, network off. The writable bind comes
    // before the read-only binds in the bwrap argv and after the read-only
    // rootfs, so it wins over the rootfs — which is what makes
    // `open("out.xlsx","wb")` work while the rest of the container stays
    // untouchable — and the `attachments/` bind then makes that one directory
    // read-only again.
    let policy = MpSandboxPolicy::WorkspaceWrite {
        writable_roots: vec![root.to_path_buf()],
        network: MpNetworkPolicy::Disabled,
    };
    let outcome = executor::execute_sandboxed_in_dir_with_ro_binds(
        &policy,
        request.language.program(),
        &[program_file.to_owned()],
        root,
        &child_env(root),
        &ro_binds,
        timeout,
    )
    .await
    .map_err(|error| format!("code_interpreter execution failed: {error}"))?;

    let mut staged: Vec<&str> = Vec::with_capacity(request.files_in.len() + 1);
    staged.push(program_file);
    staged.extend(request.files_in.iter().map(|file| file.name.as_str()));
    let files = collect_output_files(root, &staged, &pre_call_baseline);
    let office = office_verify::verify_outputs(&files).await;

    serde_json::to_string(&CodeResult {
        stdout: outcome.stdout,
        stderr: outcome.stderr,
        exit_code: outcome.exit_code,
        files,
        verification: office.reports,
        verification_unchecked: office.unchecked,
        previews: office.previews,
    })
    .map_err(|error| format!("code_interpreter could not encode its result: {error}"))
}

/// The turn's attached files, written into `<root>/attachments/` as read-only
/// files in a read-only directory. The directory is bind-mounted read-only by the
/// sandbox too, so the permission bits are the second lock, not the only one.
///
/// `Drop` makes the directory writable again and removes it. That has to happen
/// before the workspace itself is removed, or `remove_dir_all` fails on a
/// directory it may not write to and the customer's files outlive the call.
struct StagedInputs {
    dir: PathBuf,
}

impl StagedInputs {
    /// `None` when the set holds no files (nothing is created).
    fn stage(root: &Path, set: &InputSet) -> Result<Option<Self>, String> {
        if set.with_files(<[_]>::is_empty) {
            return Ok(None);
        }
        // The registry is not trusted with a path: every name is judged again
        // here, and refused, never sanitized, before anything is written.
        if !set.with_files(|files| files.iter().all(|file| validate_name(&file.name))) {
            return Err("an attached file has an unsafe name and was not staged".to_owned());
        }
        // The files could not be kept read-only on a host that runs the program
        // as a plain process, so they are not even written there.
        if !crate::sandbox::is_supported() {
            return Err(
                "attached files need the OS sandbox, which is unavailable; refusing to stage them"
                    .to_owned(),
            );
        }
        let dir = root.join(ATTACHMENTS_DIR);
        // Whatever a call killed mid-way, or a program in a persistent root, left at
        // the reserved name goes first: a stale entry would fail the create below.
        // A link there is unlinked, not followed (see `remove_staged_dir`).
        remove_staged_dir(&dir);
        std::fs::create_dir(&dir)
            .map_err(|error| format!("code_interpreter could not stage attached files: {error}"))?;
        // Created before the files are written so a failure part-way still cleans up.
        let staged = Self { dir };
        set.with_files(|files| {
            for file in files {
                let path = staged.dir.join(&file.name);
                std::fs::write(&path, &file.bytes)
                    .and_then(|()| set_mode(&path, 0o444))
                    .map_err(|error| {
                        format!("code_interpreter could not stage attached files: {error}")
                    })?;
            }
            Ok::<(), String>(())
        })?;
        set_mode(&staged.dir, 0o555)
            .map_err(|error| format!("code_interpreter could not stage attached files: {error}"))?;
        Ok(Some(staged))
    }
}

impl Drop for StagedInputs {
    fn drop(&mut self) {
        remove_staged_dir(&self.dir);
    }
}

/// Removes whatever sits at a staged-directory name, in whatever mode it was left
/// in. A missing entry is the normal case and is not an error.
///
/// The entry is looked at WITHOUT following links first. In a persistent Space
/// root, a program from an earlier call of the same run can plant `attachments`
/// as a symlink to a host directory, and `set_mode` is a chmod, which follows
/// links: chmod-ing by path would change the mode of that host directory as the
/// execution-core user. A link, or a file planted at the name, is unlinked and its
/// target is never touched. This is not race-free: a process running in the same
/// root can still swap the entry between the two calls below. Closing that needs
/// the directory opened with O_NOFOLLOW and chmod-ed through that handle, which
/// this crate does not do yet.
fn remove_staged_dir(dir: &Path) {
    let result = match std::fs::symlink_metadata(dir) {
        Err(error) => Err(error),
        Ok(metadata) if !metadata.is_dir() => std::fs::remove_file(dir),
        Ok(_) => {
            // A real directory this module staged: its entries are read-only, so the
            // directory itself must be writable before they can be removed. Best
            // effort: a chmod failure surfaces as the removal failure that follows.
            let _ = set_mode(dir, 0o755);
            std::fs::remove_dir_all(dir)
        }
    };
    if let Err(error) = result {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(%error, dir = %dir.display(), "attached files cleanup failed");
        }
    }
}

#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// There are no unix permission bits to set; staging is refused on such a host
/// before anything is written (see [`StagedInputs::stage`]).
#[cfg(not(unix))]
pub(crate) fn set_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Hashes every top-level regular file currently in `root`, for
/// [`run_in_root`]'s own "what already existed before this call" baseline in
/// the persistent-workspace path. Never errors: an unreadable entry is
/// simply absent from the baseline, which only makes `collect_output_files`
/// more likely to (correctly) report it as new/changed — the safe direction.
fn snapshot_file_hashes(root: &Path) -> HashMap<String, String> {
    let mut baseline = HashMap::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return baseline;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Ok(bytes) = std::fs::read(entry.path()) {
            baseline.insert(name, content_hash(&bytes));
        }
    }
    baseline
}

/// Collect the files the program produced: every top-level regular file that was
/// not staged by us, in name order, capped per file and in total. A file already
/// present in `pre_call_baseline` with the SAME content is excluded too — the
/// persistent-workspace path's "already reported in an earlier call" case;
/// `pre_call_baseline` is always empty for the ephemeral path, so this is a
/// no-op there.
fn collect_output_files(
    workspace: &Path,
    staged: &[&str],
    pre_call_baseline: &HashMap<String, String>,
) -> Vec<OutputFile> {
    let Ok(entries) = std::fs::read_dir(workspace) else {
        return Vec::new();
    };
    let mut found: Vec<(String, u64)> = Vec::new();
    for entry in entries.flatten() {
        // `DirEntry::file_type` does not follow symlinks, so a symlink the
        // program planted at, say, /etc/shadow is skipped instead of being read
        // out of the read-only host filesystem and handed to the model.
        // Directories are skipped for the same reason a tool result is flat.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            // A non-UTF-8 filename cannot be represented in the JSON contract.
            continue;
        };
        if staged.contains(&name.as_str()) {
            continue;
        }
        if let Some(prior_hash) = pre_call_baseline.get(&name) {
            if std::fs::read(entry.path()).is_ok_and(|bytes| &content_hash(&bytes) == prior_hash) {
                continue;
            }
        }
        let size = entry.metadata().map_or(0, |meta| meta.len());
        found.push((name, size));
    }
    // Deterministic order: the same program must produce the same tool result.
    found.sort_by(|left, right| left.0.cmp(&right.0));

    let mut files = Vec::with_capacity(found.len());
    let mut budget = MAX_TOTAL_BYTES;
    for (name, size) in found {
        let mime = mime_for(&name);
        // Over-cap files are still LISTED with their true size: the model has to
        // be able to say "it was produced but is too large to return" rather
        // than reporting that nothing came back.
        if size > MAX_FILE_BYTES || size > budget {
            files.push(OutputFile {
                name,
                mime,
                bytes: size,
                content_b64: String::new(),
                truncated: Some(true),
            });
            continue;
        }
        match std::fs::read(workspace.join(&name)) {
            Ok(bytes) => {
                budget = budget.saturating_sub(size);
                files.push(OutputFile {
                    name,
                    mime,
                    bytes: size,
                    content_b64: STANDARD.encode(&bytes),
                    truncated: None,
                });
            }
            Err(error) => {
                // Unreadable is reported the same way as too-large: listed, with
                // no content. Dropping the entry would tell the model the file
                // was never produced, which is worse than an honest gap.
                tracing::warn!(%error, file = %name, "produced file could not be read back");
                files.push(OutputFile {
                    name,
                    mime,
                    bytes: size,
                    content_b64: String::new(),
                    truncated: Some(true),
                });
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real end-to-end tests require both the interpreter and enforced
    /// local isolation. A machine without either one is a deliberately
    /// unavailable `code_interpreter` runtime, not a reason to test it through
    /// the forbidden host-process fallback.
    fn python3_available() -> bool {
        crate::sandbox::is_supported()
            && std::process::Command::new("python3")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }

    async fn run_python(code: &str, files_in: serde_json::Value) -> serde_json::Value {
        let input = serde_json::json!({
            "language": "python",
            "code": code,
            "files_in": files_in,
        })
        .to_string();
        let output = run_with_timeout(
            &input,
            "run-test",
            "step-test",
            None,
            Duration::from_secs(60),
        )
        .await
        .expect("code_interpreter call");
        serde_json::from_str(&output).expect("result is JSON")
    }

    /// Pins the WIRE SHAPE the model sees. The field names and the
    /// present-only-when-withheld `truncated` flag are a contract with the
    /// gateway, so a rename or a reordering has to break a test, not a caller.
    #[test]
    fn the_result_json_shape_is_stable() {
        let result = CodeResult {
            stdout: "done\n".to_owned(),
            stderr: String::new(),
            exit_code: 0,
            files: vec![
                OutputFile {
                    name: "out.csv".to_owned(),
                    mime: mime_for("out.csv"),
                    bytes: 2,
                    content_b64: STANDARD.encode(b"ok"),
                    truncated: None,
                },
                OutputFile {
                    name: "big.xlsx".to_owned(),
                    mime: mime_for("big.xlsx"),
                    bytes: 9_000_000,
                    content_b64: String::new(),
                    truncated: Some(true),
                },
            ],
            verification: Vec::new(),
            verification_unchecked: 0,
            previews: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&result).expect("serializes"),
            r#"{"stdout":"done\n","stderr":"","exit_code":0,"files":[{"name":"out.csv","mime":"text/csv","bytes":2,"content_b64":"b2s="},{"name":"big.xlsx","mime":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","bytes":9000000,"content_b64":"","truncated":true}]}"#
        );
    }

    #[test]
    fn empty_code_is_rejected() {
        let error = parse_request(r#"{"language":"python","code":"   \n"}"#)
            .expect_err("blank program must be rejected");
        assert!(error.contains("non-empty"), "error: {error}");
        let error = parse_request(r#"{"language":"python"}"#)
            .expect_err("missing program must be rejected");
        assert!(error.contains("non-empty"), "error: {error}");
    }

    #[test]
    fn language_defaults_to_python_and_unknown_languages_fail_loudly() {
        let request = parse_request(r#"{"code":"print(1)"}"#).expect("language is optional");
        assert_eq!(request.language, Language::Python);
        assert_eq!(request.language.program(), "python3");
        assert_eq!(
            parse_request(r#"{"language":"sh","code":"echo hi"}"#)
                .expect("sh is supported")
                .language,
            Language::Sh
        );
        // `bash` is NOT silently aliased to `sh` — dash would run the bashisms.
        for bad in ["bash", "ruby", "node"] {
            let input = format!(r#"{{"language":"{bad}","code":"x"}}"#);
            let error = parse_request(&input).expect_err("unknown language must fail");
            assert!(error.contains(bad), "error: {error}");
            assert!(error.contains("'python' or 'sh'"), "error: {error}");
        }
    }

    /// Unsafe names must be REJECTED with the reason, never quietly rewritten
    /// into something safe — the caller has to learn its filename was wrong.
    #[test]
    fn unsafe_files_in_names_are_rejected_never_sanitized() {
        let cases = [
            ("../escape.csv", "'..'"),
            ("..", "'..'"),
            ("data/../escape.csv", "'..'"),
            ("/etc/passwd", "absolute path"),
            ("sub/dir.csv", "path separator"),
            ("sub\\dir.csv", "path separator"),
            (".", "plain filename"),
            ("   ", "non-empty"),
        ];
        for (name, expected) in cases {
            let input = serde_json::json!({
                "code": "print(1)",
                "files_in": [{ "name": name, "content_b64": "" }],
            })
            .to_string();
            let error = parse_request(&input).expect_err(&format!("'{name}' must be rejected"));
            assert!(
                error.contains(expected),
                "'{name}' rejected with the wrong reason: {error}"
            );
        }
    }

    #[test]
    fn duplicate_and_reserved_files_in_names_are_rejected() {
        let duplicate = serde_json::json!({
            "code": "print(1)",
            "files_in": [
                { "name": "data.csv", "content_b64": "" },
                { "name": "data.csv", "content_b64": "" },
            ],
        })
        .to_string();
        let error = parse_request(&duplicate).expect_err("duplicate names must be rejected");
        assert!(error.contains("duplicate"), "error: {error}");

        let reserved = serde_json::json!({
            "code": "print(1)",
            "files_in": [{ "name": "_verevon_main.py", "content_b64": "" }],
        })
        .to_string();
        let error = parse_request(&reserved).expect_err("the program name is reserved");
        assert!(error.contains("reserved"), "error: {error}");
    }

    #[test]
    fn content_b64_is_decoded_tolerantly_but_garbage_is_rejected() {
        // Padded, unpadded and line-wrapped payloads all decode to "hi there".
        for encoded in ["aGkgdGhlcmU=", "aGkgdGhlcmU", "aGkgdGhl\ncmU="] {
            let input = serde_json::json!({
                "code": "print(1)",
                "files_in": [{ "name": "a.txt", "content_b64": encoded }],
            })
            .to_string();
            let request = parse_request(&input).expect("valid base64");
            assert_eq!(request.files_in[0].bytes, b"hi there");
        }
        let input = serde_json::json!({
            "code": "print(1)",
            "files_in": [{ "name": "a.txt", "content_b64": "not base64 !!!" }],
        })
        .to_string();
        let error = parse_request(&input).expect_err("garbage base64 must be rejected");
        assert!(error.contains("invalid content_b64"), "error: {error}");
    }

    #[test]
    fn mime_is_inferred_from_the_extension() {
        assert_eq!(
            mime_for("report.xlsx"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(
            mime_for("letter.DOCX"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert_eq!(
            mime_for("deck.pptx"),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        );
        assert_eq!(mime_for("invoice.pdf"), "application/pdf");
        assert_eq!(mime_for("chart.png"), "image/png");
        assert_eq!(mime_for("photo.jpeg"), "image/jpeg");
        assert_eq!(mime_for("photo.jpg"), "image/jpeg");
        assert_eq!(mime_for("logo.svg"), "image/svg+xml");
        assert_eq!(mime_for("rows.csv"), "text/csv");
        assert_eq!(mime_for("data.json"), "application/json");
        assert_eq!(mime_for("notes.txt"), "text/plain");
        assert_eq!(mime_for("readme.md"), "text/markdown");
        assert_eq!(mime_for("page.html"), "text/html");
        assert_eq!(mime_for("page.htm"), "text/html");
        assert_eq!(mime_for("model.bin"), "application/octet-stream");
        assert_eq!(mime_for("no-extension"), "application/octet-stream");
    }

    #[test]
    fn workspace_names_are_unique_and_path_safe() {
        let first = workspace_name("run/../evil", "step 1");
        let second = workspace_name("run/../evil", "step 1");
        assert_ne!(first, second, "two calls must not share a workspace");
        for name in [&first, &second] {
            assert!(name.starts_with(WORKSPACE_PREFIX));
            assert!(!name.contains('/'), "name: {name}");
            assert!(!name.contains(".."), "name: {name}");
        }
    }

    /// The guard is the ONLY cleanup mechanism, so prove it works without
    /// needing an interpreter on the host.
    #[test]
    fn the_workspace_guard_removes_its_directory_on_drop() {
        let workspace = Workspace::create("guard-run", "guard-step").expect("create workspace");
        let path = workspace.path().to_path_buf();
        std::fs::write(path.join("scratch.txt"), b"data").expect("write into workspace");
        assert!(path.is_dir());
        drop(workspace);
        assert!(
            !path.exists(),
            "workspace must be removed on drop: {}",
            path.display()
        );
    }

    #[tokio::test]
    async fn python_runs_and_returns_stdout() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let result = run_python("print('hello from code')", serde_json::json!([])).await;
        assert_eq!(
            result["stdout"].as_str().expect("stdout"),
            "hello from code\n"
        );
        assert_eq!(result["exit_code"].as_i64(), Some(0));
        assert_eq!(
            result["files"].as_array().map(Vec::len),
            Some(0),
            "the staged program must not be reported as output: {result}"
        );
    }

    /// THE regression guard for this module. execution-core's environment holds
    /// the internal keys it uses to reach the other planes; a sandbox that
    /// inherits it turns `print(os.environ)` into credential exfiltration, and
    /// output scrubbing cannot be relied on to recognise opaque values. So the
    /// child's environment must be exactly the allowlist and nothing else.
    #[tokio::test]
    async fn no_inherited_environment_reaches_the_sandboxed_code() {
        /// Exactly what `child_env` grants — nothing else may appear.
        const ALLOWED: &[&str] = &[
            "PATH",
            "HOME",
            "TMPDIR",
            "MPLCONFIGDIR",
            "MPLBACKEND",
            "PYTHONDONTWRITEBYTECODE",
            "LANG",
        ];
        /// Set by the runtime itself, NOT inherited, so they are tolerated rather
        /// than weakening the guard:
        /// * `LC_CTYPE` — `CPython`'s C-locale coercion (PEP 538) sets it in its
        ///   own process; `env -i python3 -c "import os; print(sorted(os.environ))"`
        ///   prints it under a completely empty environment.
        /// * `__CF_USER_TEXT_ENCODING` — Apple `CoreFoundation`, same story.
        /// * `PWD` — bubblewrap sets it after `--chdir`; observed on the deployed
        ///   Linux container, absent on the macOS passthrough path.
        const PLATFORM_SELF_SET: &[&str] = &["LC_CTYPE", "__CF_USER_TEXT_ENCODING", "PWD"];

        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        // A variable that exists in THIS process (so the assertion is meaningful)
        // and is not in the allowlist: if the child sees it, the clear failed.
        let parent_home = std::env::var("HOME").expect("the test runner has HOME set");
        let result = run_python(
            "import os\nprint(' '.join(sorted(os.environ)))\nprint(os.environ.get('HOME',''))",
            serde_json::json!([]),
        )
        .await;
        assert_eq!(result["exit_code"].as_i64(), Some(0));
        let stdout = result["stdout"].as_str().expect("stdout");
        let mut lines = stdout.lines();
        let names: Vec<&str> = lines
            .next()
            .expect("environment line")
            .split_whitespace()
            .collect();
        let child_home = lines.next().expect("HOME line").trim().to_owned();
        assert!(!names.is_empty(), "the child must report its environment");

        // Explicit credential-shaped check, in the terms an operator reasons in.
        for name in &names {
            let upper = name.to_ascii_uppercase();
            for marker in ["KEY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL"] {
                assert!(
                    !upper.contains(marker),
                    "credential-shaped variable '{name}' reached the sandbox"
                );
            }
        }
        // And the tighter guard: NOTHING outside the allowlist gets through, so a
        // future leak fails here even if its name looks innocent.
        for name in &names {
            assert!(
                ALLOWED.contains(name) || PLATFORM_SELF_SET.contains(name),
                "unexpected variable '{name}' reached the sandbox; env is {names:?}"
            );
        }
        // HOME is allowlisted but REPOINTED at the workspace, never inherited:
        // the real one is on the read-only rootfs, so anything writing to `~`
        // would fail.
        assert!(
            child_home.contains(WORKSPACE_PREFIX),
            "HOME must point into the workspace, got: {child_home}"
        );
        assert_ne!(
            child_home, parent_home,
            "the parent's HOME must not reach the child"
        );
    }

    #[tokio::test]
    async fn a_file_written_by_the_code_comes_back_in_files() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let result = run_python(
            "open('out.csv','w').write('a,b\\n1,2\\n')\nprint('done')",
            serde_json::json!([]),
        )
        .await;
        assert_eq!(result["exit_code"].as_i64(), Some(0));
        let files = result["files"].as_array().expect("files array");
        assert_eq!(files.len(), 1, "unexpected files: {files:?}");
        assert_eq!(files[0]["name"], "out.csv");
        assert_eq!(files[0]["mime"], "text/csv");
        assert_eq!(files[0]["bytes"].as_u64(), Some(8));
        assert!(files[0].get("truncated").is_none(), "must not be truncated");
        let content = files[0]["content_b64"].as_str().expect("content_b64");
        assert!(!content.is_empty(), "content must be returned");
        assert_eq!(
            String::from_utf8(STANDARD.decode(content).expect("decodes")).expect("utf8"),
            "a,b\n1,2\n"
        );
    }

    #[tokio::test]
    async fn a_files_in_entry_is_readable_by_the_code() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let result = run_python(
            "print(open('data.csv').read().strip().upper())",
            serde_json::json!([{
                "name": "data.csv",
                "content_b64": STANDARD.encode(b"region,amount\nnorth,42\n"),
            }]),
        )
        .await;
        assert_eq!(result["exit_code"].as_i64(), Some(0));
        assert_eq!(
            result["stdout"].as_str().expect("stdout"),
            "REGION,AMOUNT\nNORTH,42\n"
        );
        // The input file must not be echoed back as if the program produced it.
        assert_eq!(result["files"].as_array().map(Vec::len), Some(0));
    }

    #[tokio::test]
    async fn the_workspace_is_removed_after_the_call() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        // The program reports the directory it actually ran in, which is both a
        // cwd assertion and the only way to observe the (internal) path.
        let result = run_python("import os; print(os.getcwd())", serde_json::json!([])).await;
        let cwd = result["stdout"].as_str().expect("stdout").trim().to_owned();
        assert!(
            cwd.contains(WORKSPACE_PREFIX),
            "code must run inside its workspace, ran in: {cwd}"
        );
        assert!(
            !Path::new(&cwd).exists(),
            "workspace must be deleted after the call: {cwd}"
        );
    }

    #[tokio::test]
    async fn a_nonzero_exit_is_returned_as_data_with_its_traceback() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        // A failing program must still produce a RESULT (exit code + stderr +
        // any files it managed to write) — that is what the model needs to fix
        // its code. Turning it into a step error would discard all three.
        let result = run_python(
            "open('partial.txt','w').write('kept')\nraise ValueError('boom')",
            serde_json::json!([]),
        )
        .await;
        assert_eq!(result["exit_code"].as_i64(), Some(1));
        let stderr = result["stderr"].as_str().expect("stderr");
        assert!(stderr.contains("ValueError: boom"), "stderr: {stderr}");
        // The traceback names the staged program file, i.e. real line numbers.
        assert!(stderr.contains("_verevon_main.py"), "stderr: {stderr}");
        let files = result["files"].as_array().expect("files array");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["name"], "partial.txt");
    }

    #[tokio::test]
    async fn an_oversized_file_is_listed_with_its_real_size_and_no_content() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let result = run_python(
            &format!(
                "open('big.bin','wb').write(b'x' * {})\nopen('small.txt','w').write('ok')",
                MAX_FILE_BYTES + 1
            ),
            serde_json::json!([]),
        )
        .await;
        assert_eq!(result["exit_code"].as_i64(), Some(0));
        let files = result["files"].as_array().expect("files array");
        assert_eq!(files.len(), 2, "both files must be listed: {files:?}");
        let big = &files[0];
        assert_eq!(big["name"], "big.bin");
        assert_eq!(big["bytes"].as_u64(), Some(MAX_FILE_BYTES + 1));
        assert_eq!(big["content_b64"], "");
        assert_eq!(big["truncated"], true);
        // A single over-cap file must not suppress the ones that do fit.
        let small = &files[1];
        assert_eq!(small["name"], "small.txt");
        assert_eq!(small["content_b64"], STANDARD.encode(b"ok"));
        assert!(small.get("truncated").is_none());
    }

    #[tokio::test]
    async fn a_hung_program_is_killed_and_its_workspace_removed() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let input = serde_json::json!({
            "language": "python",
            "code": "import time\nwhile True:\n    time.sleep(1)\n",
        })
        .to_string();
        let started = std::time::Instant::now();
        let error = run_with_timeout(&input, "hungrun", "hungstep", None, Duration::from_secs(1))
            .await
            .expect_err("an endless loop must not outlive its timeout");
        assert!(error.contains("exceeded"), "error: {error}");
        assert!(started.elapsed() < Duration::from_secs(20));
        // Cleanup must survive the timeout path, not just the happy path.
        let leaked: Vec<_> = std::fs::read_dir(std::env::temp_dir())
            .expect("read temp dir")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("verevon-code-hungrun-hungstep")
            })
            .map(|entry| entry.path())
            .collect();
        assert!(leaked.is_empty(), "leaked workspaces: {leaked:?}");
    }

    #[tokio::test]
    async fn sh_programs_run_too() {
        if !crate::sandbox::is_supported() {
            eprintln!("skipping: local sandbox isolation is unavailable");
            return;
        }
        let input = serde_json::json!({
            "language": "sh",
            "code": "printf 'shell ok'; printf 'x' > made.txt",
        })
        .to_string();
        let output = run_with_timeout(&input, "sh-run", "sh-step", None, Duration::from_secs(30))
            .await
            .expect("sh call");
        let result: serde_json::Value = serde_json::from_str(&output).expect("result is JSON");
        assert_eq!(result["stdout"], "shell ok");
        assert_eq!(result["exit_code"].as_i64(), Some(0));
        let files = result["files"].as_array().expect("files array");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0]["name"], "made.txt");
        assert_eq!(files[0]["mime"], "text/plain");
    }

    /// A tempdir this test owns end to end — mirrors how `sandbox_lease`'s
    /// own persistent workspace directory is created and removed OUTSIDE
    /// `run_with_timeout`, which never creates or deletes it.
    struct PersistentTestDir(PathBuf);
    impl PersistentTestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!("verevon-code-persistent-test-{label}"));
            std::fs::create_dir_all(&path).expect("create persistent test dir");
            Self(path)
        }
    }
    impl Drop for PersistentTestDir {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[tokio::test]
    async fn a_persistent_workspace_is_reused_and_not_deleted_between_calls() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let dir = PersistentTestDir::new("reuse");
        let input = serde_json::json!({
            "code": "open('kept.txt','w').write('first call')",
        })
        .to_string();
        run_with_timeout(
            &input,
            "run-a",
            "step-1",
            Some(&dir.0),
            Duration::from_secs(30),
        )
        .await
        .expect("first call");
        assert!(
            dir.0.is_dir(),
            "a persistent workspace must survive the call"
        );
        assert_eq!(
            std::fs::read_to_string(dir.0.join("kept.txt")).expect("file from call 1"),
            "first call"
        );

        // A SECOND call in the same directory must be able to read what the
        // first call wrote — that is the entire point of a durable workspace.
        let input = serde_json::json!({
            "code": "print(open('kept.txt').read())",
        })
        .to_string();
        let result = run_with_timeout(
            &input,
            "run-a",
            "step-2",
            Some(&dir.0),
            Duration::from_secs(30),
        )
        .await
        .expect("second call");
        let result: serde_json::Value = serde_json::from_str(&result).expect("result is JSON");
        assert_eq!(result["stdout"].as_str(), Some("first call\n"));
    }

    #[tokio::test]
    async fn a_persistent_workspace_does_not_re_report_an_earlier_calls_unchanged_output() {
        if !python3_available() {
            eprintln!("skipping: python3 is not on PATH");
            return;
        }
        let dir = PersistentTestDir::new("no-rereport");
        run_with_timeout(
            &serde_json::json!({"code": "open('out.csv','w').write('a,b')"}).to_string(),
            "run-b",
            "step-1",
            Some(&dir.0),
            Duration::from_secs(30),
        )
        .await
        .expect("first call");

        // A second call that does not touch out.csv must not see it reported
        // again — the model already received it once.
        let result = run_with_timeout(
            &serde_json::json!({"code": "print('noop')"}).to_string(),
            "run-b",
            "step-2",
            Some(&dir.0),
            Duration::from_secs(30),
        )
        .await
        .expect("second call");
        let result: serde_json::Value = serde_json::from_str(&result).expect("result is JSON");
        assert_eq!(
            result["files"].as_array().map(Vec::len),
            Some(0),
            "an unchanged file from an earlier call must not be re-reported: {result}"
        );

        // But a THIRD call that overwrites it with different content must be
        // reported — a genuine change, not just re-existing.
        let result = run_with_timeout(
            &serde_json::json!({"code": "open('out.csv','w').write('a,b,c')"}).to_string(),
            "run-b",
            "step-3",
            Some(&dir.0),
            Duration::from_secs(30),
        )
        .await
        .expect("third call");
        let result: serde_json::Value = serde_json::from_str(&result).expect("result is JSON");
        let files = result["files"].as_array().expect("files array");
        assert_eq!(
            files.len(),
            1,
            "a genuinely changed file must be reported: {files:?}"
        );
        assert_eq!(files[0]["name"], "out.csv");
    }

    /// The attachments a turn carries, registered the way the gRPC handler does.
    /// The guard keeps the registration alive for the test.
    fn registered(
        files: &[(&str, &[u8])],
    ) -> (crate::run_inputs::RunInputsGuard, Arc<InputSet>) {
        use mp_contracts::attachments::sha256_hex;
        use mp_contracts::model_plane::v1::{RunAttachment, RunAttachmentKind};
        let attachments: Vec<RunAttachment> = files
            .iter()
            .map(|(name, content)| RunAttachment {
                id: (*name).to_owned(),
                name: (*name).to_owned(),
                mime: "application/octet-stream".to_owned(),
                size_bytes: content.len() as u64,
                sha256: sha256_hex(content),
                content: content.to_vec(),
                kind: RunAttachmentKind::File as i32,
            })
            .collect();
        let registry = crate::run_inputs::RunInputs::with_limits(4, 1 << 20, Duration::from_secs(60));
        let guard = registry
            .register("org", "run", &attachments)
            .expect("registers")
            .expect("guard");
        let set = registry.get("org", "run").expect("registered");
        (guard, set)
    }

    fn workspaces_of(run_id: &str) -> Vec<String> {
        let prefix = format!("{WORKSPACE_PREFIX}-{}-", path_slug(run_id));
        std::fs::read_dir(std::env::temp_dir())
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| entry.file_name().into_string().ok())
                    .filter(|name| name.starts_with(&prefix))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn an_attachment_is_readable_by_the_code() {
        if !python3_available() {
            eprintln!("skipping: python3 or the OS sandbox is unavailable");
            return;
        }
        let (_guard, set) = registered(&[("data.csv", b"month,revenue\njan,10\n")]);
        let output = run_staged(
            &serde_json::json!({"code": "print(open('attachments/data.csv').read())"}).to_string(),
            "run-attach-read",
            "step-1",
            None,
            Some(set),
            Duration::from_secs(60),
        )
        .await
        .expect("code_interpreter call");
        let result: serde_json::Value = serde_json::from_str(&output).expect("result is JSON");
        assert!(
            result["stdout"].as_str().unwrap_or_default().contains("jan,10"),
            "the program must read the attached file: {result}"
        );
        // The attachment is an input, not an output of the program.
        assert_eq!(result["files"].as_array().map(Vec::len), Some(0), "{result}");
    }

    #[tokio::test]
    async fn writing_into_the_attachments_directory_fails_read_only() {
        if !python3_available() {
            eprintln!("skipping: python3 or the OS sandbox is unavailable");
            return;
        }
        let (_guard, set) = registered(&[("data.csv", b"a,b\n")]);
        let code = [
            "import errno",
            "for target in ('attachments/data.csv', 'attachments/new.txt'):",
            "    try:",
            "        open(target, 'w').write('x')",
            "        print('WROTE', target)",
            "    except OSError as error:",
            "        print('ERRNO', error.errno == errno.EROFS, target)",
        ]
        .join("
");
        let output = run_staged(
            &serde_json::json!({"code": code}).to_string(),
            "run-attach-write",
            "step-1",
            None,
            Some(set),
            Duration::from_secs(60),
        )
        .await
        .expect("code_interpreter call");
        let result: serde_json::Value = serde_json::from_str(&output).expect("result is JSON");
        let stdout = result["stdout"].as_str().unwrap_or_default();
        assert!(!stdout.contains("WROTE"), "a write got through: {stdout}");
        assert_eq!(stdout.matches("ERRNO True").count(), 2, "EROFS for both: {stdout}");
    }

    /// A skill bundle is readable at `/skills` and not writable there. The file is left
    /// writable on purpose, so the refusal can only come from the read-only bind. Needs the
    /// `/skills` mount point the image creates.
    #[tokio::test]
    async fn a_skill_bundle_is_readable_at_skills_and_not_writable() {
        if !python3_available() || !Path::new(SKILLS_MOUNT).is_dir() {
            eprintln!("skipping: python3, the OS sandbox or the /skills mount point is unavailable");
            return;
        }
        let tree = std::env::temp_dir().join(format!("skills-test-{}", mp_ids::new_ulid()));
        let file = tree.join("docs").join("SKILL.md");
        std::fs::create_dir_all(file.parent().expect("parent")).expect("tree");
        std::fs::write(&file, b"bundle text").expect("bundle file");
        set_mode(&file, 0o644).expect("writable for the test");
        let code = [
            "import errno",
            "print(open('/skills/docs/SKILL.md').read())",
            "try:",
            "    open('/skills/docs/SKILL.md', 'w').write('x')",
            "    print('WROTE')",
            "except OSError as error:",
            "    print('ERRNO', error.errno == errno.EROFS)",
        ]
        .join("\n");
        let output = run_staged_with_skills(
            &serde_json::json!({"code": code}).to_string(),
            "run-skills-read",
            "step-1",
            None,
            None,
            Some(tree.as_path()),
            Duration::from_secs(60),
        )
        .await;
        let _ = std::fs::remove_dir_all(&tree);
        let output = output.expect("code_interpreter call");
        let result: serde_json::Value = serde_json::from_str(&output).expect("result is JSON");
        let stdout = result["stdout"].as_str().unwrap_or_default();
        assert!(stdout.contains("bundle text"), "the bundle is readable: {stdout}");
        assert!(!stdout.contains("WROTE"), "a write got through: {stdout}");
        assert!(stdout.contains("ERRNO True"), "EROFS at /skills: {stdout}");
    }

    #[test]
    fn attachments_name_is_reserved_for_files_in() {
        for name in ["attachments", "Attachments", "skills", "SKILLS"] {
            let input = serde_json::json!({
                "code": "print(1)",
                "files_in": [{ "name": name, "content_b64": "" }],
            })
            .to_string();
            let error = parse_request(&input).expect_err("the directory name is reserved");
            assert!(error.contains("reserved"), "error: {error}");
        }
        // The model cannot add staged files through its own input.
        let request = parse_request(
            r#"{"code":"print(1)","attachments":[{"name":"x","content":"eA=="}],"ro_inputs":[]}"#,
        )
        .expect("unknown keys are ignored");
        assert!(request.ro_inputs.is_none());
    }

    #[tokio::test]
    async fn unsafe_attachment_names_are_rejected_never_sanitized() {
        let escape = std::env::temp_dir().join("escape-from-attachments.csv");
        let _ = std::fs::remove_file(&escape);
        for name in ["../escape-from-attachments.csv", "/etc/passwd", "a/b.csv", ".hidden"] {
            let (_guard, set) = registered(&[(name, b"x")]);
            let error = run_staged(
                &serde_json::json!({"code": "print(1)"}).to_string(),
                "run-attach-unsafe",
                "step-1",
                None,
                Some(set),
                Duration::from_secs(30),
            )
            .await
            .expect_err("an unsafe name must refuse the call");
            assert!(error.contains("unsafe name"), "'{name}': {error}");
        }
        assert!(!escape.exists(), "a traversal name must never be written");
        assert!(workspaces_of("run-attach-unsafe").is_empty());
    }

    #[tokio::test]
    async fn workspace_is_removed_after_the_call() {
        // Passes on every host: where the sandbox is unavailable the call is
        // refused, and the workspace and the read-only directory must be gone
        // all the same.
        let (_guard, set) = registered(&[("data.csv", b"a,b\n")]);
        let _ = run_staged(
            &serde_json::json!({"code": "print(open('attachments/data.csv').read())"}).to_string(),
            "run-attach-cleanup",
            "step-1",
            None,
            Some(set),
            Duration::from_secs(60),
        )
        .await;
        assert_eq!(
            workspaces_of("run-attach-cleanup"),
            Vec::<String>::new(),
            "no verevon-code workspace may remain after the call"
        );
    }

    /// Puts owner write access back on a directory a test locked down, so the
    /// tempdir cleanup still works when an assertion fails part-way.
    #[cfg(unix)]
    struct OwnerWritable(PathBuf);

    #[cfg(unix)]
    impl Drop for OwnerWritable {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }

    /// A host directory a program must not reach through the persistent root:
    /// read-only, holding one file that has to survive.
    #[cfg(unix)]
    fn locked_host_dir(label: &str) -> PersistentTestDir {
        use std::os::unix::fs::PermissionsExt;
        let host = PersistentTestDir::new(&format!("{label}-{}", std::process::id()));
        std::fs::write(host.0.join("keep.txt"), b"host data").expect("host file");
        std::fs::set_permissions(&host.0, std::fs::Permissions::from_mode(0o500))
            .expect("lock the host directory");
        host
    }

    /// True only when nothing at all, not even a dangling link, sits at `path`.
    #[cfg(unix)]
    fn is_absent(path: &Path) -> bool {
        matches!(
            std::fs::symlink_metadata(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
    }

    /// A symlink planted at the reserved name is unlinked. The directory it points
    /// at keeps its mode and its contents: the chmod that used to run first
    /// followed the link and changed the host directory's mode.
    #[cfg(unix)]
    #[test]
    fn a_planted_symlink_is_unlinked_and_its_target_is_never_chmodded() {
        use std::os::unix::fs::PermissionsExt;
        let root = PersistentTestDir::new(&format!("planted-link-{}", std::process::id()));
        let host = locked_host_dir("planted-link-host");
        let _restore = OwnerWritable(host.0.clone());
        let link = root.0.join(ATTACHMENTS_DIR);
        std::os::unix::fs::symlink(&host.0, &link).expect("plant the link");

        remove_staged_dir(&link);

        assert!(is_absent(&link), "the planted link must be removed");
        let metadata = std::fs::metadata(&host.0).expect("host directory");
        let mode = metadata.permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o500,
            "the link target's mode must not change"
        );
        assert!(
            host.0.join("keep.txt").is_file(),
            "the link target's contents must survive"
        );
    }

    /// A regular file at the reserved name is not a staged directory. Left in
    /// place it would make every later staging fail with a create error.
    #[cfg(unix)]
    #[test]
    fn a_regular_file_planted_at_the_reserved_name_is_removed() {
        let root = PersistentTestDir::new(&format!("planted-file-{}", std::process::id()));
        let planted = root.0.join(ATTACHMENTS_DIR);
        std::fs::write(&planted, b"not a directory").expect("plant the file");

        remove_staged_dir(&planted);

        assert!(is_absent(&planted), "a planted file must be removed");
    }

    /// A staged directory is read-only and so is the file in it. Removing the
    /// pair has to make the directory writable first, as it always has.
    #[cfg(unix)]
    #[test]
    fn a_staged_read_only_directory_is_removed() {
        use std::os::unix::fs::PermissionsExt;
        let root = PersistentTestDir::new(&format!("staged-ro-{}", std::process::id()));
        let staged = root.0.join(ATTACHMENTS_DIR);
        std::fs::create_dir(&staged).expect("staged directory");
        let file = staged.join("data.csv");
        std::fs::write(&file, b"a,b\n").expect("staged file");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).expect("file mode");
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o555))
            .expect("directory mode");
        let _restore = OwnerWritable(staged.clone());

        remove_staged_dir(&staged);

        assert!(
            is_absent(&staged),
            "a staged read-only directory must be removed"
        );
    }

    /// A persistent root keeps its directory when a call is killed mid-way, so the
    /// next call finds it there and must restage over it.
    #[cfg(unix)]
    #[test]
    fn a_persistent_root_restages_over_a_leftover_attachments_directory() {
        if !crate::sandbox::is_supported() {
            eprintln!("skipping: the OS sandbox is unavailable");
            return;
        }
        let root = PersistentTestDir::new(&format!("restage-{}", std::process::id()));
        let (_first_guard, first) = registered(&[("data.csv", b"first call")]);
        let leftover = StagedInputs::stage(&root.0, &first)
            .expect("first staging")
            .expect("the first call has a file");
        // A killed call never runs Drop, so its directory stays behind.
        std::mem::forget(leftover);

        let (_second_guard, second) = registered(&[("data.csv", b"second call")]);
        let restaged = StagedInputs::stage(&root.0, &second)
            .expect("the second staging must not fail over the leftover")
            .expect("the second call has a file");
        assert_eq!(
            std::fs::read(root.0.join(ATTACHMENTS_DIR).join("data.csv")).expect("restaged file"),
            b"second call",
            "this call's files must replace the leftover"
        );
        drop(restaged);
        assert!(
            is_absent(&root.0.join(ATTACHMENTS_DIR)),
            "the restaged directory is removed on drop"
        );
    }

    /// The attack a persistent root allows: an earlier call plants `attachments` as
    /// a symlink to a host directory. Staging must replace the link with its own
    /// directory and leave the host directory exactly as it was.
    #[cfg(unix)]
    #[test]
    fn a_symlink_planted_in_a_persistent_root_is_replaced_and_never_followed() {
        use std::os::unix::fs::PermissionsExt;
        if !crate::sandbox::is_supported() {
            eprintln!("skipping: the OS sandbox is unavailable");
            return;
        }
        let root = PersistentTestDir::new(&format!("planted-stage-{}", std::process::id()));
        let host = locked_host_dir("planted-stage-host");
        let _restore = OwnerWritable(host.0.clone());
        std::os::unix::fs::symlink(&host.0, root.0.join(ATTACHMENTS_DIR)).expect("plant the link");
        let (_guard, set) = registered(&[("data.csv", b"fresh")]);

        let _staged = StagedInputs::stage(&root.0, &set)
            .expect("staging must replace the planted link")
            .expect("the call has a file");

        let metadata = std::fs::metadata(&host.0).expect("host directory");
        let mode = metadata.permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o500,
            "the host directory's mode must not change"
        );
        assert!(
            host.0.join("keep.txt").is_file(),
            "the host directory's contents must survive"
        );
        let entry = std::fs::symlink_metadata(root.0.join(ATTACHMENTS_DIR)).expect("staged entry");
        assert!(
            entry.is_dir() && !entry.file_type().is_symlink(),
            "staging must leave a real directory in the root"
        );
    }
}
