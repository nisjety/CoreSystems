//! The saved recipe: a stored browser procedure the Model Plane replays one step at a time.
//!
//! This module is pure. It reads the version 2 procedure that Quarry compiled, binds the typed
//! text slots a person supplies, resolves each semantic target against the latest wire snapshot,
//! and proposes one [`BrowserAction`] per step, with the form `replay-check` compares. It does no
//! I/O and keeps nothing. Quarry still executes every step and re-verifies the ref and the
//! fingerprint, so a locator that resolves here can still be refused there.
//!
//! The artifact is a recipe, to keep it apart from `procedure.rs`, which holds the planner's
//! standing instructions (`browse/1`).
//!
//! [`resolve_locator`] mirrors Quarry's `semantic_matches` (`observation.rs` in quarry-runtime): a
//! value is trimmed and compared ASCII case-insensitively, exactly or as a substring, and `nth`
//! never matches. The two change together.
//!
//! A secret slot is refused. A person supplies a secret by takeover; a model never does.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

use super::{ActionType, BrowserAction, BrowserTargetRef};
use crate::quarry_agent::WireSnapshotTarget;

/// The only recipe version read: Quarry's canonical (version 2) procedure.
pub const RECIPE_VERSION: u32 = 2;
/// The most steps a recipe may hold.
pub const MAX_STEPS: usize = 100;
/// The longest value a slot may take, in characters.
pub const MAX_SLOT_CHARS: usize = 2_000;

/// The action types a recipe step may name: the closed vocabulary of a version 2 procedure.
const VOCABULARY: [&str; 12] = [
    "navigate",
    "click",
    "click_semantic",
    "type",
    "type_semantic",
    "select_semantic",
    "wait_for_semantic",
    "press",
    "scroll",
    "wait",
    "back",
    "get_content",
];

/// The kinds that name their target by locator, so they act on the latest snapshot.
const SEMANTIC_KINDS: [&str; 4] = [
    "click_semantic",
    "type_semantic",
    "select_semantic",
    "wait_for_semantic",
];

/// The kinds that type into a slot.
const TYPING_KINDS: [&str; 2] = ["type", "type_semantic"];

/// A saved recipe, as read from the stored procedure JSON. Tolerant: a field this module does not
/// use is ignored, and an optional field takes its default.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Recipe {
    #[serde(default)]
    pub procedure_id: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub steps: Vec<RecipeStep>,
    #[serde(default)]
    pub slots: Vec<RecipeSlot>,
}

/// One stored step. `action` is Quarry's wire form of the action, such as
/// `{"type": "click_semantic", "locator": {...}}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecipeStep {
    pub step: u32,
    pub action: Value,
    #[serde(default)]
    pub expected_outcome: RecipeOutcome,
    /// The postcondition Quarry verified when the recipe was compiled, if any.
    #[serde(default)]
    pub expect: Option<Value>,
    #[serde(default)]
    pub target: Option<RecipeTarget>,
    /// The slot this step types into.
    #[serde(default)]
    pub slot: Option<String>,
}

/// The element a semantic step acts on: the locator it resolves by, and the fingerprint identity
/// Quarry matched when the recipe was compiled.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RecipeTarget {
    pub locator: Value,
    #[serde(default)]
    pub fingerprint_id: String,
    #[serde(default)]
    pub tag: String,
}

/// A named input a person supplies at replay.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecipeSlot {
    pub name: String,
    pub step: u32,
    pub kind: RecipeSlotKind,
}

/// Whether a slot takes ordinary text or a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeSlotKind {
    Text,
    /// A password, code or token. [`bind_slots`] refuses it.
    Secret,
}

/// The outcome Quarry recorded for a step when the recipe was compiled. An unrecognised value
/// reads as `Unknown`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeOutcome {
    Verified,
    Failed,
    #[default]
    #[serde(other)]
    Unknown,
}

impl Recipe {
    /// Reads a recipe from its stored JSON.
    ///
    /// # Errors
    ///
    /// Refuses bytes that are not a recipe, a version other than 2, a step count outside
    /// `1..=MAX_STEPS`, an action type outside the vocabulary, and slots that do not line up with
    /// the steps that type into them.
    pub fn parse(bytes: &[u8]) -> Result<Self, RecipeRefusal> {
        let recipe: Self = serde_json::from_slice(bytes).map_err(|_| RecipeRefusal::Malformed)?;
        if recipe.version != RECIPE_VERSION {
            return Err(RecipeRefusal::UnsupportedVersion);
        }
        if recipe.steps.is_empty() || recipe.steps.len() > MAX_STEPS {
            return Err(RecipeRefusal::StepCount);
        }
        for step in &recipe.steps {
            match step.kind() {
                Some(kind) if VOCABULARY.contains(&kind) => {}
                _ => return Err(RecipeRefusal::UnsupportedAction(step.step)),
            }
        }
        recipe.check_slots()?;
        Ok(recipe)
    }

