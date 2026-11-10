//! F-t3-081 Teach mode: a redacted procedure draft from a run's human takeover receipts.
//!
//!   GET /v1/agent/runs/{run_id}/procedure-draft
//!
//! The draft is what a person did on a run they drove or took over, in order. A typed or
//! selected value becomes a named placeholder (`{{param_<n>}}`), and the draft names each
//! placeholder with its kind. Typed text, page text, screenshots and full URLs never leave: a
//! navigation keeps its host only. The route only reads. Quarry owns compile and replay, so
//! nothing is replayed here.
//!
//! A ZDR run answers `409` (`zdr_not_supported`): it keeps no receipts to draft from. The route is
//! dark until `QUARRY_TEACH_MODE_ENABLED` is the literal `true`; while it is off, it answers `404`
//! (`teach_mode_disabled`).

use super::*;

use quarry_core::contracts::SemanticLocator;
use quarry_core::redaction::{classify_field, FieldFacts, FieldKind};

/// The switch. Read per call, like `QUARRY_EDGE__HIT_TEST`, so it needs no AppState plumbing.
pub const TEACH_MODE_FLAG: &str = "QUARRY_TEACH_MODE_ENABLED";

/// The most steps one draft returns. A run with more human steps sets `truncated`.
const MAX_DRAFT_STEPS: usize = 100;

/// The longest hint kept. A longer value is dropped whole, never cut, so page text cannot pass
/// as a hint.
const MAX_HINT_CHARS: usize = 200;

/// Only the exact literal `true` turns teach mode on. Any other value, or none, leaves it off.
fn parse_teach_mode_flag(raw: Option<&str>) -> bool {
    raw == Some("true")
}

fn teach_mode_enabled() -> bool {
    parse_teach_mode_flag(std::env::var(TEACH_MODE_FLAG).ok().as_deref())
}

/// One step of a draft: a human action named with one of the draft's six verbs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcedureDraftStep {
    /// The step's place in the draft, counted from 1.
    pub index: u32,
    pub action: ProcedureDraftAction,
    pub target: ProcedureDraftTarget,
    /// The host the step ran on: the scheme's host only, never a path, query or port. `None`
    /// when the receipt keeps no page address.
    pub url_host: Option<String>,
    /// The placeholder that replaces a typed or selected value, `{{param_<n>}}`. `None` for a
    /// step that carried no value.
    pub parameter: Option<String>,
}

/// The verb of a draft step.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcedureDraftAction {
    Navigate,
    Click,
    Type,
    Select,
    Scroll,
    Submit,
}

/// What a step acted on, as its receipt recorded it. Each field is absent when the receipt does
/// not record it.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ProcedureDraftTarget {
    /// The element's ARIA role.
    pub role: Option<String>,
    /// The element's accessible name, label or visible text.
    pub name: Option<String>,
    /// A selector or test id the receipt recorded, as a hint for a replay.
    pub selector_hint: Option<String>,
}

/// A value the draft asks the caller to supply.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcedureDraftParam {
    /// The placeholder's name, `param_<n>`, counted from 1 in step order.
    pub name: String,
    pub kind: ProcedureDraftParamKind,
}

/// What kind of value a parameter holds.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcedureDraftParamKind {
    /// Free text typed into a field.
    Text,
    /// A password or one-time code. The caller supplies it at replay and it is never stored.
    Secret,
    /// An option chosen from a list.
    Choice,
}

/// The answer's body.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProcedureDraft {
    pub run_id: String,
    pub steps: Vec<ProcedureDraftStep>,
    pub params: Vec<ProcedureDraftParam>,
    /// True when the run holds more human steps than the draft keeps.
    pub truncated: bool,
}

