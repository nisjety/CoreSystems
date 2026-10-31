//! The result of a structural check on an Office or PDF deliverable.
//!
//! # What the check says
//!
//! A `.docx`, `.xlsx`, `.pptx` or `.pdf` produced by `code_interpreter` is only
//! a container of XML and streams. This contract carries what a hermetic
//! structural check found in that container: an unreadable zip, a relationship
//! that points nowhere, a formula that names a sheet that does not exist, a cell
//! that already holds an error value. It does NOT say the file opens in Office,
//! and every surface that shows the result must say so too.
//!
//! # Why it is bounded and settled here
//!
//! The report is produced by code the model's file was run through, so its text
//! is untrusted. [`OfficeVerification::from_untrusted_json`] caps every list and
//! every message before anything else sees it, and it never lets a report claim
//! less severity than its own checks show: a failed check always makes the file
//! failed, whatever level the checker wrote.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Most files one call reports on. A call that produces more Office files
/// leaves the rest unchecked and says so, rather than growing the report.
pub const MAX_FILES: usize = 4;

/// Most checks kept per file.
pub const MAX_CHECKS: usize = 40;

/// Longest message kept per check, in characters.
pub const MAX_MESSAGE: usize = 200;

/// Most stats kept per file.
pub const MAX_STATS: usize = 16;

/// Longest file name kept, in characters.
pub const MAX_NAME: usize = 255;

/// Severity of one check, or of a whole file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerificationLevel {
    /// Nothing wrong was found by the checks that ran.
    Passed,
    /// Something a reader should know, but no structural fault.
    Warnings,
    /// A structural fault that will very likely break the file in Office.
    Failed,
    /// The check did not run, so the file has no verdict.
    Skipped,
}

impl VerificationLevel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Warnings => "warnings",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// The formats the structural check understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfficeKind {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
}

impl OfficeKind {
    /// The kind a file name's extension names, if it is one this check reads.
    /// Case-insensitive; anything else is not an Office deliverable.
    #[must_use]
    pub fn from_file_name(name: &str) -> Option<Self> {
        let (_, extension) = name.rsplit_once('.')?;
        match extension.to_ascii_lowercase().as_str() {
            "docx" => Some(Self::Docx),
            "xlsx" => Some(Self::Xlsx),
            "pptx" => Some(Self::Pptx),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Pdf => "pdf",
        }
    }
}

/// One finding about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeCheck {
    pub id: String,
    pub level: VerificationLevel,
    pub message: String,
}

/// The structural verdict on one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficeVerification {
    /// The file name as `code_interpreter` returned it. Set by the caller, not
    /// trusted from the checker's output.
    #[serde(default)]
    pub name: String,
    pub kind: OfficeKind,
    pub level: VerificationLevel,
    /// Findings only: a file with nothing to say has an empty list.
    #[serde(default)]
    pub checks: Vec<OfficeCheck>,
    /// Counts the checker measured, such as sheets, slides or pages.
    #[serde(default)]
    pub stats: BTreeMap<String, u64>,
}

impl OfficeVerification {
    /// A file the check did not run on, with the reason as its one finding.
    #[must_use]
    pub fn skipped(name: &str, kind: OfficeKind, reason: &str) -> Self {
        Self {
            name: bounded_name(name),
            kind,
            level: VerificationLevel::Skipped,
            checks: vec![OfficeCheck {
                id: "verifier".to_owned(),
                level: VerificationLevel::Warnings,
                message: truncate_chars(reason, MAX_MESSAGE),
            }],
            stats: BTreeMap::new(),
        }
    }

    /// Parse a checker's report for the file `name`, which must be of `kind`.
    ///
    /// # Errors
    /// When the JSON does not parse, names a level or kind outside the contract,
    /// or reports a different kind than the file the caller sent.
    pub fn from_untrusted_json(raw: &str, name: &str, kind: OfficeKind) -> Result<Self, String> {
        let parsed: Self = serde_json::from_str(raw).map_err(|error| error.to_string())?;
        if parsed.kind != kind {
            return Err(format!(
                "report is for a {} file, not a {} file",
                parsed.kind.as_str(),
                kind.as_str()
            ));
        }
        Ok(Self {
            name: bounded_name(name),
            kind,
            level: settled_level(parsed.level, &parsed.checks),
            checks: parsed
                .checks
                .into_iter()
                .take(MAX_CHECKS)
                .map(|check| OfficeCheck {
                    id: truncate_chars(&check.id, 64),
                    level: check.level,
                    message: truncate_chars(&check.message, MAX_MESSAGE),
                })
                .collect(),
            stats: parsed.stats.into_iter().take(MAX_STATS).collect(),
        })
    }
}