    /// Step numbers and slot names are each unique. A typing step names a slot of its own number,
    /// and every slot is named by the step it belongs to. No other step carries a slot.
    fn check_slots(&self) -> Result<(), RecipeRefusal> {
        let mut numbers = BTreeSet::new();
        for step in &self.steps {
            if !numbers.insert(step.step) {
                return Err(RecipeRefusal::Malformed);
            }
            let typing = step.kind().is_some_and(|kind| TYPING_KINDS.contains(&kind));
            if typing != step.slot.is_some() {
                return Err(RecipeRefusal::Malformed);
            }
            if let Some(name) = &step.slot {
                let declared = self
                    .slots
                    .iter()
                    .any(|slot| &slot.name == name && slot.step == step.step);
                if !declared {
                    return Err(RecipeRefusal::Malformed);
                }
            }
        }
        let mut names = BTreeSet::new();
        for slot in &self.slots {
            if !names.insert(slot.name.as_str()) {
                return Err(RecipeRefusal::Malformed);
            }
            let owned = self.steps.iter().any(|step| {
                step.step == slot.step && step.slot.as_deref() == Some(slot.name.as_str())
            });
            if !owned {
                return Err(RecipeRefusal::Malformed);
            }
        }
        Ok(())
    }
}

impl RecipeStep {
    /// The action's `type`, when the action names one.
    #[must_use]
    pub fn kind(&self) -> Option<&str> {
        self.action.get("type").and_then(Value::as_str)
    }

    /// Whether the step names its target by locator, so it needs the latest snapshot.
    #[must_use]
    pub fn needs_snapshot(&self) -> bool {
        self.kind()
            .is_some_and(|kind| SEMANTIC_KINDS.contains(&kind))
    }
}

/// A recipe with every slot bound to a value a person supplied. It holds typed text, so its
/// `Debug` output shows counts only.
pub struct BoundRecipe {
    recipe: Recipe,
    values: BTreeMap<String, String>,
}

impl BoundRecipe {
    /// The recipe as it was read.
    #[must_use]
    pub fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    /// The bound text of a step's slot. `None` when the step types nothing.
    #[must_use]
    pub fn text_for(&self, step: &RecipeStep) -> Option<&str> {
        step.slot
            .as_deref()
            .and_then(|name| self.values.get(name))
            .map(String::as_str)
    }
}

impl std::fmt::Debug for BoundRecipe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundRecipe")
            .field("procedure_id", &self.recipe.procedure_id)
            .field("steps", &self.recipe.steps.len())
            .field("bound_slots", &self.values.len())
            .finish()
    }
}

/// Binds a person's values to a recipe's slots.
///
/// # Errors
///
/// Refuses a recipe with a secret slot, whatever is bound; a slot with no value; a value for a
/// name the recipe does not declare; and a value over [`MAX_SLOT_CHARS`] or with a control
/// character.
pub fn bind_slots(
    recipe: &Recipe,
    values: &BTreeMap<String, String>,
) -> Result<BoundRecipe, RecipeRefusal> {
    if recipe
        .slots
        .iter()
        .any(|slot| slot.kind == RecipeSlotKind::Secret)
    {
        return Err(RecipeRefusal::SecretSlot);
    }
    if values
        .keys()
        .any(|key| !recipe.slots.iter().any(|slot| &slot.name == key))
    {
        return Err(RecipeRefusal::UnknownSlot);
    }
    let mut bound = BTreeMap::new();
    for slot in &recipe.slots {
        let value = values.get(&slot.name).ok_or(RecipeRefusal::SlotUnbound)?;
        if value.chars().count() > MAX_SLOT_CHARS || value.chars().any(char::is_control) {
            return Err(RecipeRefusal::SlotValueInvalid);
        }
        bound.insert(slot.name.clone(), value.clone());
    }
    Ok(BoundRecipe {
        recipe: recipe.clone(),
        values: bound,
    })
}

/// The result of resolving a locator against one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocatorMatch {
    /// Exactly one target matched, and it has a ref to act on.
    One(String),
    /// No target matched, the locator is not one this module reads, or the one match has no ref.
    None,
    /// More than one target matched.
    Ambiguous(usize),
}

/// Resolves a semantic locator against the targets of the latest wire snapshot.
#[must_use]
pub fn resolve_locator(targets: &[WireSnapshotTarget], locator: &Value) -> LocatorMatch {
    match locate(targets, locator) {
        Hit::One(target) if !target.ref_id.is_empty() => LocatorMatch::One(target.ref_id.clone()),
        Hit::One(_) | Hit::Missing => LocatorMatch::None,
        Hit::Ambiguous(count) => LocatorMatch::Ambiguous(count),
    }
}