/// `GET /v1/agent/runs/{run_id}/procedure-draft`: the human steps of one run, as a redacted draft.
///
/// Authorized exactly as `list_receipts` is: a live run answers to its own tenant and actor, and a
/// durable run to the caller's org and actor. Anyone else gets the run's `404`.
pub async fn run_procedure_draft(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Path(run_id): Path<String>,
) -> Result<Json<Envelope<ProcedureDraft>>, ApiErr> {
    let request_id = crate::request_id::current();
    if !teach_mode_enabled() {
        return Err(refusal(
            &request_id,
            QuarryError::new(
                ErrorCode::NotFound,
                "teach mode is not enabled on this edge",
            )
            .with_details(serde_json::json!({ "reason": "teach_mode_disabled" })),
        ));
    }
    let receipts = owned_receipts(&state, &claims, &run_id, &request_id).await?;
    let draft = build_draft(&run_id, receipts);
    Ok(Json(Envelope::ok(request_id, draft)))
}

/// The receipts of a run the caller owns, or the refusal. A ZDR run is refused (`409`) before any
/// receipt is read, and so is every run for a caller whose token carries the ZDR floor: that answer
/// is the same for every run id, so it reveals nothing about which runs exist. A run the caller does
/// not own, or one that does not exist, is the plain `404`.
async fn owned_receipts(
    state: &AppState,
    claims: &crate::auth::Claims,
    run_id: &str,
    request_id: &str,
) -> Result<Vec<StepReceipt>, ApiErr> {
    // A caller whose token carries the ZDR floor is refused for every run id, so the answer does not
    // depend on which runs exist.
    if claims.zdr_floor() {
        return Err(zdr_refusal_err(request_id));
    }
    let entry_arc = {
        let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
        map.get(run_id).map(|handle| {
            if handle.live.owned_by(claims) {
                handle.live.touch();
            }
            handle.entry.clone()
        })
    };
    if let Some(entry_arc) = entry_arc {
        let entry = entry_arc.lock().await;
        if !caller_owns_live_run(&entry, claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                request_id,
                "agent run not found",
            ));
        }
        if entry.zdr.is_active() {
            return Err(zdr_refusal_err(request_id));
        }
        return state
            .receipts
            .list_for_actor(&entry.org_id, &entry.actor_id, &entry.run_id)
            .await
            .map_err(|error| driver_err(request_id, error));
    }
    // Receipts are durable apart from the live browser. The checkpoint says the run's posture, and it
    // is read first so a ZDR run is refused before any receipt is.
    let parsed_run_id = run_id
        .parse::<RunKind>()
        .map_err(|_| status_err(StatusCode::NOT_FOUND, request_id, "agent run not found"))?;
    let checkpoint = state
        .receipts
        .load_run_checkpoint(&claims.org_id, &parsed_run_id)
        .await
        .map_err(|error| driver_err(request_id, error))?;
    // The checkpoint is scoped to the tenant only, so its posture counts only when the caller owns
    // it. Otherwise a run of someone else in the org would answer differently from an unknown one.
    let owned = checkpoint.filter(|checkpoint| {
        caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, claims)
    });
    if owned
        .as_ref()
        .is_some_and(|checkpoint| checkpoint.zdr.is_active())
    {
        return Err(zdr_refusal_err(request_id));
    }
    let receipts = state
        .receipts
        .list_for_actor(&claims.org_id, claims.actor_id(), &parsed_run_id)
        .await
        .map_err(|error| driver_err(request_id, error))?;
    if owned.is_none() && receipts.is_empty() {
        return Err(status_err(
            StatusCode::NOT_FOUND,
            request_id,
            "agent run not found",
        ));
    }
    Ok(receipts)
}

