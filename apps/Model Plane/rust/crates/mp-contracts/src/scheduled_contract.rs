//! The stored-consent contract a scheduled Do run may use without a user present
//! (DEC-16, amended not reversed). The contract is read-only in v1: it names the
//! tools the owner approved, each under the class it was approved as, and an
//! expiry. It is stored as JSON text under `approval_contract` in the schedule's
//! task template, because storage keeps a string verbatim and would re-order a
//! nested object. Control signs the digest of that text; capability-core and the
//! gateway recompute it. This module is the Rust twin of
//! `capability-core/internal/cron/contract.go`, and both are pinned by one vector.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The contract version this build understands; any other `v` is refused.
pub const CONTRACT_VERSION: u32 = 1;
/// The highest class a stored consent may carry in v1.
pub const CLASS_CEILING: &str = "read";
pub const MAX_TOOLS: usize = 16;
pub const MAX_ROUNDS: u32 = 8;
pub const MAX_TOKENS_PER_ROUND: u32 = 4096;

/// Exact tool names that are never delegable, whatever their registry row says.
const NON_DELEGABLE_EXACT: &[&str] = &[
    "browser_agent",
    "code_interpreter",
    "shell",
    "tickets.create",
    "book_shipment",
    "memory.write",
    "skill.write",
    "schedule.create",
    "schedule.patch",
    "schedule.delete",
];

/// Tool families that are never delegable: browser, process and every MCP tool.
const NON_DELEGABLE_PREFIXES: &[&str] = &["browser.", "process_", "mcp__"];

/// The parsed `approval_contract`. Unknown keys are refused so the contract can
/// only carry fields this build validates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledContract {
    pub v: u32,
    pub tools: Vec<ContractTool>,
    pub expires_at: String,
    pub max_rounds: u32,
    pub max_tokens_per_round: u32,
}

/// One approved tool, with the class it was approved under and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractTool {
    pub tool: String,
    pub capability: String,
    pub class: String,
    pub approved_at: String,
}

/// Closed refusal tokens. They name a reason and never echo the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractRefusal {
    Malformed,
    UnsupportedVersion,
    ToolCount,
    ToolIdentifier,
    DuplicateTool,
    ClassAboveCeiling,
    NonDelegableTool,
    TimeFormat,
    Limit,
}

impl ContractRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "scheduled_contract_malformed",
            Self::UnsupportedVersion => "scheduled_contract_version",
            Self::ToolCount => "scheduled_contract_tool_count",
            Self::ToolIdentifier => "scheduled_contract_tool_identifier",
            Self::DuplicateTool => "scheduled_contract_duplicate_tool",
            Self::ClassAboveCeiling => "scheduled_contract_class_above_ceiling",
            Self::NonDelegableTool => "scheduled_contract_non_delegable_tool",
            Self::TimeFormat => "scheduled_contract_time_format",
            Self::Limit => "scheduled_contract_limit",
        }
    }
}

/// Whether a declared class may be approved in a stored consent. v1 admits
/// `read` only, so a contract can never authorise an effect.
pub fn delegable(class: &str) -> bool {
    class == CLASS_CEILING
}