/// A locator as Quarry writes it, tagged by `kind`. `nth`, and any shape this module does not
/// read, does not parse, and so matches nothing.
enum Locator<'a> {
    Role {
        role: &'a str,
        name: Option<&'a str>,
        exact: bool,
    },
    Text {
        text: &'a str,
        exact: bool,
    },
    Label {
        label: &'a str,
        exact: bool,
    },
    Placeholder {
        placeholder: &'a str,
        exact: bool,
    },
    TestId {
        test_id: &'a str,
        exact: bool,
    },
}

impl<'a> Locator<'a> {
    fn read(value: &'a Value) -> Option<Self> {
        let exact = match value.get("exact") {
            None => false,
            Some(flag) => flag.as_bool()?,
        };
        match str_field(value, "kind")? {
            "role" => Some(Self::Role {
                role: str_field(value, "role")?,
                name: match value.get("name") {
                    None | Some(Value::Null) => None,
                    Some(name) => Some(name.as_str()?),
                },
                exact,
            }),
            "text" => Some(Self::Text {
                text: str_field(value, "text")?,
                exact,
            }),
            "label" => Some(Self::Label {
                label: str_field(value, "label")?,
                exact,
            }),
            "placeholder" => Some(Self::Placeholder {
                placeholder: str_field(value, "placeholder")?,
                exact,
            }),
            "test_id" => Some(Self::TestId {
                test_id: str_field(value, "test_id")?,
                exact,
            }),
            _ => None,
        }
    }

    /// Quarry's `semantic_matches` over one target.
    fn matches(&self, target: &WireSnapshotTarget) -> bool {
        match *self {
            Self::Role { role, name, exact } => {
                let name_matches = match name {
                    Some(name) => value_matches(target.name.as_deref(), name, exact),
                    None => true,
                };
                value_matches(target.role.as_deref(), role, exact) && name_matches
            }
            Self::Text { text, exact } => value_matches(target.text.as_deref(), text, exact),
            Self::Label { label, exact } => value_matches(target.name.as_deref(), label, exact),
            Self::Placeholder { placeholder, exact } => {
                value_matches(target.placeholder.as_deref(), placeholder, exact)
            }
            Self::TestId { test_id, exact } => {
                value_matches(target.test_id.as_deref(), test_id, exact)
            }
        }
    }
}

/// What a locator matched, with the target itself.
enum Hit<'a> {
    One(&'a WireSnapshotTarget),
    Missing,
    Ambiguous(usize),
}

fn locate<'a>(targets: &'a [WireSnapshotTarget], locator: &Value) -> Hit<'a> {
    let Some(locator) = Locator::read(locator) else {
        return Hit::Missing;
    };
    let hits: Vec<&'a WireSnapshotTarget> = targets
        .iter()
        .filter(|target| locator.matches(target))
        .collect();
    match hits.as_slice() {
        [] => Hit::Missing,
        [only] => Hit::One(*only),
        many => Hit::Ambiguous(many.len()),
    }
}

/// Quarry's `value_matches`: the trimmed, non-empty value against the trimmed needle, by ASCII
/// case-insensitive equality when `exact`, and otherwise by substring.
fn value_matches(value: Option<&str>, needle: &str, exact: bool) -> bool {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let needle = needle.trim();
    if exact {
        value.eq_ignore_ascii_case(needle)
    } else {
        value
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    }
}

/// One proposed step: the action for the loop, the form `replay-check` compares, and the
/// postcondition the step carries.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub action: BrowserAction,
    /// The stored action with this run's snapshot in it, and the bound text of a typing step.
    /// Quarry compares it with the stored step and drops the handles and the slot text.
    pub check_json: Value,
    /// The postcondition for the loop to hand to Quarry with the step.
    pub expect: Option<Value>,
}