/// The file's level is never less severe than its own checks. A checker that
/// found a fault and wrote `passed` still reports a failed file. A skipped file
/// has no verdict, so its reason stays a note and never becomes a warning.
fn settled_level(reported: VerificationLevel, checks: &[OfficeCheck]) -> VerificationLevel {
    if reported == VerificationLevel::Skipped {
        reported
    } else if checks
        .iter()
        .any(|check| check.level == VerificationLevel::Failed)
    {
        VerificationLevel::Failed
    } else if checks
        .iter()
        .any(|check| check.level == VerificationLevel::Warnings)
    {
        VerificationLevel::Warnings
    } else {
        reported
    }
}

fn bounded_name(name: &str) -> String {
    truncate_chars(name, MAX_NAME)
}

/// Truncates by characters, not bytes, so a Norwegian message is not cut mid
/// letter. Marks the cut so a reader can tell it was longer.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(max.saturating_sub(1)).collect();
    kept.push('…');
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(level: &str, checks: &str) -> String {
        format!(
            r#"{{"kind":"docx","level":"{level}","checks":{checks},"stats":{{"paragraphs":3}}}}"#
        )
    }

    #[test]
    fn a_clean_report_parses_and_keeps_its_name_and_stats() {
        let parsed = OfficeVerification::from_untrusted_json(
            &report("passed", "[]"),
            "letter.docx",
            OfficeKind::Docx,
        )
        .expect("valid report");
        assert_eq!(parsed.name, "letter.docx");
        assert_eq!(parsed.level, VerificationLevel::Passed);
        assert_eq!(parsed.stats.get("paragraphs"), Some(&3));
    }

    /// The checker's own verdict cannot soften a fault it reported.
    #[test]
    fn a_failed_check_makes_the_file_failed_whatever_level_was_written() {
        let raw = report(
            "passed",
            r#"[{"id":"zip","level":"failed","message":"not a zip"}]"#,
        );
        let parsed = OfficeVerification::from_untrusted_json(&raw, "a.docx", OfficeKind::Docx)
            .expect("valid report");
        assert_eq!(parsed.level, VerificationLevel::Failed);
    }

    #[test]
    fn from_untrusted_json_truncates_and_rejects_unknown_level() {
        let many: Vec<String> = (0..MAX_CHECKS + 10)
            .map(|index| {
                format!(
                    r#"{{"id":"c{index}","level":"warnings","message":"{}"}}"#,
                    "é".repeat(500)
                )
            })
            .collect();
        let raw = report("warnings", &format!("[{}]", many.join(",")));
        let parsed = OfficeVerification::from_untrusted_json(&raw, "a.docx", OfficeKind::Docx)
            .expect("valid report");
        assert_eq!(parsed.checks.len(), MAX_CHECKS);
        let message = &parsed.checks[0].message;
        assert_eq!(message.chars().count(), MAX_MESSAGE);
        assert!(message.ends_with('…'));

        let unknown = report("maybe", "[]");
        assert!(
            OfficeVerification::from_untrusted_json(&unknown, "a.docx", OfficeKind::Docx).is_err(),
            "a level outside the contract must not parse"
        );
        let unknown_check = report("passed", r#"[{"id":"x","level":"great","message":"ok"}]"#);
        assert!(OfficeVerification::from_untrusted_json(
            &unknown_check,
            "a.docx",
            OfficeKind::Docx
        )
        .is_err());
    }

    /// A file the checker did not run on keeps its `skipped` verdict even when
    /// the reason is recorded as a warnings check, so it never reads as passed.
    #[test]
    fn a_skipped_report_with_a_warnings_reason_stays_skipped() {
        let raw = report(
            "skipped",
            r#"[{"id":"verifier","level":"warnings","message":"too large to check"}]"#,
        );
        let parsed = OfficeVerification::from_untrusted_json(&raw, "a.docx", OfficeKind::Docx)
            .expect("valid report");
        assert_eq!(parsed.level, VerificationLevel::Skipped);
        assert_eq!(parsed.checks.len(), 1);
    }

    #[test]
    fn a_report_for_another_kind_is_refused() {
        assert!(OfficeVerification::from_untrusted_json(
            &report("passed", "[]"),
            "a.xlsx",
            OfficeKind::Xlsx,
        )
        .is_err());
    }

    #[test]
    fn kinds_are_read_from_the_extension_only() {
        assert_eq!(
            OfficeKind::from_file_name("Budget.XLSX"),
            Some(OfficeKind::Xlsx)
        );
        assert_eq!(OfficeKind::from_file_name("notes.csv"), None);
        assert_eq!(OfficeKind::from_file_name("docx"), None);
    }
}