/// Whether a tool may never be part of a stored consent.
pub fn non_delegable_tool(name: &str) -> bool {
    NON_DELEGABLE_EXACT.contains(&name)
        || NON_DELEGABLE_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

impl ScheduledContract {
    /// Parses an `approval_contract` text strictly, then validates it. Trailing
    /// data is refused by the JSON parser itself.
    pub fn parse(text: &str) -> Result<Self, ContractRefusal> {
        let contract: Self = serde_json::from_str(text).map_err(|_| ContractRefusal::Malformed)?;
        contract.validate()?;
        Ok(contract)
    }

    /// Checks shape and bounds. Registry and window checks live with the callers
    /// that hold the registry and the clock.
    pub fn validate(&self) -> Result<(), ContractRefusal> {
        if self.v != CONTRACT_VERSION {
            return Err(ContractRefusal::UnsupportedVersion);
        }
        if self.tools.is_empty() || self.tools.len() > MAX_TOOLS {
            return Err(ContractRefusal::ToolCount);
        }
        let mut seen: Vec<&str> = Vec::with_capacity(self.tools.len());
        for tool in &self.tools {
            if !identifier_ok(&tool.tool) || !identifier_ok(&tool.capability) {
                return Err(ContractRefusal::ToolIdentifier);
            }
            if seen.contains(&tool.tool.as_str()) {
                return Err(ContractRefusal::DuplicateTool);
            }
            seen.push(&tool.tool);
            if non_delegable_tool(&tool.tool) {
                return Err(ContractRefusal::NonDelegableTool);
            }
            if !delegable(&tool.class) {
                return Err(ContractRefusal::ClassAboveCeiling);
            }
            if !canonical_utc(&tool.approved_at) {
                return Err(ContractRefusal::TimeFormat);
            }
        }
        if !canonical_utc(&self.expires_at) {
            return Err(ContractRefusal::TimeFormat);
        }
        if self.max_rounds < 1 || self.max_rounds > MAX_ROUNDS {
            return Err(ContractRefusal::Limit);
        }
        if self.max_tokens_per_round < 1 || self.max_tokens_per_round > MAX_TOKENS_PER_ROUND {
            return Err(ContractRefusal::Limit);
        }
        Ok(())
    }
}

/// `sha256:<hex>` over the exact UTF-8 bytes of the stored `approval_contract`
/// text. The bytes, not a re-serialisation, are what Go and the gateway hash too.
pub fn digest(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity("sha256:".len() + 64);
    out.push_str("sha256:");
    for byte in hash {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn identifier_ok(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-'))
}

/// Accepts only `YYYY-MM-DDTHH:MM:SSZ`, one spelling per instant, matching the
/// Go side's canonical check. Field ranges are checked, not calendar validity.
fn canonical_utc(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if !(digits(0..4)
        && digits(5..7)
        && digits(8..10)
        && digits(11..13)
        && digits(14..16)
        && digits(17..19))
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> u32 {
        raw[range].parse::<u32>().unwrap_or(u32::MAX)
    };
    (1..=12).contains(&number(5..7))
        && (1..=31).contains(&number(8..10))
        && number(11..13) <= 23
        && number(14..16) <= 59
        && number(17..19) <= 60
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vector shared with capability-core's contract_test.go. Keep the two in step.
    const VECTOR: &str = r#"{"expires_at":"2026-11-23T10:00:00Z","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}"#;
    const GO_VECTOR_DIGEST: &str =
        "sha256:3d4828c85b976a25447098498e5077de75aa531a0e64aebe2d68bfc8f7953d55";

    fn with(from: &str, to: &str) -> String {
        assert!(VECTOR.contains(from), "vector lacks {from}");
        VECTOR.replacen(from, to, 1)
    }

    #[test]
    fn valid_vector_parses_and_validates() {
        let contract = ScheduledContract::parse(VECTOR).expect("vector is valid");
        assert_eq!(contract.tools.len(), 1);
        assert_eq!(contract.max_rounds, 6);
    }

    #[test]
    fn digest_is_pinned_to_the_exact_text() {
        assert_eq!(digest(VECTOR), GO_VECTOR_DIGEST);
        let reordered = with(
            r#""max_rounds":6,"max_tokens_per_round":2048"#,
            r#""max_tokens_per_round":2048,"max_rounds":6"#,
        );
        assert_ne!(digest(&reordered), GO_VECTOR_DIGEST);
    }

    #[test]
    fn unknown_keys_and_trailing_data_are_refused() {
        assert_eq!(
            ScheduledContract::parse(&with(r#""v":1"#, r#""v":1,"extra":true"#)),
            Err(ContractRefusal::Malformed)
        );
        assert_eq!(
            ScheduledContract::parse(&format!("{VECTOR} {{}}")),
            Err(ContractRefusal::Malformed)
        );
    }

    #[test]
    fn version_tool_count_and_duplicates_are_refused() {
        assert_eq!(
            ScheduledContract::parse(&with(r#""v":1"#, r#""v":2"#)),
            Err(ContractRefusal::UnsupportedVersion)
        );
        let empty = with(
            r#""tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}]"#,
            r#""tools":[]"#,
        );
        assert_eq!(ScheduledContract::parse(&empty), Err(ContractRefusal::ToolCount));
        let tool = r#"{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}"#;
        let duplicate = with(
            &format!("[{tool}]"),
            &format!("[{tool},{tool}]"),
        );
        assert_eq!(
            ScheduledContract::parse(&duplicate),
            Err(ContractRefusal::DuplicateTool)
        );
    }

    #[test]
    fn class_above_ceiling_and_non_delegable_tools_are_refused() {
        assert_eq!(
            ScheduledContract::parse(&with(r#""class":"read""#, r#""class":"external_write""#)),
            Err(ContractRefusal::ClassAboveCeiling)
        );
        for name in ["browser.open", "mcp__github__search", "shell", "process_start"] {
            let value = with(r#""tool":"web_search""#, &format!(r#""tool":"{name}""#));
            assert_eq!(
                ScheduledContract::parse(&value),
                Err(ContractRefusal::NonDelegableTool),
                "{name} must not be delegable"
            );
        }
    }

    #[test]
    fn limits_and_time_format_are_refused() {
        assert_eq!(
            ScheduledContract::parse(&with(r#""max_rounds":6"#, r#""max_rounds":9"#)),
            Err(ContractRefusal::Limit)
        );
        assert_eq!(
            ScheduledContract::parse(&with(
                r#""max_tokens_per_round":2048"#,
                r#""max_tokens_per_round":4097"#
            )),
            Err(ContractRefusal::Limit)
        );
        assert_eq!(
            ScheduledContract::parse(&with(
                r#""expires_at":"2026-11-23T10:00:00Z""#,
                r#""expires_at":"2026-11-23T10:00:00+00:00""#
            )),
            Err(ContractRefusal::TimeFormat)
        );
    }

    #[test]
    fn delegable_admits_read_only() {
        assert!(delegable("read"));
        for class in ["sandbox_compute", "context_write", "external_write", "unclassified", ""] {
            assert!(!delegable(class), "{class} must not be delegable");
        }
    }
}