/// Proposes the action for one recipe step against the latest snapshot.
///
/// `text` is the bound value of the step's slot, from [`BoundRecipe::text_for`]. A semantic step
/// resolves its locator against `targets`, and the matched ref and frame go into the action with
/// the fresh `snapshot_id` and `generation`. `check_json` is the stored action with the same fresh
/// handles, which Quarry's replay check compares with the stored step.
///
/// # Errors
///
/// [`StepRefusal::TargetRepairRequired`] when the locator matches no target with a ref, or more
/// than one. [`StepRefusal::SlotUnbound`] when a typing step has no text. [`StepRefusal::Malformed`]
/// and [`StepRefusal::UnsupportedAction`] for a step this module cannot read.
pub fn proposal_for_step(
    step: &RecipeStep,
    text: Option<&str>,
    snapshot_id: &str,
    generation: u32,
    targets: &[WireSnapshotTarget],
    grant_id: &str,
) -> Result<Proposal, StepRefusal> {
    let kind = step.kind().ok_or(StepRefusal::Malformed)?;
    let action_type = loop_action_type(kind).ok_or(StepRefusal::UnsupportedAction)?;
    let stored = &step.action;
    let mut action = BrowserAction {
        action_id: format!("recipe-{}", step.step),
        grant_id: grant_id.to_owned(),
        action_type,
        selector: String::new(),
        value: String::new(),
        url: String::new(),
        max_wait_ms: 0,
        target: None,
        reason: format!("recipe step {}", step.step),
        risk_category: None,
    };
    if SEMANTIC_KINDS.contains(&kind) {
        let locator = stored.get("locator").ok_or(StepRefusal::Malformed)?;
        if step
            .target
            .as_ref()
            .is_some_and(|target| target.locator != *locator)
        {
            return Err(StepRefusal::Malformed);
        }
        let hit = match locate(targets, locator) {
            Hit::One(hit) if !hit.ref_id.is_empty() => hit,
            _ => return Err(StepRefusal::TargetRepairRequired),
        };
        action.target = Some(BrowserTargetRef {
            snapshot_id: snapshot_id.to_owned(),
            generation,
            ref_id: hit.ref_id.clone(),
            frame_id: hit
                .frame_id
                .clone()
                .filter(|frame| !frame.trim().is_empty()),
        });
    }
    match kind {
        "navigate" => action.url = required(stored, "url")?,
        "click" => action.selector = required(stored, "selector")?,
        "type" => {
            action.selector = required(stored, "selector")?;
            action.value = bound_text(text)?;
        }
        "type_semantic" => action.value = bound_text(text)?,
        "select_semantic" => action.value = required(stored, "value")?,
        "wait_for_semantic" => action.max_wait_ms = millis(stored, "timeout_ms")?,
        "press" => action.value = required(stored, "key")?,
        "scroll" => action.selector = required(stored, "target")?,
        "wait" => action.max_wait_ms = millis(stored, "ms")?,
        "click_semantic" | "back" | "get_content" => {}
        _ => return Err(StepRefusal::UnsupportedAction),
    }
    let mut check_json = stored.clone();
    if let Some(fields) = check_json.as_object_mut() {
        if SEMANTIC_KINDS.contains(&kind) {
            fields.insert(
                "snapshot_id".to_owned(),
                Value::String(snapshot_id.to_owned()),
            );
            fields.insert("generation".to_owned(), Value::from(generation));
        }
        if TYPING_KINDS.contains(&kind) {
            fields.insert("text".to_owned(), Value::String(action.value.clone()));
        }
    }
    Ok(Proposal {
        action,
        check_json,
        expect: step.expect.clone(),
    })
}

/// The loop's verb for a recipe action type.
fn loop_action_type(kind: &str) -> Option<ActionType> {
    Some(match kind {
        "navigate" => ActionType::Goto,
        "click" | "click_semantic" => ActionType::Click,
        "type" | "type_semantic" => ActionType::Type,
        "select_semantic" => ActionType::Select,
        "wait_for_semantic" => ActionType::WaitFor,
        "press" => ActionType::Press,
        "scroll" => ActionType::Scroll,
        "wait" => ActionType::Wait,
        "back" => ActionType::Back,
        "get_content" => ActionType::Extract,
        _ => return None,
    })
}

/// A string field of a JSON object.
fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str()
}

/// A required string field of a stored action.
fn required(stored: &Value, key: &str) -> Result<String, StepRefusal> {
    str_field(stored, key)
        .map(str::to_owned)
        .ok_or(StepRefusal::Malformed)
}

/// A required millisecond field of a stored action, as the `i32` the loop carries.
fn millis(stored: &Value, key: &str) -> Result<i32, StepRefusal> {
    stored
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|ms| i32::try_from(ms).ok())
        .ok_or(StepRefusal::Malformed)
}

/// The bound text a typing step types.
fn bound_text(text: Option<&str>) -> Result<String, StepRefusal> {
    text.map(str::to_owned).ok_or(StepRefusal::SlotUnbound)
}

/// Why a recipe, or its slot bindings, was refused. Closed, and it names no step content or value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeRefusal {
    /// The bytes are not a recipe this module reads.
    Malformed,
    /// The version is not [`RECIPE_VERSION`].
    UnsupportedVersion,
    /// The recipe holds no steps, or more than [`MAX_STEPS`].
    StepCount,
    /// A step's action type is outside the vocabulary. Carries the step number.
    UnsupportedAction(u32),
    /// A slot has no bound value.
    SlotUnbound,
    /// A value is bound to a name the recipe does not declare.
    UnknownSlot,
    /// The recipe has a secret slot. A model never fills one.
    SecretSlot,
    /// A bound value is over [`MAX_SLOT_CHARS`] or holds a control character.
    SlotValueInvalid,
}

