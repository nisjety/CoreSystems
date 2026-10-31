//! Structural verification of the Office and PDF files `code_interpreter` makes.
//!
//! # What it does, and what it does not
//!
//! Each Office or PDF file the program produced is checked by [`check.py`]
//! (embedded with `include_str!`) in its OWN hermetic workspace: a fresh
//! directory holding a copy of the file, run under the same `WorkspaceWrite`
//! policy with the network disabled. The check reads the container and reports
//! what it finds. It never changes the file, and the file returned to the user is
//! the one the program wrote, byte for byte.
//!
//! The verdict is advisory. A file that passes may still fail to open in Office,
//! and the model-facing text says so. The check exists so a corrupt deliverable
//! is seen and repaired, not so a clean one is certified.
//!
//! # Switches
//!
//! * `CODE_INTERPRETER_OFFICE_VERIFY` — `off` (default, and any unset or unknown
//!   value), `structural`, or `render`. `render` adds the LibreOffice layer: a
//!   PDF render, page and font facts, a recalculated copy of each workbook and
//!   up to three page previews. Its failures are warnings, never failures of the
//!   file.
//! * `OFFICE_VERIFY_MAX_FILES` — files checked per call, capped at
//!   [`MAX_FILES`].
//! * `OFFICE_VERIFY_TIMEOUT_SECS` — per file, default 60.
//! * `OFFICE_VERIFY_MAX_CONCURRENCY` — checks in flight across all calls,
//!   default 2.
//! * `OFFICE_PREVIEW_MAX_BYTES` — the largest PDF preview returned, default 2
//!   MiB. A PDF over it is dropped, never cut short.
//!
//! With the switch off this module returns an empty [`Verification`], so the
//! `code_interpreter` result is byte-identical to before it existed.

use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use mp_contracts::office_verification::{OfficeKind, OfficeVerification, MAX_FILES};
use tokio::sync::Semaphore;

use super::{child_env_for, OutputFile, Workspace, PYTHON_PROGRAM};
use crate::executor;
use crate::policy::{MpNetworkPolicy, MpSandboxPolicy};
use crate::scrub;

/// The structural checker, compiled into the binary so the check cannot be
/// swapped for a different program by anything in the workspace.
const CHECKER: &str = include_str!("office_verify/check.py");

const CHECKER_NAME: &str = "check.py";
const SUBJECT_STEM: &str = "subject";
const WORKSPACE_TAG: &str = "office-verify";
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const DEFAULT_CONCURRENCY: usize = 2;
const MAX_CONCURRENCY: usize = 8;
const DEFAULT_PREVIEW_MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PREVIEW_BYTES: u64 = 8 * 1024 * 1024;

/// The argument that asks `check.py` for the render layer.
const RENDER_ARG: &str = "render";
/// Where `check.py` leaves its render, relative to the verifier's workspace.
/// The page caps are fixed here and in `check.py`; the PDF cap is configurable.
const RENDER_DIR: &str = "render";
const PREVIEW_PDF: &str = "preview.pdf";
const MAX_THUMBS: usize = 3;
const MAX_THUMB_BYTES: u64 = 150 * 1024;

/// Whether verification runs, and how far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Structural,
    /// The structural checks plus the LibreOffice render layer.
    Render,
}

/// What one `code_interpreter` call's Office check produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Verification {
    /// One verdict per checked file, in the order the files were listed.
    pub reports: Vec<OfficeVerification>,
    /// Office files that were not checked: too large to return, or past
    /// `OFFICE_VERIFY_MAX_FILES`.
    pub unchecked: usize,
    /// The rendered previews of the checked files that produced any.
    pub previews: Vec<FilePreview>,
}

/// The rendered copy of one Office file, base64 for the viewer. Each part is
/// absent when it was over its cap, so a viewer never gets a cut-short PDF.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct FilePreview {
    pub name: String,
    pub pdf_b64: Option<String>,
    pub thumbs_b64: Vec<String>,
}