/// The draft of a run's human steps, in step order: each step kept, and a parameter for each value
/// it carried.
fn build_draft(run_id: &str, receipts: Vec<StepReceipt>) -> ProcedureDraft {
    let mut human: Vec<StepReceipt> = receipts
        .into_iter()
        .filter(|receipt| receipt.initiated_by == Some(BrowserTimelineInitiator::Human))
        .filter(|receipt| matches!(receipt.outcome, StepOutcome::Completed { .. }))
        .collect();
    human.sort_by_key(|receipt| receipt.step);
    let mut steps = Vec::new();
    let mut params = Vec::new();
    let mut truncated = false;
    for mut receipt in human {
        // A legacy row can hold raw typed text. It is masked on read, as the procedure compiler
        // masks it before it compiles.
        receipt.redact_for_read();
        let observed = receipt
            .observation
            .as_ref()
            .and_then(|observation| observation.target_resolution.as_ref())
            .and_then(|proof| proof.locator.as_ref());
        let Some(mapped) = map_action(&receipt.action, observed) else {
            continue;
        };
        if steps.len() == MAX_DRAFT_STEPS {
            truncated = true;
            break;
        }
        let parameter = mapped.input.map(|kind| {
            let name = format!("param_{}", params.len() + 1);
            let placeholder = ["{{", name.as_str(), "}}"].concat();
            params.push(ProcedureDraftParam { name, kind });
            placeholder
        });
        let host = mapped.navigate_host.or_else(|| {
            receipt
                .observation
                .as_ref()
                .and_then(|observation| url_host(&observation.url))
        });
        steps.push(ProcedureDraftStep {
            index: steps.len() as u32 + 1,
            action: mapped.action,
            target: mapped.target,
            url_host: host,
            parameter,
        });
    }
    ProcedureDraft {
        run_id: run_id.to_owned(),
        steps,
        params,
        truncated,
    }
}

/// One receipt's action before its index and parameter are set.
struct Mapped {
    action: ProcedureDraftAction,
    target: ProcedureDraftTarget,
    /// The kind of value the step carried, when it carried one.
    input: Option<ProcedureDraftParamKind>,
    /// The host a navigation went to.
    navigate_host: Option<String>,
}

/// What a receipt's action is in the draft, or `None` when none of the six verbs names it. A ref
/// addressed step names no target itself, so `observed` (the locator the observation proved for
/// the ref) supplies it when the observation kept one.
fn map_action(action: &AgentAction, observed: Option<&SemanticLocator>) -> Option<Mapped> {
    let observed_target = || observed.map(target_of_locator).unwrap_or_default();
    Some(match action {
        AgentAction::Navigate { url } => Mapped {
            action: ProcedureDraftAction::Navigate,
            target: ProcedureDraftTarget::default(),
            input: None,
            navigate_host: url_host(url),
        },
        AgentAction::Click { selector } => {
            plain(ProcedureDraftAction::Click, target_of_selector(selector))
        }
        AgentAction::ClickRef { .. }
        | AgentAction::FrameClickRef { .. }
        | AgentAction::ClickPoint { .. } => plain(ProcedureDraftAction::Click, observed_target()),
        AgentAction::ClickSemantic { locator, .. } => {
            plain(ProcedureDraftAction::Click, target_of_locator(locator))
        }
        AgentAction::Type { selector, .. } => typed(target_of_selector(selector)),
        AgentAction::TypeRef { .. } | AgentAction::FrameTypeRef { .. } => typed(observed_target()),
        AgentAction::TypeSemantic { locator, .. } => typed(target_of_locator(locator)),
        // A one-time code is always a secret: the fill names the OTP input by definition.
        AgentAction::FillOtpRef { .. } => Mapped {
            action: ProcedureDraftAction::Type,
            target: observed_target(),
            input: Some(ProcedureDraftParamKind::Secret),
            navigate_host: None,
        },
        // A named Enter key is kept on a full receipt and masked on a metadata one. Only the named
        // key submits, so a masked key is not a submit and is left out.
        AgentAction::Press { key } if key.trim().eq_ignore_ascii_case("enter") => plain(
            ProcedureDraftAction::Submit,
            ProcedureDraftTarget::default(),
        ),
        AgentAction::Select { selector, .. } => selected(target_of_selector(selector)),
        AgentAction::SelectRef { .. } | AgentAction::FrameSelectRef { .. } => {
            selected(observed_target())
        }
        AgentAction::SelectSemantic { locator, .. } => selected(target_of_locator(locator)),
        AgentAction::Scroll { target } => {
            plain(ProcedureDraftAction::Scroll, target_of_selector(target))
        }
        AgentAction::MouseWheel { .. } => plain(
            ProcedureDraftAction::Scroll,
            ProcedureDraftTarget::default(),
        ),
        _ => return None,
    })
}