impl RecipeRefusal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnsupportedVersion => "unsupported_version",
            Self::StepCount => "step_count",
            Self::UnsupportedAction(_) => "unsupported_action",
            Self::SlotUnbound => "slot_unbound",
            Self::UnknownSlot => "unknown_slot",
            Self::SecretSlot => "secret_slot",
            Self::SlotValueInvalid => "slot_value_invalid",
        }
    }
}

/// Why one step could not be proposed. Closed; it names no page content and no typed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepRefusal {
    /// The locator matches no target with a ref, or matches more than one. The step needs repair.
    TargetRepairRequired,
    /// A field the step needs is missing or has the wrong type, or its locator disagrees with its
    /// target.
    Malformed,
    /// The action type is outside the vocabulary.
    UnsupportedAction,
    /// A typing step has no bound text.
    SlotUnbound,
}

impl StepRefusal {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TargetRepairRequired => "target_repair_required",
            Self::Malformed => "malformed",
            Self::UnsupportedAction => "unsupported_action",
            Self::SlotUnbound => "slot_unbound",
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn target(ref_id: &str, role: &str, name: &str) -> WireSnapshotTarget {
        WireSnapshotTarget {
            ref_id: ref_id.to_owned(),
            role: Some(role.to_owned()),
            name: Some(name.to_owned()),
            ..WireSnapshotTarget::default()
        }
    }

    fn recipe_json(steps: Value, slots: Value) -> Value {
        json!({ "procedure_id": "proc-1", "version": 2, "steps": steps, "slots": slots })
    }

    fn parse_value(value: &Value) -> Result<Recipe, RecipeRefusal> {
        Recipe::parse(value.to_string().as_bytes())
    }

    fn plain(step: &RecipeStep) -> Result<Proposal, StepRefusal> {
        proposal_for_step(step, None, "snap-1", 1, &[], "grant-1")
    }

    /// Navigate, type into a search box (slot 2), then click the search button.
    fn sample() -> Value {
        recipe_json(
            json!([
                {
                    "step": 1,
                    "action": { "type": "navigate", "url": "https://example.com/" },
                    "expected_outcome": "verified"
                },
                {
                    "step": 2,
                    "action": {
                        "type": "type_semantic",
                        "snapshot_id": "",
                        "generation": 0,
                        "locator": { "kind": "placeholder", "placeholder": "Search", "exact": true },
                        "text": ""
                    },
                    "expected_outcome": "verified",
                    "target": {
                        "locator": { "kind": "placeholder", "placeholder": "Search", "exact": true },
                        "fingerprint_id": "fp-2",
                        "tag": "input"
                    },
                    "slot": "slot_2"
                },
                {
                    "step": 3,
                    "action": {
                        "type": "click_semantic",
                        "snapshot_id": "",
                        "generation": 0,
                        "locator": { "kind": "role", "role": "button", "name": "Search", "exact": true }
                    },
                    "expected_outcome": "verified",
                    "target": {
                        "locator": { "kind": "role", "role": "button", "name": "Search", "exact": true },
                        "fingerprint_id": "fp-3",
                        "tag": "button"
                    }
                }
            ]),
            json!([{ "name": "slot_2", "step": 2, "kind": "text" }]),
        )
    }

    #[test]
    fn parse_reads_a_version_two_recipe_and_ignores_fields_it_does_not_use() {
        let mut value = sample();
        value["source_receipt_ids"] = json!(["receipt-1"]);
        let recipe = parse_value(&value).expect("version 2 parses");
        assert_eq!(recipe.procedure_id, "proc-1");
        assert_eq!(recipe.steps.len(), 3);
        assert_eq!(recipe.steps[1].slot.as_deref(), Some("slot_2"));
        assert_eq!(recipe.steps[0].expected_outcome, RecipeOutcome::Verified);
        assert!(recipe.steps[2].needs_snapshot());
        assert!(!recipe.steps[0].needs_snapshot());
    }

    #[test]
    fn parse_refuses_another_version_and_a_bad_step_count() {
        let mut version_one = sample();
        version_one["version"] = json!(1);
        assert_eq!(
            parse_value(&version_one),
            Err(RecipeRefusal::UnsupportedVersion)
        );
        assert_eq!(
            parse_value(&recipe_json(json!([]), json!([]))),
            Err(RecipeRefusal::StepCount)
        );
        let too_many: Vec<Value> = (1..=101)
            .map(|step| json!({ "step": step, "action": { "type": "back" } }))
            .collect();
        assert_eq!(
            parse_value(&recipe_json(json!(too_many), json!([]))),
            Err(RecipeRefusal::StepCount)
        );
    }

    #[test]
    fn parse_refuses_an_action_outside_the_vocabulary_by_step_number() {
        let value = recipe_json(
            json!([
                { "step": 1, "action": { "type": "navigate", "url": "https://example.com/" } },
                { "step": 2, "action": { "type": "evaluate", "script": "1" } }
            ]),
            json!([]),
        );
        assert_eq!(
            parse_value(&value),
            Err(RecipeRefusal::UnsupportedAction(2))
        );
    }