/// The render files `check.py` left in the workspace, read under their caps
/// before the workspace is removed.
#[derive(Debug, Default, PartialEq, Eq)]
struct RenderedFiles {
    pdf: Option<Vec<u8>>,
    thumbs: Vec<Vec<u8>>,
}

/// The strict parser: an unset or unknown value is off, and an unknown one says
/// so in the log rather than silently meaning something else.
fn parse_mode(raw: Option<&str>) -> Mode {
    match raw
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("" | "off") => Mode::Off,
        Some("structural") => Mode::Structural,
        Some("render") => Mode::Render,
        Some(other) => {
            tracing::warn!(
                value = other,
                "CODE_INTERPRETER_OFFICE_VERIFY is not off, structural or render; Office verification stays off"
            );
            Mode::Off
        }
    }
}

fn parse_max_files(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(MAX_FILES)
        .min(MAX_FILES)
}

fn parse_timeout(raw: Option<&str>) -> Duration {
    let secs = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

fn parse_concurrency(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(DEFAULT_CONCURRENCY)
        .min(MAX_CONCURRENCY)
}

fn parse_preview_max(raw: Option<&str>) -> u64 {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(DEFAULT_PREVIEW_MAX_BYTES)
        .min(MAX_PREVIEW_BYTES)
}

fn env(var: &str) -> Option<String> {
    std::env::var(var).ok()
}

/// Checks in flight across every `code_interpreter` call in this process. The
/// limit is read once, the first time a check is about to run.
fn gate() -> &'static Semaphore {
    static GATE: OnceLock<Semaphore> = OnceLock::new();
    GATE.get_or_init(|| {
        Semaphore::new(parse_concurrency(
            env("OFFICE_VERIFY_MAX_CONCURRENCY").as_deref(),
        ))
    })
}

/// One Office file the program produced, as the check sees it.
struct Subject {
    name: String,
    kind: OfficeKind,
    /// `None` when the file was not returned in full, so it cannot be checked.
    bytes: Option<Vec<u8>>,
}

/// The Office files among the program's outputs. Other files are not reported
/// at all: there is nothing structural to say about a CSV or a PNG here.
fn subjects(files: &[OutputFile]) -> Vec<Subject> {
    files
        .iter()
        .filter_map(|file| {
            let kind = OfficeKind::from_file_name(&file.name)?;
            let bytes = if file.truncated.is_none() && !file.content_b64.is_empty() {
                STANDARD.decode(&file.content_b64).ok()
            } else {
                None
            };
            Some(Subject {
                name: file.name.clone(),
                kind,
                bytes,
            })
        })
        .collect()
}

/// Check every subject that can be checked, up to `max_files`, through `check`.
/// The gate bounds the checks in flight; `check` is the sandboxed run in
/// production and a stand-in in tests.
async fn verify_with<F, Fut>(
    subjects: Vec<Subject>,
    max_files: usize,
    gate: &Semaphore,
    check: F,
) -> Verification
where
    F: Fn(OfficeKind, Vec<u8>) -> Fut,
    Fut: Future<Output = Result<(String, RenderedFiles), String>>,
{
    let mut verification = Verification::default();
    for subject in subjects {
        let Some(bytes) = subject.bytes else {
            verification.unchecked += 1;
            continue;
        };
        if verification.reports.len() >= max_files {
            verification.unchecked += 1;
            continue;
        }
        let outcome = match gate.acquire().await {
            Ok(_permit) => check(subject.kind, bytes).await,
            Err(_) => Err("the Office verifier is shut down".to_owned()),
        };
        match outcome {
            Ok((stdout, rendered)) => {
                verification
                    .reports
                    .push(report_for(&subject.name, subject.kind, Ok(stdout)));
                if let Some(preview) = preview_for(&subject.name, rendered) {
                    verification.previews.push(preview);
                }
            }
            Err(reason) => {
                verification
                    .reports
                    .push(report_for(&subject.name, subject.kind, Err(reason)))
            }
        }
    }
    verification
}

