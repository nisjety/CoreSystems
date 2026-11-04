//! The one lint every skill write passes (EP-11 S4).
//!
//! Two services write `agent_skills`: session-core's `UpsertAgentSkill` and
//! capability-core's registry routes. Before this module each accepted whatever
//! text it was handed, so a skill too long to inject, a transcript pasted as a
//! "lesson", or a tool restriction that names no tool could reach the table from
//! one writer and not the other. The rules are written down once, in
//! `api/contracts/authored-behaviour-lint.json`; this is the Rust reading of
//! them and capability-core `internal/skilllint` is the Go one. Both test
//! against the same rule file, its `.sha256` pin and
//! `authored-behaviour-lint-golden.json`, so a rule changed in one language
//! and not the other fails a test.
//!
//! A finding names a rule and a field, never the text, so it can be logged,
//! counted and returned to a caller without echoing the skill back.

/// The switch both writers read. Only `report` and `enforce` turn the lint on.
pub const MODE_ENV: &str = "SKILL_LINT_MODE";

/// The prefix of the `skip_reason` a refused `UpsertAgentSkill` carries; the
/// first error's code follows it.
pub const SKIP_REASON_PREFIX: &str = "skill_lint:";

/// Longest name, in characters.
pub const MAX_NAME_CHARS: usize = 64;
/// Longest description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 300;
/// Longest content a write may store, in characters.
pub const MAX_CONTENT_CHARS: usize = 32_000;
/// The injection budget (`SKILL_CONTEXT_BUDGET_CHARS`): longer content is stored
/// but cut when it is injected, so the author is warned.
pub const CONTENT_BUDGET_CHARS: usize = 8_000;
/// How many lines shaped like chat turns make the content a transcript.
pub const TRANSCRIPT_MIN_LINES: usize = 3;
/// Most trigger keywords on one skill.
pub const MAX_TRIGGER_KEYWORDS: usize = 20;
/// Longest trigger keyword, in characters.
pub const MAX_TRIGGER_KEYWORD_CHARS: usize = 40;

/// The line starts that read as a chat turn rather than a lesson. Case matters:
/// the speaker labels chat exports write are capitalised.
pub const TRANSCRIPT_PREFIXES: &[&str] = &["User:", "Assistant:", "Bruker:"];

/// The closed rule codes, in the order findings are reported.
pub mod code {
    pub const NAME_EMPTY: &str = "name_empty";
    pub const NAME_TOO_LONG: &str = "name_too_long";
    pub const NAME_CHARSET: &str = "name_charset";
    pub const DESCRIPTION_MISSING: &str = "description_missing";
    pub const DESCRIPTION_TOO_LONG: &str = "description_too_long";
    pub const CONTENT_EMPTY: &str = "content_empty";
    pub const CONTENT_TOO_LONG: &str = "content_too_long";
    pub const CONTENT_EXCEEDS_BUDGET: &str = "content_exceeds_budget";
    pub const TRANSCRIPT_SHAPE: &str = "transcript_shape";
    pub const TRIGGER_KEYWORD_COUNT: &str = "trigger_keyword_count";
    pub const TRIGGER_KEYWORD_TOO_LONG: &str = "trigger_keyword_too_long";
    pub const TOOL_RESTRICTION_INVALID: &str = "tool_restriction_invalid";
    pub const FRONTMATTER_NAME_MISMATCH: &str = "frontmatter_name_mismatch";
}

/// Whether a finding refuses a write under `enforce` or only informs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
}

impl Severity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

/// One rule of the contract: its code, the field it reads, its severity and
/// its limit when it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub code: &'static str,
    pub field: &'static str,
    pub severity: Severity,
    pub limit: Option<usize>,
}

const fn rule(
    code: &'static str,
    field: &'static str,
    severity: Severity,
    limit: Option<usize>,
) -> Rule {
    Rule {
        code,
        field,
        severity,
        limit,
    }
}

/// Every rule, in the contract's order.
pub const RULES: &[Rule] = &[
    rule(code::NAME_EMPTY, "name", Severity::Error, None),
    rule(code::NAME_TOO_LONG, "name", Severity::Error, Some(MAX_NAME_CHARS)),
    rule(code::NAME_CHARSET, "name", Severity::Error, None),
    rule(code::DESCRIPTION_MISSING, "description", Severity::Warn, None),
    rule(code::DESCRIPTION_TOO_LONG, "description", Severity::Error, Some(MAX_DESCRIPTION_CHARS)),
    rule(code::CONTENT_EMPTY, "content", Severity::Error, None),
    rule(code::CONTENT_TOO_LONG, "content", Severity::Error, Some(MAX_CONTENT_CHARS)),
    rule(code::CONTENT_EXCEEDS_BUDGET, "content", Severity::Warn, Some(CONTENT_BUDGET_CHARS)),
    rule(code::TRANSCRIPT_SHAPE, "content", Severity::Warn, Some(TRANSCRIPT_MIN_LINES)),
    rule(
        code::TRIGGER_KEYWORD_COUNT,
        "trigger_keywords",
        Severity::Error,
        Some(MAX_TRIGGER_KEYWORDS),
    ),
    rule(
        code::TRIGGER_KEYWORD_TOO_LONG,
        "trigger_keywords",
        Severity::Error,
        Some(MAX_TRIGGER_KEYWORD_CHARS),
    ),
    rule(code::TOOL_RESTRICTION_INVALID, "tool_restrictions", Severity::Error, None),
    rule(code::FRONTMATTER_NAME_MISMATCH, "frontmatter", Severity::Error, None),
];