    #[test]
    fn parse_refuses_bytes_that_are_not_a_recipe() {
        assert_eq!(Recipe::parse(b"not json"), Err(RecipeRefusal::Malformed));
        let unknown_slot_kind = recipe_json(
            json!([{ "step": 1, "action": { "type": "back" } }]),
            json!([{ "name": "slot_1", "step": 1, "kind": "pin" }]),
        );
        assert_eq!(
            parse_value(&unknown_slot_kind),
            Err(RecipeRefusal::Malformed)
        );
    }

    #[test]
    fn parse_refuses_slots_that_do_not_line_up_with_their_steps() {
        let typing_without_slot = recipe_json(
            json!([{ "step": 1, "action": { "type": "type", "selector": "#q", "text": "" } }]),
            json!([]),
        );
        assert_eq!(
            parse_value(&typing_without_slot),
            Err(RecipeRefusal::Malformed)
        );
        let slot_nobody_names = recipe_json(
            json!([{ "step": 1, "action": { "type": "back" } }]),
            json!([{ "name": "slot_1", "step": 1, "kind": "text" }]),
        );
        assert_eq!(
            parse_value(&slot_nobody_names),
            Err(RecipeRefusal::Malformed)
        );
    }

    #[test]
    fn bind_slots_binds_text_and_refuses_what_it_should() {
        let recipe = parse_value(&sample()).expect("parses");
        let mut values = BTreeMap::new();
        assert_eq!(
            bind_slots(&recipe, &values).err(),
            Some(RecipeRefusal::SlotUnbound)
        );
        values.insert("slot_9".to_owned(), "extra".to_owned());
        assert_eq!(
            bind_slots(&recipe, &values).err(),
            Some(RecipeRefusal::UnknownSlot)
        );
        values.remove("slot_9");
        values.insert("slot_2".to_owned(), "rust sandbox".to_owned());
        let bound = bind_slots(&recipe, &values).expect("binds");
        assert_eq!(bound.text_for(&recipe.steps[1]), Some("rust sandbox"));
        assert_eq!(bound.text_for(&recipe.steps[0]), None);
        values.insert("slot_2".to_owned(), "x".repeat(MAX_SLOT_CHARS));
        assert!(bind_slots(&recipe, &values).is_ok());
        values.insert("slot_2".to_owned(), "x".repeat(MAX_SLOT_CHARS + 1));
        assert_eq!(
            bind_slots(&recipe, &values).err(),
            Some(RecipeRefusal::SlotValueInvalid)
        );
        values.insert("slot_2".to_owned(), "two\nlines".to_owned());
        assert_eq!(
            bind_slots(&recipe, &values).err(),
            Some(RecipeRefusal::SlotValueInvalid)
        );
    }

    #[test]
    fn a_secret_slot_is_refused_whatever_is_bound() {
        let mut value = sample();
        value["slots"] = json!([{ "name": "slot_2", "step": 2, "kind": "secret" }]);
        let recipe = parse_value(&value).expect("parses");
        let mut values = BTreeMap::new();
        values.insert("slot_2".to_owned(), "placeholder-value".to_owned());
        assert_eq!(
            bind_slots(&recipe, &values).err(),
            Some(RecipeRefusal::SecretSlot)
        );
    }

    #[test]
    fn the_debug_output_of_a_bound_recipe_shows_counts_not_text() {
        let recipe = parse_value(&sample()).expect("parses");
        let mut values = BTreeMap::new();
        values.insert("slot_2".to_owned(), "private search text".to_owned());
        let bound = bind_slots(&recipe, &values).expect("binds");
        let shown = format!("{:?}", bound);
        assert!(!shown.contains("private search text"));
        assert!(shown.contains("bound_slots"));
    }