fn preview_for(name: &str, rendered: RenderedFiles) -> Option<FilePreview> {
    if rendered.pdf.is_none() && rendered.thumbs.is_empty() {
        return None;
    }
    Some(FilePreview {
        name: name.to_owned(),
        pdf_b64: rendered.pdf.map(|bytes| STANDARD.encode(bytes)),
        thumbs_b64: rendered
            .thumbs
            .iter()
            .map(|bytes| STANDARD.encode(bytes))
            .collect(),
    })
}

/// The render files under `root`, each read only within its cap. The program
/// under check can leave anything in its directory, a link included, so a render
/// directory or file that is not a plain one is ignored: a link would read a file
/// on the host.
fn collect_render(root: &std::path::Path, pdf_max_bytes: u64) -> RenderedFiles {
    let render = root.join(RENDER_DIR);
    let is_plain_dir = std::fs::symlink_metadata(&render).is_ok_and(|metadata| metadata.is_dir());
    if !is_plain_dir {
        return RenderedFiles::default();
    }
    RenderedFiles {
        pdf: read_capped(&render.join(PREVIEW_PDF), pdf_max_bytes),
        thumbs: (1..=MAX_THUMBS)
            .filter_map(|index| {
                read_capped(&render.join(format!("thumb-{index}.png")), MAX_THUMB_BYTES)
            })
            .collect(),
    }
}

fn read_capped(path: &std::path::Path, cap: u64) -> Option<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > cap {
        return None;
    }
    std::fs::read(path).ok()
}

/// Turn one check's outcome into the contract. Anything that stops the check
/// from giving a verdict is `skipped`, never `passed`.
fn report_for(name: &str, kind: OfficeKind, outcome: Result<String, String>) -> OfficeVerification {
    match outcome {
        Ok(stdout) => {
            // Scrub the whole report before it is parsed and cut. A secret cut
            // in half no longer matches the scrub patterns.
            let scrubbed = scrub::scrub_string(&stdout);
            OfficeVerification::from_untrusted_json(&scrubbed, name, kind).unwrap_or_else(|_| {
                OfficeVerification::skipped(name, kind, "the verifier's report could not be read")
            })
        }
        Err(reason) => OfficeVerification::skipped(name, kind, &scrub::scrub_string(&reason)),
    }
}

/// Copy the checker and the file into a fresh workspace.
fn stage(root: &std::path::Path, kind: OfficeKind, bytes: &[u8]) -> Result<String, String> {
    std::fs::write(root.join(CHECKER_NAME), CHECKER.as_bytes())
        .map_err(|error| format!("the verifier could not stage its checker: {error}"))?;
    let subject = format!("{SUBJECT_STEM}.{}", kind.as_str());
    std::fs::write(root.join(&subject), bytes)
        .map_err(|error| format!("the verifier could not stage the file: {error}"))?;
    Ok(subject)
}

/// One file's check, sandboxed. The workspace is removed when this returns on
/// every path, including the timeout and the refusal to run unsandboxed.
async fn run_checker(
    kind: OfficeKind,
    bytes: Vec<u8>,
    timeout: Duration,
    mode: Mode,
    preview_max_bytes: u64,
) -> Result<(String, RenderedFiles), String> {
    let workspace = Workspace::create(WORKSPACE_TAG, kind.as_str())
        .map_err(|error| format!("the verifier could not create its workspace: {error}"))?;
    let root = workspace.path();
    let subject = stage(root, kind, &bytes)?;
    let policy = MpSandboxPolicy::WorkspaceWrite {
        writable_roots: vec![root.to_path_buf()],
        network: MpNetworkPolicy::Disabled,
    };
    let mut args = vec![CHECKER_NAME.to_owned(), subject];
    if mode == Mode::Render {
        args.push(RENDER_ARG.to_owned());
    }
    let outcome = executor::execute_sandboxed_in_dir(
        &policy,
        PYTHON_PROGRAM,
        &args,
        root,
        &child_env_for(root),
        timeout,
    )
    .await
    .map_err(|error| format!("the verifier did not run: {error}"))?;
    if outcome.exit_code != 0 {
        return Err(format!(
            "the verifier exited with status {}",
            outcome.exit_code
        ));
    }
    // Read before `workspace` is dropped at the end of this function.
    let rendered = if mode == Mode::Render {
        collect_render(root, preview_max_bytes)
    } else {
        RenderedFiles::default()
    };
    Ok((outcome.stdout, rendered))
}

