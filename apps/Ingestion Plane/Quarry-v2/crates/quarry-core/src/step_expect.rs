//! Step-level postconditions for a browser action (E4.2 step 1, contract half).
//!
//! The Model proposes an `expect` on a step: a closed, capped list of
//! conditions that must all hold once the step has run. Quarry checks it
//! against the observation it already holds, so a step is verified by what
//! Quarry reads on the page and never by the Model's word. The shape is
//! declarative (ADR 6): no script, no regex, no order-dependent locator.
//!
//! This module holds the types and the bounds check. The evaluator and the
//! wiring live in `quarry-runtime` and `quarry-edge` behind their own switch.

use serde::{Deserialize, Serialize};

use crate::contracts::SemanticLocator;
use crate::error::{ErrorCode, QuarryError, QuarryResult};

/// Most conditions one expectation may carry.
pub const STEP_EXPECT_MAX_CONDITIONS: usize = 8;
/// Longest `url_matches` pattern, in bytes.
pub const STEP_EXPECT_PATTERN_MAX_BYTES: usize = 512;
/// Longest `text_present` or `text_absent` text, in bytes.
pub const STEP_EXPECT_TEXT_MAX_BYTES: usize = 256;
/// Longest string inside a locator, in bytes.
pub const STEP_EXPECT_LOCATOR_MAX_BYTES: usize = 256;

/// A declarative postcondition for one browser step. It holds when every
/// condition in `all` holds (AND). Unknown fields are refused rather than
/// ignored, so a caller cannot add a mode or a regex that Quarry would drop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StepExpectation {
    pub all: Vec<ExpectCondition>,
}

/// One checkable condition, tagged by `kind` on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectCondition {
    /// The URL matches `pattern`. `*` is the only metacharacter; every other
    /// byte is literal.
    UrlMatches { pattern: String },
    /// At least one element of the current snapshot satisfies `locator`.
    ElementPresent { locator: SemanticLocator },
    /// No element of the current snapshot satisfies `locator`.
    ElementAbsent { locator: SemanticLocator },
    /// `text` appears in the page text or, with `within`, in the element that
    /// `within` names.
    TextPresent {
        text: String,
        #[serde(default)]
        within: Option<SemanticLocator>,
    },
    /// `text` does not appear in the page text or, with `within`, in the
    /// element that `within` names.
    TextAbsent {
        text: String,
        #[serde(default)]
        within: Option<SemanticLocator>,
    },
}

/// What Quarry found when it checked a [`StepExpectation`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ExpectVerdict {
    /// Every condition holds.
    Met,
    /// At least one condition does not hold. `unmet` lists the zero-based
    /// indexes into `all` of the conditions that do not.
    NotMet { unmet: Vec<u32> },
    /// Quarry could not check the expectation. `reason` is a closed token,
    /// such as `no_snapshot` or `no_page_text`.
    Unevaluable { reason: String },
}

/// The expectation a step carried and the verdict Quarry reached on it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExpectationReport {
    pub expect: StepExpectation,
    pub verdict: ExpectVerdict,
}

impl StepExpectation {
    /// Checks the bounds a proposed expectation must meet before Quarry checks
    /// it: 1 to [`STEP_EXPECT_MAX_CONDITIONS`] conditions; each URL pattern and
    /// each text 1 to its byte cap; each locator string 1 to
    /// [`STEP_EXPECT_LOCATOR_MAX_BYTES`]; and no `nth` locator, whose match
    /// depends on page order and cannot be replayed. A violation is
    /// `BAD_REQUEST` with `details.field` naming the path of the field. Neither
    /// the message nor the details carry the proposed text.
    pub fn validate(&self) -> QuarryResult<()> {
        if self.all.is_empty() || self.all.len() > STEP_EXPECT_MAX_CONDITIONS {
            return Err(refuse(
                "all",
                format!("a step expectation takes 1 to {STEP_EXPECT_MAX_CONDITIONS} conditions"),
            ));
        }
        for (index, condition) in self.all.iter().enumerate() {
            condition.validate_at(&format!("all[{index}]"))?;
        }
        Ok(())
    }
}

impl ExpectCondition {
    /// Checks one condition. `path` is the field path of the condition in the
    /// request, used only to name a refused field.
    fn validate_at(&self, path: &str) -> QuarryResult<()> {
        match self {
            Self::UrlMatches { pattern } => check_bounded(
                format!("{path}.pattern"),
                pattern,
                STEP_EXPECT_PATTERN_MAX_BYTES,
            ),
            Self::ElementPresent { locator } | Self::ElementAbsent { locator } => {
                validate_locator(&format!("{path}.locator"), locator)
            }
            Self::TextPresent { text, within } | Self::TextAbsent { text, within } => {
                check_bounded(format!("{path}.text"), text, STEP_EXPECT_TEXT_MAX_BYTES)?;
                match within {
                    Some(locator) => validate_locator(&format!("{path}.within"), locator),
                    None => Ok(()),
                }
            }
        }
    }
}

fn validate_locator(path: &str, locator: &SemanticLocator) -> QuarryResult<()> {
    let max = STEP_EXPECT_LOCATOR_MAX_BYTES;
    match locator {
        SemanticLocator::Role { role, name, .. } => {
            check_bounded(format!("{path}.role"), role, max)?;
            match name {
                Some(name) => check_bounded(format!("{path}.name"), name, max),
                None => Ok(()),
            }
        }
        SemanticLocator::Text { text, .. } => check_bounded(format!("{path}.text"), text, max),
        SemanticLocator::Label { label, .. } => check_bounded(format!("{path}.label"), label, max),
        SemanticLocator::Placeholder { placeholder, .. } => {
            check_bounded(format!("{path}.placeholder"), placeholder, max)
        }
        SemanticLocator::TestId { test_id, .. } => {
            check_bounded(format!("{path}.test_id"), test_id, max)
        }
        SemanticLocator::Nth { .. } => Err(refuse(
            path,
            "a locator of kind nth is order-dependent and cannot be replayed; use role, text, label, placeholder or test_id",
        )),
    }
}

/// Refuses a string that is empty or longer than its byte cap.
fn check_bounded(field: impl Into<String>, value: &str, max_bytes: usize) -> QuarryResult<()> {
    if value.is_empty() || value.len() > max_bytes {
        return Err(refuse(
            field,
            format!("a step expectation string must be 1 to {max_bytes} bytes"),
        ));
    }
    Ok(())
}

/// A `BAD_REQUEST` that names the refused field by its path and never a value.
fn refuse(field: impl Into<String>, message: impl Into<String>) -> QuarryError {
    let field: String = field.into();
    QuarryError::new(ErrorCode::BadRequest, message)
        .with_details(serde_json::json!({ "field": field }))
}