    #[test]
    fn resolve_locator_matches_the_way_quarry_does() {
        let targets = vec![
            target("@e1", "button", "Save draft"),
            target("@e2", "button", "Save"),
            WireSnapshotTarget {
                ref_id: "@e3".to_owned(),
                text: Some("  Welcome back  ".to_owned()),
                ..WireSnapshotTarget::default()
            },
            WireSnapshotTarget {
                ref_id: "@e4".to_owned(),
                placeholder: Some("Search the docs".to_owned()),
                ..WireSnapshotTarget::default()
            },
            WireSnapshotTarget {
                ref_id: "@e5".to_owned(),
                test_id: Some("checkout-submit".to_owned()),
                ..WireSnapshotTarget::default()
            },
        ];
        // Exact role and name: the button named "Save", not "Save draft".
        let save = json!({ "kind": "role", "role": "button", "name": "save", "exact": true });
        assert_eq!(
            resolve_locator(&targets, &save),
            LocatorMatch::One("@e2".to_owned())
        );
        // Not exact: "Save" is a substring of both buttons.
        let any_save = json!({ "kind": "role", "role": "button", "name": "Save" });
        assert_eq!(
            resolve_locator(&targets, &any_save),
            LocatorMatch::Ambiguous(2)
        );
        // The page text is trimmed before the compare.
        let welcome = json!({ "kind": "text", "text": "welcome back", "exact": true });
        assert_eq!(
            resolve_locator(&targets, &welcome),
            LocatorMatch::One("@e3".to_owned())
        );
        let search = json!({ "kind": "placeholder", "placeholder": "search", "exact": false });
        assert_eq!(
            resolve_locator(&targets, &search),
            LocatorMatch::One("@e4".to_owned())
        );
        let submit = json!({ "kind": "test_id", "test_id": "checkout-submit", "exact": true });
        assert_eq!(
            resolve_locator(&targets, &submit),
            LocatorMatch::One("@e5".to_owned())
        );
        // A label is the accessible name.
        let label = json!({ "kind": "label", "label": "Save draft", "exact": true });
        assert_eq!(
            resolve_locator(&targets, &label),
            LocatorMatch::One("@e1".to_owned())
        );
        // Nth never matches, and neither does a shape this module does not read.
        let nth = json!({ "kind": "nth", "selector": "button", "index": 0 });
        assert_eq!(resolve_locator(&targets, &nth), LocatorMatch::None);
        assert_eq!(
            resolve_locator(&targets, &json!({ "kind": "role" })),
            LocatorMatch::None
        );
    }

    #[test]
    fn a_click_proposal_carries_the_fresh_snapshot_and_the_stored_check() {
        let recipe = parse_value(&sample()).expect("parses");
        let targets = vec![
            target("@e7", "button", "Search"),
            target("@e8", "button", "Cancel"),
        ];
        let proposal = proposal_for_step(&recipe.steps[2], None, "snap-9", 4, &targets, "grant-1")
            .expect("proposes");
        assert_eq!(proposal.action.action_id, "recipe-3");
        assert_eq!(proposal.action.reason, "recipe step 3");
        assert_eq!(proposal.action.grant_id, "grant-1");
        assert_eq!(proposal.action.action_type, ActionType::Click);
        let bound = proposal.action.target.as_ref().expect("a target is bound");
        assert_eq!(bound.ref_id, "@e7");
        assert_eq!(bound.snapshot_id, "snap-9");
        assert_eq!(bound.generation, 4);
        assert_eq!(bound.frame_id, None);
        assert_eq!(proposal.check_json["type"], "click_semantic");
        assert_eq!(proposal.check_json["snapshot_id"], "snap-9");
        assert_eq!(proposal.check_json["generation"], 4);
        assert_eq!(
            proposal.check_json["locator"],
            recipe.steps[2].action["locator"]
        );
        assert_eq!(proposal.expect, None);
    }

    #[test]
    fn a_typing_proposal_carries_the_bound_text_to_the_action_and_the_check() {
        let recipe = parse_value(&sample()).expect("parses");
        let mut values = BTreeMap::new();
        values.insert("slot_2".to_owned(), "rust sandbox".to_owned());
        let bound = bind_slots(&recipe, &values).expect("binds");
        let targets = vec![WireSnapshotTarget {
            ref_id: "@e2".to_owned(),
            placeholder: Some("Search".to_owned()),
            ..WireSnapshotTarget::default()
        }];
        let step = &recipe.steps[1];
        let proposal =
            proposal_for_step(step, bound.text_for(step), "snap-1", 1, &targets, "grant-1")
                .expect("proposes");
        assert_eq!(proposal.action.action_type, ActionType::Type);
        assert_eq!(proposal.action.value, "rust sandbox");
        assert_eq!(proposal.check_json["type"], "type_semantic");
        assert_eq!(proposal.check_json["text"], "rust sandbox");
        assert_eq!(
            proposal.action.target.as_ref().map(|t| t.ref_id.as_str()),
            Some("@e2")
        );
        // Without the bound text a typing step is refused, and never types a blank.
        assert_eq!(
            proposal_for_step(step, None, "snap-1", 1, &targets, "grant-1").err(),
            Some(StepRefusal::SlotUnbound)
        );
    }

    #[test]
    fn a_step_whose_target_is_missing_or_ambiguous_needs_repair() {
        let recipe = parse_value(&sample()).expect("parses");
        let step = &recipe.steps[2];
        let missing = vec![target("@e1", "link", "Home")];
        assert_eq!(
            proposal_for_step(step, None, "s", 1, &missing, "g").err(),
            Some(StepRefusal::TargetRepairRequired)
        );
        let twice = vec![
            target("@e1", "button", "Search"),
            target("@e2", "button", "Search"),
        ];
        assert_eq!(
            proposal_for_step(step, None, "s", 1, &twice, "g").err(),
            Some(StepRefusal::TargetRepairRequired)
        );
    }