/// Verify the Office files among `files`. Returns an empty result when the switch
/// is off, so the caller's output does not change.
pub async fn verify_outputs(files: &[OutputFile]) -> Verification {
    let mode = parse_mode(env("CODE_INTERPRETER_OFFICE_VERIFY").as_deref());
    if mode == Mode::Off {
        return Verification::default();
    }
    let max_files = parse_max_files(env("OFFICE_VERIFY_MAX_FILES").as_deref());
    let timeout = parse_timeout(env("OFFICE_VERIFY_TIMEOUT_SECS").as_deref());
    let preview_max_bytes = parse_preview_max(env("OFFICE_PREVIEW_MAX_BYTES").as_deref());
    verify_with(subjects(files), max_files, gate(), |kind, bytes| {
        run_checker(kind, bytes, timeout, mode, preview_max_bytes)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::super::WORKSPACE_PREFIX;
    use super::*;
    use mp_contracts::office_verification::VerificationLevel;

    /// Builds the fixture packages in Python, the same language the checker is
    /// written in, so a fixture is a real zip rather than a hand-rolled one.
    const FIXTURE_BUILDER: &str = r##"
import io, sys, zipfile
REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = ('<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
      '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
      '<Default Extension="xml" ContentType="application/xml"/></Types>')
def rels(*targets):
    body = "".join('<Relationship Id="rId%d" Type="http://x/rel" Target="%s"/>' % (i, t) for i, t in enumerate(targets, 1))
    return '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="%s">%s</Relationships>' % (REL_NS, body)
def package(parts):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, data in parts:
            archive.writestr(name, data)
    return buf.getvalue()
def docx(doc_rels_targets=()):
    return package([
        ("[Content_Types].xml", CT),
        ("_rels/.rels", rels("word/document.xml")),
        ("word/document.xml", '<?xml version="1.0"?><w:document xmlns:w="http://w"><w:body><w:p><w:r><w:t>Hi</w:t></w:r></w:p></w:body></w:document>'),
        ("word/_rels/document.xml.rels", rels(*doc_rels_targets)),
    ])
def xlsx(formula, cached_error=None):
    cell = '<c r="A1"><f>%s</f><v>0</v></c>' % formula
    if cached_error is not None:
        cell = '<c r="A1" t="e"><f>%s</f><v>%s</v></c>' % (formula, cached_error)
    return package([
        ("[Content_Types].xml", CT),
        ("_rels/.rels", rels("xl/workbook.xml")),
        ("xl/workbook.xml", '<?xml version="1.0"?><workbook xmlns="http://m" xmlns:r="http://r"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>'),
        ("xl/worksheets/sheet1.xml", '<?xml version="1.0"?><worksheet xmlns="http://m"><sheetData><row r="1">%s</row></sheetData></worksheet>' % cell),
    ])
spec = sys.argv[1]
if spec == "docx-valid":
    data = docx()
elif spec == "docx-truncated":
    data = docx()[: len(docx()) // 2]
elif spec == "docx-missing-target":
    data = docx(("media/missing.png",))
elif spec == "docx-traversal":
    data = package([
        ("[Content_Types].xml", CT),
        ("_rels/.rels", rels("word/document.xml")),
        ("word/document.xml", '<w:document xmlns:w="http://w"/>'),
        ("../escape.xml", "<x/>"),
    ])
elif spec == "docx-missing-font":
    data = package([
        ("[Content_Types].xml", CT),
        ("_rels/.rels", rels("word/document.xml")),
        ("word/document.xml", '<?xml version="1.0"?><w:document xmlns:w="http://w"><w:body><w:p><w:r><w:rPr><w:rFonts w:ascii="Verevon Missing Font" w:hAnsi="Verevon Missing Font"/></w:rPr><w:t>Hi</w:t></w:r></w:p></w:body></w:document>'),
    ])
elif spec == "xlsx-missing-sheet":
    data = xlsx("Nope!A1")
elif spec == "xlsx-error-cell":
    data = xlsx("1/0", "#DIV/0!")
elif spec == "xlsx-formula-div0":
    data = xlsx("1/0")
else:
    sys.exit(3)
sys.stdout.buffer.write(data)
"##;

    /// Runs the real checker on the host, outside the sandbox. Only for tests of
    /// the checker's own logic: the sandbox wrapper is exercised separately, and
    /// it refuses to run on a host with no bubblewrap. The environment is
    /// inherited because Windows needs SYSTEMROOT to start Python at all.
    async fn run_checker_on_host(
        kind: OfficeKind,
        bytes: &[u8],
        mode: Mode,
    ) -> Result<(String, RenderedFiles), String> {
        let workspace = Workspace::create("office-verify-host", kind.as_str())
            .map_err(|error| error.to_string())?;
        let root = workspace.path();
        let subject = stage(root, kind, bytes)?;
        let mut command = tokio::process::Command::new(PYTHON_PROGRAM);
        command.arg(CHECKER_NAME).arg(&subject);
        if mode == Mode::Render {
            command.arg(RENDER_ARG);
        }
        let output = command
            .current_dir(root)
            .output()
            .await
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("checker exited with {}", output.status));
        }
        let rendered = if mode == Mode::Render {
            collect_render(root, DEFAULT_PREVIEW_MAX_BYTES)
        } else {
            RenderedFiles::default()
        };
        Ok((
            String::from_utf8_lossy(&output.stdout).into_owned(),
            rendered,
        ))
    }

    fn soffice_available() -> bool {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths)
                .any(|dir| dir.join("soffice").is_file() || dir.join("soffice.exe").is_file())
        })
    }

    fn openpyxl_available() -> bool {
        std::process::Command::new(PYTHON_PROGRAM)
            .args(["-c", "import openpyxl"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn fixture(spec: &str) -> Vec<u8> {
        let output = std::process::Command::new(PYTHON_PROGRAM)
            .arg("-c")
            .arg(FIXTURE_BUILDER)
            .arg(spec)
            .output()
            .expect("python builds the fixture");
        assert!(output.status.success(), "fixture {spec} failed");
        output.stdout
    }

    fn python_available() -> bool {
        std::process::Command::new(PYTHON_PROGRAM)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    async fn verdict(kind: OfficeKind, spec: &str) -> OfficeVerification {
        let outcome = run_checker_on_host(kind, &fixture(spec), Mode::Structural)
            .await
            .map(|(stdout, _)| stdout);
        report_for("deliverable", kind, outcome)
    }

    fn has_check(report: &OfficeVerification, id: &str, level: VerificationLevel) -> bool {
        report
            .checks
            .iter()
            .any(|check| check.id == id && check.level == level)
    }

    #[test]
    fn switches_are_strict_and_off_by_default() {
        assert_eq!(parse_mode(None), Mode::Off);
        assert_eq!(parse_mode(Some("")), Mode::Off);
        assert_eq!(parse_mode(Some("off")), Mode::Off);
        assert_eq!(parse_mode(Some(" Structural ")), Mode::Structural);
        assert_eq!(parse_mode(Some("render")), Mode::Render);
        assert_eq!(
            parse_mode(Some("true")),
            Mode::Off,
            "only the named values enable it"
        );
        assert_eq!(parse_max_files(None), MAX_FILES);
        assert_eq!(parse_max_files(Some("99")), MAX_FILES);
        assert_eq!(parse_max_files(Some("0")), MAX_FILES);
        assert_eq!(
            parse_timeout(Some("0")),
            Duration::from_secs(DEFAULT_TIMEOUT_SECS)
        );
        assert_eq!(parse_timeout(Some("15")), Duration::from_secs(15));
        assert_eq!(parse_concurrency(Some("100")), MAX_CONCURRENCY);
        assert_eq!(parse_concurrency(None), DEFAULT_CONCURRENCY);
    }

    /// A check's message can carry text the program wrote into the file, so it
    /// is scrubbed and capped before the model sees it.
    #[test]
    fn report_text_is_scrubbed_and_capped() {
        let long = "x".repeat(600);
        let raw = format!(
            r#"{{"kind":"docx","level":"failed","checks":[{{"id":"xml","level":"failed","message":"sk-abcdef1234567890XYZ {long}"}}],"stats":{{}}}}"#
        );
        let report = report_for("a.docx", OfficeKind::Docx, Ok(raw));
        let message = &report.checks[0].message;
        assert!(
            !message.contains("sk-abcdef"),
            "a key in a message must not survive"
        );
        assert!(message.chars().count() <= mp_contracts::office_verification::MAX_MESSAGE);
    }

    #[test]
    fn non_office_files_are_not_reported_and_oversize_ones_are_counted() {
        let files = vec![
            OutputFile {
                name: "data.csv".to_owned(),
                mime: "text/csv",
                bytes: 2,
                content_b64: STANDARD.encode(b"ok"),
                truncated: None,
            },
            OutputFile {
                name: "deck.pptx".to_owned(),
                mime: "application/octet-stream",
                bytes: 9_000_000,
                content_b64: String::new(),
                truncated: Some(true),
            },
        ];
        let found = subjects(&files);
        assert_eq!(found.len(), 1, "only the Office file is a subject");
        assert_eq!(found[0].kind, OfficeKind::Pptx);
        assert!(
            found[0].bytes.is_none(),
            "a withheld file cannot be checked"
        );
    }

    #[tokio::test]
    async fn over_the_file_limit_is_counted_not_checked() {
        let gate = Semaphore::new(1);
        let subjects = vec![
            Subject {
                name: "a.docx".to_owned(),
                kind: OfficeKind::Docx,
                bytes: Some(vec![1]),
            },
            Subject {
                name: "b.docx".to_owned(),
                kind: OfficeKind::Docx,
                bytes: Some(vec![2]),
            },
        ];
        let verification = verify_with(subjects, 1, &gate, |_kind, _bytes| async {
            Ok::<(String, RenderedFiles), String>((
                r#"{"kind":"docx","level":"passed","checks":[],"stats":{}}"#.to_owned(),
                RenderedFiles::default(),
            ))
        })
        .await;
        assert_eq!(verification.reports.len(), 1);
        assert_eq!(verification.unchecked, 1);
    }

    /// The gate is shared across calls, so it caps checks in flight process-wide.
    #[tokio::test]
    async fn concurrency_is_bounded() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let gate = Semaphore::new(2);
        let in_flight = AtomicUsize::new(0);
        let most_in_flight = AtomicUsize::new(0);
        let calls = (0..6).map(|index| {
            let gate = &gate;
            let in_flight = &in_flight;
            let most_in_flight = &most_in_flight;
            async move {
                let subjects = vec![Subject {
                    name: format!("f{index}.docx"),
                    kind: OfficeKind::Docx,
                    bytes: Some(vec![0]),
                }];
                verify_with(subjects, MAX_FILES, gate, |_kind, _bytes| async move {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    most_in_flight.fetch_max(now, Ordering::SeqCst);
                    for _ in 0..8 {
                        tokio::task::yield_now().await;
                    }
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok::<(String, RenderedFiles), String>((
                        r#"{"kind":"docx","level":"passed","checks":[],"stats":{}}"#.to_owned(),
                        RenderedFiles::default(),
                    ))
                })
                .await
            }
        });
        futures::future::join_all(calls).await;
        assert!(
            most_in_flight.load(Ordering::SeqCst) <= 2,
            "no more than the gate allows"
        );
        assert!(most_in_flight.load(Ordering::SeqCst) >= 1);
    }

    /// The workspace is the verifier's own; it must not outlive the check, on the
    /// refusal path here and on the timeout path where bubblewrap is present.
    #[tokio::test]
    async fn verify_workspace_is_removed_on_timeout() {
        let prefix = format!("{WORKSPACE_PREFIX}-{WORKSPACE_TAG}-docx");
        let count = || {
            std::fs::read_dir(std::env::temp_dir())
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
                        .count()
                })
                .unwrap_or(0)
        };
        let before = count();
        let _ = run_checker(
            OfficeKind::Docx,
            b"not a package".to_vec(),
            Duration::from_millis(1),
            Mode::Structural,
            DEFAULT_PREVIEW_MAX_BYTES,
        )
        .await;
        assert_eq!(count(), before, "no verifier workspace is left behind");
    }

    #[tokio::test]
    async fn a_valid_python_docx_passes() {
        if !python_available() {
            return;
        }
        let report = verdict(OfficeKind::Docx, "docx-valid").await;
        assert_eq!(report.level, VerificationLevel::Passed, "{report:?}");
        assert!(report.checks.is_empty());
    }

    #[tokio::test]
    async fn a_truncated_zip_reports_failed() {
        if !python_available() {
            return;
        }
        let report = verdict(OfficeKind::Docx, "docx-truncated").await;
        assert_eq!(report.level, VerificationLevel::Failed, "{report:?}");
        assert!(has_check(&report, "container", VerificationLevel::Failed));
    }

    #[tokio::test]
    async fn a_missing_relationship_target_fails() {
        if !python_available() {
            return;
        }
        let report = verdict(OfficeKind::Docx, "docx-missing-target").await;
        assert_eq!(report.level, VerificationLevel::Failed, "{report:?}");
        assert!(has_check(
            &report,
            "relationship",
            VerificationLevel::Failed
        ));
    }

    #[tokio::test]
    async fn zip_traversal_names_are_refused() {
        if !python_available() {
            return;
        }
        let report = verdict(OfficeKind::Docx, "docx-traversal").await;
        assert_eq!(report.level, VerificationLevel::Failed, "{report:?}");
        assert!(has_check(&report, "names", VerificationLevel::Failed));
    }

    #[tokio::test]
    async fn a_formula_to_a_missing_sheet_warns() {
        if !python_available() {
            return;
        }
        let report = verdict(OfficeKind::Xlsx, "xlsx-missing-sheet").await;
        assert_eq!(report.level, VerificationLevel::Warnings, "{report:?}");
        assert!(has_check(
            &report,
            "formula_sheet",
            VerificationLevel::Warnings
        ));
    }

    #[tokio::test]
    async fn a_literal_error_cell_fails() {
        if !python_available() {
            return;
        }
        let report = verdict(OfficeKind::Xlsx, "xlsx-error-cell").await;
        assert_eq!(report.level, VerificationLevel::Failed, "{report:?}");
        assert!(has_check(&report, "error_cells", VerificationLevel::Failed));
    }

    /// Without LibreOffice the render layer says so as a warning; the file's own
    /// verdict is untouched, so a clean file is never reported as failed.
    #[tokio::test]
    async fn a_missing_soffice_is_a_warning_never_a_failure() {
        if !python_available() || soffice_available() {
            return;
        }
        let (stdout, _) =
            run_checker_on_host(OfficeKind::Docx, &fixture("docx-valid"), Mode::Render)
                .await
                .expect("the checker runs");
        let report = report_for("deliverable", OfficeKind::Docx, Ok(stdout));
        assert_eq!(report.level, VerificationLevel::Warnings, "{report:?}");
        assert!(has_check(&report, "render", VerificationLevel::Warnings));
        assert!(!report
            .checks
            .iter()
            .any(|check| check.level == VerificationLevel::Failed));
    }

    #[tokio::test]
    async fn libreoffice_converts_a_docx_to_pdf_under_the_bwrap_policy() {
        if !python_available() || !soffice_available() || !crate::sandbox::is_supported() {
            return;
        }
        let (_stdout, rendered) = run_checker(
            OfficeKind::Docx,
            fixture("docx-valid"),
            Duration::from_secs(60),
            Mode::Render,
            DEFAULT_PREVIEW_MAX_BYTES,
        )
        .await
        .expect("the sandboxed render runs");
        assert!(
            rendered.pdf.is_some(),
            "LibreOffice wrote a PDF in the sandbox"
        );
        assert!(!rendered.thumbs.is_empty(), "the first page was rendered");
    }

    #[tokio::test]
    async fn recalc_reports_div0_in_a_formula_cell() {
        if !python_available() || !soffice_available() || !openpyxl_available() {
            return;
        }
        let (stdout, _) = run_checker_on_host(
            OfficeKind::Xlsx,
            &fixture("xlsx-formula-div0"),
            Mode::Render,
        )
        .await
        .expect("the checker runs");
        let report = report_for("deliverable", OfficeKind::Xlsx, Ok(stdout));
        assert_eq!(report.level, VerificationLevel::Failed, "{report:?}");
        assert!(has_check(&report, "recalc", VerificationLevel::Failed));
    }

    /// The document names a family that is not installed. LibreOffice draws an
    /// installed stand-in instead, and that stand-in is embedded, so only the
    /// families the file declares can be compared with the ones the render shows.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_missing_font_is_listed_as_substituted() {
        if !python_available() || !soffice_available() {
            return;
        }
        let (stdout, _) = run_checker_on_host(
            OfficeKind::Docx,
            &fixture("docx-missing-font"),
            Mode::Render,
        )
        .await
        .expect("the checker runs");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("the report is JSON");
        assert_eq!(parsed["stats"]["fonts_substituted"], 1, "{parsed}");
        let report = report_for("deliverable", OfficeKind::Docx, Ok(stdout));
        assert!(
            has_check(&report, "fonts", VerificationLevel::Warnings),
            "{report:?}"
        );
    }

    /// A preview over its cap is dropped whole; the cap is checked per part, so a
    /// large thumbnail does not take the small ones with it.
    #[test]
    fn preview_over_cap_is_dropped_not_truncated() {
        let workspace = Workspace::create("office-verify-render", "preview").expect("workspace");
        let render = workspace.path().join(RENDER_DIR);
        std::fs::create_dir_all(&render).expect("render dir");
        std::fs::write(render.join(PREVIEW_PDF), vec![7u8; 3000]).expect("pdf");
        std::fs::write(
            render.join("thumb-1.png"),
            vec![1u8; MAX_THUMB_BYTES as usize + 1],
        )
        .expect("large thumbnail");
        std::fs::write(render.join("thumb-2.png"), vec![2u8; 1024]).expect("small thumbnail");
        let small = collect_render(workspace.path(), 2000);
        assert!(small.pdf.is_none(), "a PDF over its cap is dropped");
        assert_eq!(
            small.thumbs,
            vec![vec![2u8; 1024]],
            "only the thumbnail under its cap is kept"
        );
        let large = collect_render(workspace.path(), 4000);
        assert_eq!(large.pdf.map(|bytes| bytes.len()), Some(3000));
    }

    /// The program under check can leave a link in its render directory. The host
    /// must not follow it to a file outside the workspace.
    #[cfg(unix)]
    #[test]
    fn a_symlink_in_the_render_is_not_read() {
        let outside = Workspace::create("office-verify-outside", "secret").expect("workspace");
        let secret = outside.path().join("host.txt");
        std::fs::write(&secret, b"host secret").expect("secret");
        let inside = Workspace::create("office-verify-render", "link").expect("workspace");
        let render = inside.path().join(RENDER_DIR);
        std::fs::create_dir_all(&render).expect("render dir");
        std::os::unix::fs::symlink(&secret, render.join(PREVIEW_PDF)).expect("symlink");
        assert!(collect_render(inside.path(), u64::MAX).pdf.is_none());
    }
}