/// The skill text a write would store. `frontmatter_name` is the `name` of a
/// bundle's `SKILL.md` front matter, present only when a bundle is checked.
#[derive(Clone, Copy, Debug, Default)]
pub struct SkillInput<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub content: &'a str,
    pub trigger_keywords: &'a [String],
    pub tool_restrictions: &'a [String],
    pub frontmatter_name: Option<&'a str>,
}

/// The lint mode a writer runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Nothing runs: a write is exactly what it was before this lint.
    Off,
    /// Findings are counted and logged; every write goes ahead.
    Report,
    /// A write with an error finding is refused; warnings never refuse.
    Enforce,
}

impl Mode {
    /// Only the exact words `report` and `enforce` turn the lint on; anything
    /// else, including an unset variable, leaves it off.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value {
            Some("report") => Self::Report,
            Some("enforce") => Self::Enforce,
            _ => Self::Off,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Report => "report",
            Self::Enforce => "enforce",
        }
    }
}

/// Runs every rule over `input` and returns the rules it breaks, in rule order.
#[must_use]
pub fn lint(input: &SkillInput<'_>) -> Vec<&'static Rule> {
    let mut broken: Vec<&'static str> = Vec::new();

    if input.name.trim().is_empty() {
        broken.push(code::NAME_EMPTY);
    } else {
        if chars(input.name) > MAX_NAME_CHARS {
            broken.push(code::NAME_TOO_LONG);
        }
        if input.name.chars().any(char::is_control) || input.name.trim() != input.name {
            broken.push(code::NAME_CHARSET);
        }
    }

    if input.description.trim().is_empty() {
        broken.push(code::DESCRIPTION_MISSING);
    }
    if chars(input.description) > MAX_DESCRIPTION_CHARS {
        broken.push(code::DESCRIPTION_TOO_LONG);
    }

    let content_chars = chars(input.content);
    if input.content.trim().is_empty() {
        broken.push(code::CONTENT_EMPTY);
    }
    if content_chars > MAX_CONTENT_CHARS {
        broken.push(code::CONTENT_TOO_LONG);
    } else if content_chars > CONTENT_BUDGET_CHARS {
        broken.push(code::CONTENT_EXCEEDS_BUDGET);
    }
    if transcript_lines(input.content) >= TRANSCRIPT_MIN_LINES {
        broken.push(code::TRANSCRIPT_SHAPE);
    }

    if input.trigger_keywords.len() > MAX_TRIGGER_KEYWORDS {
        broken.push(code::TRIGGER_KEYWORD_COUNT);
    }
    if input
        .trigger_keywords
        .iter()
        .any(|keyword| chars(keyword) > MAX_TRIGGER_KEYWORD_CHARS)
    {
        broken.push(code::TRIGGER_KEYWORD_TOO_LONG);
    }
    if input
        .tool_restrictions
        .iter()
        .any(|tool| !is_plain_tool_name(tool))
    {
        broken.push(code::TOOL_RESTRICTION_INVALID);
    }

    if let Some(frontmatter) = input.frontmatter_name {
        if frontmatter.trim() != input.name.trim() {
            broken.push(code::FRONTMATTER_NAME_MISMATCH);
        }
    }

    broken.into_iter().filter_map(rule_for).collect()
}

/// The first error among `findings`: the one a refusal names.
#[must_use]
pub fn first_error<'a>(findings: &[&'a Rule]) -> Option<&'a Rule> {
    findings
        .iter()
        .copied()
        .find(|rule| rule.severity == Severity::Error)
}

/// The rule a code names, or `None` for a code outside the contract.
#[must_use]
pub fn rule_for(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.code == code)
}

/// Length in Unicode scalar values, the unit the contract counts in, so a
/// Norwegian name is not held to a shorter limit than an English one.
fn chars(text: &str) -> usize {
    text.chars().count()
}

fn transcript_lines(content: &str) -> usize {
    content
        .split('\n')
        .map(str::trim_start)
        .filter(|line| TRANSCRIPT_PREFIXES.iter().any(|prefix| line.starts_with(prefix)))
        .count()
}

