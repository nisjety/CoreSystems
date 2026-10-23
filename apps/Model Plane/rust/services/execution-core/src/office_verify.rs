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
//!   value), `structural`, or `render`. `render` names the LibreOffice layer,
//!   which is not in this build yet, so it runs the structural checks too.
//! * `OFFICE_VERIFY_MAX_FILES` — files checked per call, capped at
//!   [`MAX_FILES`].
//! * `OFFICE_VERIFY_TIMEOUT_SECS` — per file, default 60.
//! * `OFFICE_VERIFY_MAX_CONCURRENCY` — checks in flight across all calls,
//!   default 2.
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

/// Whether verification runs, and how far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Off,
    Structural,
    /// Reserved for the LibreOffice render layer. Until that layer exists it
    /// runs the structural checks, and nothing claims a render happened.
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
    Fut: Future<Output = Result<String, String>>,
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
        verification
            .reports
            .push(report_for(&subject.name, subject.kind, outcome));
    }
    verification
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
) -> Result<String, String> {
    let workspace = Workspace::create(WORKSPACE_TAG, kind.as_str())
        .map_err(|error| format!("the verifier could not create its workspace: {error}"))?;
    let root = workspace.path();
    let subject = stage(root, kind, &bytes)?;
    let policy = MpSandboxPolicy::WorkspaceWrite {
        writable_roots: vec![root.to_path_buf()],
        network: MpNetworkPolicy::Disabled,
    };
    let outcome = executor::execute_sandboxed_in_dir(
        &policy,
        PYTHON_PROGRAM,
        &[CHECKER_NAME.to_owned(), subject],
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
    Ok(outcome.stdout)
}

/// Verify the Office files among `files`. Returns an empty result when the switch
/// is off, so the caller's output does not change.
pub async fn verify_outputs(files: &[OutputFile]) -> Verification {
    let mode = parse_mode(env("CODE_INTERPRETER_OFFICE_VERIFY").as_deref());
    if mode == Mode::Off {
        return Verification::default();
    }
    if mode == Mode::Render {
        tracing::warn!("CODE_INTERPRETER_OFFICE_VERIFY=render: the render layer is not in this build; running structural checks");
    }
    let max_files = parse_max_files(env("OFFICE_VERIFY_MAX_FILES").as_deref());
    let timeout = parse_timeout(env("OFFICE_VERIFY_TIMEOUT_SECS").as_deref());
    verify_with(subjects(files), max_files, gate(), |kind, bytes| {
        run_checker(kind, bytes, timeout)
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
elif spec == "xlsx-missing-sheet":
    data = xlsx("Nope!A1")
elif spec == "xlsx-error-cell":
    data = xlsx("1/0", "#DIV/0!")
else:
    sys.exit(3)
sys.stdout.buffer.write(data)
"##;

    /// Runs the real checker on the host, outside the sandbox. Only for tests of
    /// the checker's own logic: the sandbox wrapper is exercised separately, and
    /// it refuses to run on a host with no bubblewrap. The environment is
    /// inherited because Windows needs SYSTEMROOT to start Python at all.
    async fn run_checker_on_host(kind: OfficeKind, bytes: &[u8]) -> Result<String, String> {
        let workspace = Workspace::create("office-verify-host", kind.as_str())
            .map_err(|error| error.to_string())?;
        let root = workspace.path();
        let subject = stage(root, kind, bytes)?;
        let output = tokio::process::Command::new(PYTHON_PROGRAM)
            .arg(CHECKER_NAME)
            .arg(&subject)
            .current_dir(root)
            .output()
            .await
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!("checker exited with {}", output.status));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
        let outcome = run_checker_on_host(kind, &fixture(spec)).await;
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
            Ok::<String, String>(
                r#"{"kind":"docx","level":"passed","checks":[],"stats":{}}"#.to_owned(),
            )
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
                    Ok::<String, String>(
                        r#"{"kind":"docx","level":"passed","checks":[],"stats":{}}"#.to_owned(),
                    )
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
}
