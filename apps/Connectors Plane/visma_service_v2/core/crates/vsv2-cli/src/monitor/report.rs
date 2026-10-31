//! What the checks found, and how it is shown: the text report, the JSON report, the webhook
//! message and the exit code. All pure: a report in, text out.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Serialize, Serializer};

/// The check that stands for "the state database could not be read".
pub const UNREADABLE_CHECK: &str = "state_database";

/// The most problem lines a webhook message carries; the rest is a count.
const WEBHOOK_MAX_PROBLEM_LINES: usize = 15;

/// Exit code: nothing wrong (warnings are allowed unless `--strict`).
pub const EXIT_OK: i32 = 0;
/// Exit code: at least one problem (or a warning under `--strict`).
pub const EXIT_PROBLEM: i32 = 1;
/// Exit code: the state could not be read, so nothing was checked.
pub const EXIT_UNREADABLE: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warning,
    Problem,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "WARNING",
            Self::Problem => "PROBLEM",
        }
    }
}

/// One result of one check for one tenant (or for the pipeline as a whole: no tenant).
///
/// The field names are the JSON contract of `--format json`; `observed_at` is when the thing the
/// finding is about happened (the pass or run it is based on), `null` when there is no such row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub tenant_id: Option<String>,
    pub tenant_name: Option<String>,
    pub check: String,
    pub severity: Severity,
    pub detail: String,
    #[serde(serialize_with = "serialize_timestamp")]
    pub observed_at: Option<DateTime<Utc>>,
}

impl Finding {
    pub fn new(
        tenant_id: Option<&str>,
        tenant_name: Option<&str>,
        check: impl Into<String>,
        severity: Severity,
        detail: impl Into<String>,
        observed_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            tenant_id: tenant_id.map(str::to_string),
            tenant_name: tenant_name.map(str::to_string),
            check: check.into(),
            severity,
            detail: detail.into(),
            observed_at,
        }
    }
}

/// The overall verdict (`summary.status` in the JSON).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warning,
    Problem,
    /// The state database could not be read: nothing was checked.
    Unreadable,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "WARNING",
            Self::Problem => "PROBLEM",
            Self::Unreadable => "UNREADABLE",
        }
    }
}

/// The exit code for a verdict. Warnings pass unless `strict`.
pub fn exit_code(status: Status, strict: bool) -> i32 {
    match status {
        Status::Unreadable => EXIT_UNREADABLE,
        Status::Problem => EXIT_PROBLEM,
        Status::Warning if strict => EXIT_PROBLEM,
        Status::Warning | Status::Ok => EXIT_OK,
    }
}

/// The `summary` object of the JSON report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub status: Status,
    pub exit_code: i32,
    pub problems: usize,
    pub warnings: usize,
    pub ok: usize,
    pub tenants: usize,
    pub evaluated_at: String,
    /// Why the state could not be read (the database URL masked); `null` when it could.
    pub error: Option<String>,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    summary: Summary,
    findings: &'a [Finding],
}

/// Everything one run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub evaluated_at: DateTime<Utc>,
    pub tenants: usize,
    pub findings: Vec<Finding>,
    unreadable: Option<String>,
}

impl Report {
    /// The state was read and checked.
    pub fn checked(evaluated_at: DateTime<Utc>, tenants: usize, findings: Vec<Finding>) -> Self {
        Self {
            evaluated_at,
            tenants,
            findings,
            unreadable: None,
        }
    }

    /// The state could not be read. `reason` is shown in the report and in the JSON, never in the
    /// webhook message, and must already be free of secrets.
    pub fn unreadable(evaluated_at: DateTime<Utc>, tenants: usize, reason: String) -> Self {
        let finding = Finding::new(
            None,
            None,
            UNREADABLE_CHECK,
            Severity::Problem,
            "the state database cannot be read, so nothing was checked",
            None,
        );
        Self {
            evaluated_at,
            tenants,
            findings: vec![finding],
            unreadable: Some(reason),
        }
    }

    fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.severity == severity)
            .count()
    }

    pub fn status(&self) -> Status {
        if self.unreadable.is_some() {
            Status::Unreadable
        } else if self.count(Severity::Problem) > 0 {
            Status::Problem
        } else if self.count(Severity::Warning) > 0 {
            Status::Warning
        } else {
            Status::Ok
        }
    }

    pub fn exit_code(&self, strict: bool) -> i32 {
        exit_code(self.status(), strict)
    }

    pub fn summary(&self, strict: bool) -> Summary {
        Summary {
            status: self.status(),
            exit_code: self.exit_code(strict),
            problems: self.count(Severity::Problem),
            warnings: self.count(Severity::Warning),
            ok: self.count(Severity::Ok),
            tenants: self.tenants,
            evaluated_at: timestamp(self.evaluated_at),
            error: self.unreadable.clone(),
        }
    }

    /// The JSON report: `{"summary": {...}, "findings": [{tenant_id, tenant_name, check,
    /// severity, detail, observed_at}, ...]}`.
    pub fn to_json(&self, strict: bool) -> String {
        let report = JsonReport {
            summary: self.summary(strict),
            findings: &self.findings,
        };
        // Strings, numbers and options only: serializing cannot fail. The fallback keeps the
        // exit path free of panics all the same, and is JSON too.
        serde_json::to_string_pretty(&report).unwrap_or_else(|e| {
            serde_json::json!({ "error": format!("could not render the report: {e}") }).to_string()
        })
    }

    /// The report for people: the verdict, the counts, then every problem and warning with its
    /// detail, and the tenants with nothing to report.
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "vsv2 monitor: {} at {} (database clock)\n{}\n",
            self.status().label(),
            timestamp(self.evaluated_at),
            self.counts_line()
        );
        if let Some(reason) = &self.unreadable {
            out.push_str(&format!("reason: {reason}\n"));
        }
        for severity in [Severity::Problem, Severity::Warning] {
            let section: Vec<&Finding> = self
                .findings
                .iter()
                .filter(|finding| finding.severity == severity)
                .collect();
            if section.is_empty() {
                continue;
            }
            out.push('\n');
            for finding in section {
                out.push_str(&format!(
                    "{:<8} {}  [{}]\n         {}\n",
                    severity.label(),
                    subject(finding, true),
                    finding.check,
                    finding.detail
                ));
            }
        }
        if let Some(names) = self.healthy_tenants() {
            out.push_str(&format!("\nOK: {names}\n"));
        }
        out
    }

    fn counts_line(&self) -> String {
        format!(
            "{}, {}, {} ok; {} tenant{} checked",
            plural(self.count(Severity::Problem), "problem"),
            plural(self.count(Severity::Warning), "warning"),
            self.count(Severity::Ok),
            self.tenants,
            if self.tenants == 1 { "" } else { "s" }
        )
    }

    /// The names of tenants that have findings and no problem or warning among them.
    fn healthy_tenants(&self) -> Option<String> {
        let mut healthy: Vec<&str> = Vec::new();
        for finding in &self.findings {
            let (Some(id), Some(name)) = (&finding.tenant_id, &finding.tenant_name) else {
                continue;
            };
            let all_ok = self
                .findings
                .iter()
                .filter(|other| other.tenant_id.as_ref() == Some(id))
                .all(|other| other.severity == Severity::Ok);
            if all_ok && !healthy.contains(&name.as_str()) {
                healthy.push(name);
            }
        }
        (!healthy.is_empty()).then(|| healthy.join(", "))
    }

    /// The message for Slack and Teams incoming webhooks: counts, then one line per problem. `None`
    /// when there is no problem: an all-clear is never posted.
    ///
    /// Only what the monitor itself composed goes in (tenant names, check names, ages, endpoint
    /// names): no error text from the database, and not the reason the state was unreadable.
    pub fn webhook_text(&self) -> Option<String> {
        let problems: Vec<&Finding> = self
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Problem)
            .collect();
        if problems.is_empty() {
            return None;
        }
        let mut lines = vec![format!(
            "vsv2 monitor: {}, {} ({} tenant{})",
            plural(problems.len(), "problem"),
            plural(self.count(Severity::Warning), "warning"),
            self.tenants,
            if self.tenants == 1 { "" } else { "s" }
        )];
        lines.extend(
            problems
                .iter()
                .take(WEBHOOK_MAX_PROBLEM_LINES)
                .map(|finding| {
                    format!(
                        "- {}: {}: {}",
                        subject(finding, false),
                        finding.check,
                        finding.detail
                    )
                }),
        );
        if problems.len() > WEBHOOK_MAX_PROBLEM_LINES {
            lines.push(format!(
                "... and {} more (run `vsv2 monitor` for the full report)",
                problems.len() - WEBHOOK_MAX_PROBLEM_LINES
            ));
        }
        Some(escape_for_chat(&lines.join("\n")))
    }
}

/// `Name (first8)` for a tenant finding (`Name` alone when `with_id` is false), `pipeline` for one
/// that belongs to no tenant.
fn subject(finding: &Finding, with_id: bool) -> String {
    match (&finding.tenant_name, &finding.tenant_id) {
        (Some(name), Some(id)) if with_id => {
            format!("{name} ({})", id.chars().take(8).collect::<String>())
        }
        (Some(name), _) => name.clone(),
        (None, Some(id)) => format!("tenant {id}"),
        (None, None) => "pipeline".to_string(),
    }
}

fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// Slack reads `<`, `>` and `&` as markup (`<!channel>` pings everyone); Teams takes the same
/// entities. Everything else is plain text in both.
fn escape_for_chat(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `2026-10-13T08:15:03Z`: whole seconds, UTC.
pub fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn serialize_timestamp<S: Serializer>(
    value: &Option<DateTime<Utc>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(at) => serializer.serialize_str(&timestamp(*at)),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests;