fn plain(action: ProcedureDraftAction, target: ProcedureDraftTarget) -> Mapped {
    Mapped {
        action,
        target,
        input: None,
        navigate_host: None,
    }
}

fn typed(target: ProcedureDraftTarget) -> Mapped {
    let input = typed_input(&target);
    Mapped {
        action: ProcedureDraftAction::Type,
        target,
        input: Some(input),
        navigate_host: None,
    }
}

fn selected(target: ProcedureDraftTarget) -> Mapped {
    Mapped {
        action: ProcedureDraftAction::Select,
        target,
        input: Some(ProcedureDraftParamKind::Choice),
        navigate_host: None,
    }
}

/// Whether a typed value is a secret. A receipt keeps no field kind, so the recorded target's name
/// and selector are read by the same classifier that masks such fields on write. A field with no
/// recorded hint is `text`.
fn typed_input(target: &ProcedureDraftTarget) -> ProcedureDraftParamKind {
    let facts = FieldFacts {
        accessible_name: target.name.clone(),
        name_attr: target.selector_hint.clone(),
        ..FieldFacts::default()
    };
    match classify_field(&facts) {
        FieldKind::Password | FieldKind::Otp => ProcedureDraftParamKind::Secret,
        _ => ProcedureDraftParamKind::Text,
    }
}

fn target_of_selector(selector: &str) -> ProcedureDraftTarget {
    ProcedureDraftTarget {
        selector_hint: safe_hint(selector),
        ..ProcedureDraftTarget::default()
    }
}

fn target_of_locator(locator: &SemanticLocator) -> ProcedureDraftTarget {
    let named = |label: &str| ProcedureDraftTarget {
        name: safe_hint(label),
        ..ProcedureDraftTarget::default()
    };
    match locator {
        SemanticLocator::Role { role, name, .. } => ProcedureDraftTarget {
            role: safe_hint(role),
            name: name.as_deref().and_then(safe_hint),
            selector_hint: None,
        },
        SemanticLocator::Text { text, .. } => named(text),
        SemanticLocator::Label { label, .. } => named(label),
        SemanticLocator::Placeholder { placeholder, .. } => named(placeholder),
        SemanticLocator::TestId { test_id, .. } => ProcedureDraftTarget {
            selector_hint: safe_hint(test_id),
            ..ProcedureDraftTarget::default()
        },
        SemanticLocator::Nth { selector, .. } => target_of_selector(selector),
    }
}

/// A recorded hint as it is, or `None` when it is empty, longer than `MAX_HINT_CHARS`, or carries a
/// URL (`://`). A hint is an identifier or a label, so a value of any other shape is not kept.
fn safe_hint(raw: &str) -> Option<String> {
    let value = raw.trim();
    let kept =
        !value.is_empty() && value.chars().count() <= MAX_HINT_CHARS && !value.contains("://");
    kept.then(|| value.to_owned())
}

/// The host of a page address: `None` for a scheme other than http or https, and for an address
/// that does not parse. No userinfo, port, path or query is kept.
fn url_host(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str().map(str::to_owned)
}

/// The refusal for a ZDR run: `409`, with `details.reason` the client can match.
fn zdr_refusal_err(request_id: &str) -> ApiErr {
    refusal(
        request_id,
        QuarryError::new(
            ErrorCode::Conflict,
            "ZDR active: a zero data retention run keeps no receipts, so there is no procedure to draft",
        )
        .with_details(serde_json::json!({ "reason": "zdr_not_supported" })),
    )
}

/// An error answer at the HTTP status its code names, with the standard envelope.
fn refusal(request_id: &str, error: QuarryError) -> ApiErr {
    let status =
        StatusCode::from_u16(error.code.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(Envelope::<()>::err(request_id, error)))
}