    #[test]
    fn a_target_in_a_child_frame_keeps_its_frame() {
        let value = recipe_json(
            json!([{
                "step": 1,
                "action": {
                    "type": "click_semantic",
                    "snapshot_id": "",
                    "generation": 0,
                    "locator": { "kind": "test_id", "test_id": "pay", "exact": true }
                }
            }]),
            json!([]),
        );
        let recipe = parse_value(&value).expect("parses");
        let targets = vec![WireSnapshotTarget {
            ref_id: "@f1".to_owned(),
            frame_id: Some("frame-7".to_owned()),
            test_id: Some("pay".to_owned()),
            ..WireSnapshotTarget::default()
        }];
        let proposal = proposal_for_step(&recipe.steps[0], None, "snap-1", 1, &targets, "grant-1")
            .expect("proposes");
        let bound = proposal.action.target.expect("a target is bound");
        assert_eq!(bound.frame_id.as_deref(), Some("frame-7"));
        assert_eq!(bound.ref_id, "@f1");
    }

    #[test]
    fn the_plain_verbs_map_onto_the_loop_fields() {
        let value = recipe_json(
            json!([
                { "step": 1, "action": { "type": "navigate", "url": "https://example.com/" } },
                { "step": 2, "action": { "type": "click", "selector": "#go" } },
                { "step": 3, "action": { "type": "press", "key": "Enter" } },
                { "step": 4, "action": { "type": "scroll", "target": "main" } },
                { "step": 5, "action": { "type": "wait", "ms": 750 } },
                { "step": 6, "action": { "type": "back" } },
                { "step": 7, "action": { "type": "get_content" } }
            ]),
            json!([]),
        );
        let recipe = parse_value(&value).expect("parses");

        let navigate = plain(&recipe.steps[0]).expect("navigate");
        assert_eq!(navigate.action.action_type, ActionType::Goto);
        assert_eq!(navigate.action.url, "https://example.com/");
        assert_eq!(navigate.check_json, recipe.steps[0].action);

        let click = plain(&recipe.steps[1]).expect("click");
        assert_eq!(click.action.action_type, ActionType::Click);
        assert_eq!(click.action.selector, "#go");
        assert!(click.action.target.is_none());

        let press = plain(&recipe.steps[2]).expect("press");
        assert_eq!(press.action.action_type, ActionType::Press);
        assert_eq!(press.action.value, "Enter");

        let scroll = plain(&recipe.steps[3]).expect("scroll");
        assert_eq!(scroll.action.action_type, ActionType::Scroll);
        assert_eq!(scroll.action.selector, "main");

        let wait = plain(&recipe.steps[4]).expect("wait");
        assert_eq!(wait.action.action_type, ActionType::Wait);
        assert_eq!(wait.action.max_wait_ms, 750);

        let back = plain(&recipe.steps[5]).expect("back");
        assert_eq!(back.action.action_type, ActionType::Back);
        let extract = plain(&recipe.steps[6]).expect("get_content");
        assert_eq!(extract.action.action_type, ActionType::Extract);
    }

    #[test]
    fn select_and_wait_for_act_on_the_resolved_ref() {
        let value = recipe_json(
            json!([
                {
                    "step": 1,
                    "action": {
                        "type": "select_semantic",
                        "snapshot_id": "",
                        "generation": 0,
                        "locator": { "kind": "label", "label": "Plan", "exact": true },
                        "value": "Express"
                    }
                },
                {
                    "step": 2,
                    "action": {
                        "type": "wait_for_semantic",
                        "snapshot_id": "",
                        "generation": 0,
                        "locator": { "kind": "text", "text": "Done", "exact": false },
                        "timeout_ms": 2500
                    }
                }
            ]),
            json!([]),
        );
        let recipe = parse_value(&value).expect("parses");
        let targets = vec![
            target("@e4", "combobox", "Plan"),
            WireSnapshotTarget {
                ref_id: "@e5".to_owned(),
                text: Some("All done".to_owned()),
                ..WireSnapshotTarget::default()
            },
        ];
        let select = proposal_for_step(&recipe.steps[0], None, "snap-2", 3, &targets, "grant-2")
            .expect("select");
        assert_eq!(select.action.action_type, ActionType::Select);
        assert_eq!(select.action.value, "Express");
        assert_eq!(
            select.action.target.map(|t| t.ref_id),
            Some("@e4".to_owned())
        );
        let wait = proposal_for_step(&recipe.steps[1], None, "snap-2", 3, &targets, "grant-2")
            .expect("wait");
        assert_eq!(wait.action.action_type, ActionType::WaitFor);
        assert_eq!(wait.action.max_wait_ms, 2500);
        assert_eq!(wait.action.target.map(|t| t.generation), Some(3));
    }
}