/// One plain tool name (`[A-Za-z0-9_.-]+`): no wildcard, no space, not empty.
fn is_plain_tool_name(tool: &str) -> bool {
    !tool.is_empty()
        && tool
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    const CONTRACT: &str = include_str!("../../../../api/contracts/authored-behaviour-lint.json");
    const PIN: &str = include_str!("../../../../api/contracts/authored-behaviour-lint.json.sha256");
    const GOLDEN: &str =
        include_str!("../../../../api/contracts/authored-behaviour-lint-golden.json");

    fn json(text: &str) -> Value {
        serde_json::from_str(text).expect("the contract file is JSON")
    }

    fn strings(value: &Value) -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().expect("a list item is text").to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The Go test computes the same digest over the same file, so a rule
    /// changed in the file without both readers being retested fails here and there.
    #[test]
    fn skill_lint_contract_digest_is_the_pinned_one() {
        let normalised = CONTRACT.replace("\r\n", "\n");
        let digest: String = Sha256::digest(normalised.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let pinned = PIN.split_whitespace().next().expect("the pin names a digest");
        assert_eq!(
            digest, pinned,
            "authored-behaviour-lint.json changed without its .sha256 pin being refreshed"
        );
    }

    #[test]
    fn skill_lint_rules_are_the_contract_rules_in_order() {
        let contract = json(CONTRACT);
        let rows = contract["rules"].as_array().expect("rules is a list");
        assert_eq!(rows.len(), RULES.len(), "a rule was added to one side only");
        for (row, rule) in rows.iter().zip(RULES) {
            assert_eq!(row["code"], rule.code);
            assert_eq!(row["field"], rule.field, "field of {}", rule.code);
            assert_eq!(row["severity"], rule.severity.as_str(), "severity of {}", rule.code);
            let limit = row["limit"]
                .as_u64()
                .map(|limit| usize::try_from(limit).expect("a limit fits"));
            assert_eq!(limit, rule.limit, "limit of {}", rule.code);
        }
        assert_eq!(contract["mode"]["env"], MODE_ENV);
        assert_eq!(contract["refusal"]["grpc_skip_reason_prefix"], SKIP_REASON_PREFIX);
        assert_eq!(strings(&contract["transcript_prefixes"]), TRANSCRIPT_PREFIXES);
        assert_eq!(contract["transcript_min_lines"], TRANSCRIPT_MIN_LINES);
        assert_eq!(contract["tool_name_pattern"], "^[A-Za-z0-9_.-]+$");
    }

    #[test]
    fn skill_lint_golden_cases_produce_their_codes() {
        let golden = json(GOLDEN);
        let cases = golden["cases"].as_array().expect("cases is a list");
        assert!(!cases.is_empty());
        for case in cases {
            let id = case["id"].as_str().expect("a case has an id");
            let input = &case["input"];
            let text = |field: &str| -> String {
                if case["repeat"]["field"] == field {
                    let unit = case["repeat"]["text"].as_str().expect("repeat text");
                    let count = case["repeat"]["count"].as_u64().expect("repeat count");
                    unit.repeat(usize::try_from(count).expect("a count fits"))
                } else {
                    input[field].as_str().unwrap_or_default().to_owned()
                }
            };
            let (name, description, content) = (text("name"), text("description"), text("content"));
            let keywords = strings(&input["trigger_keywords"]);
            let tools = strings(&input["tool_restrictions"]);
            let findings = lint(&SkillInput {
                name: &name,
                description: &description,
                content: &content,
                trigger_keywords: &keywords,
                tool_restrictions: &tools,
                frontmatter_name: input["frontmatter_name"].as_str(),
            });
            let got: Vec<&str> = findings.iter().map(|rule| rule.code).collect();
            assert_eq!(got, strings(&case["expect"]), "case {id}");
        }
    }

    #[test]
    fn skill_lint_mode_is_only_the_exact_words() {
        assert_eq!(Mode::from_env_value(Some("report")), Mode::Report);
        assert_eq!(Mode::from_env_value(Some("enforce")), Mode::Enforce);
        for other in [None, Some(""), Some("off"), Some("true"), Some("Enforce"), Some(" report")] {
            assert_eq!(Mode::from_env_value(other), Mode::Off, "{other:?}");
        }
    }

    #[test]
    fn skill_lint_first_error_skips_warnings() {
        let findings = lint(&SkillInput {
            name: "Report",
            content: "User: a\nAssistant: b\nUser: c",
            tool_restrictions: &["*".to_owned()],
            ..SkillInput::default()
        });
        let codes: Vec<&str> = findings.iter().map(|rule| rule.code).collect();
        assert_eq!(
            codes,
            [code::DESCRIPTION_MISSING, code::TRANSCRIPT_SHAPE, code::TOOL_RESTRICTION_INVALID]
        );
        assert_eq!(
            first_error(&findings).map(|rule| rule.code),
            Some(code::TOOL_RESTRICTION_INVALID)
        );
        let warnings_only = lint(&SkillInput {
            name: "Report",
            content: "steps",
            ..SkillInput::default()
        });
        assert!(first_error(&warnings_only).is_none(), "a warning never refuses");
    }
}
