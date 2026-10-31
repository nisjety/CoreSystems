//! Why the answer pipeline is, or is not, available (INNH-8a).
//!
//! `POST /v1/answer`, `POST /v1/answer/stream` and `include_answer` on `POST /v1/search` all run
//! through one [`quarry_runtime::answer::AnswerPipeline`]. Until now a missing pipeline looked the
//! same from every angle: a bare 501 whose hint named two of the gates, or a silent
//! `answer: null` on search. This module is the one place that names the gate that is closed.
//!
//! - [`AnswerPipelineStatus`]: derived once at boot and stored on `AppState`. It names the FIRST
//!   closed gate, in the order an operator opens them: the `QUARRY_EDGE__ANSWER_SYNTHESIS` switch,
//!   a search provider, `QUARRY_EDGE__MODEL_PLANE_URL`, then a Model Plane client that builds.
//! - [`AnswerReason`]: the closed vocabulary that goes on the wire as `reason` (answer routes) and
//!   `answer_reason` (search). It also covers the failures of a pipeline that IS wired: the Model
//!   Plane refusing the edge's service token, rate limiting it, or failing. Never provider text.
//! - [`AnswerStatus`]: the `answer_status` of a search response that asked for an answer.
//!
//! The model hop is told apart from the search hop by a detail the runtime stamps on the error
//! (`hop: model_plane`, see [`quarry_runtime::answer::MODEL_HOP`]). A refusal of the service
//! token is a deployment fact, not a fault of the caller, so it is a 503 and never a 403.

use std::sync::Arc;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::answer::{AnswerPipeline, MODEL_HOP, MODEL_HOP_KEY};
use serde::Serialize;

use crate::state::AppState;

/// Env var of the switch. Default off: setting `MODEL_PLANE_URL` for another consumer (autoprompt,
/// rerank, page titles) must not also turn answer synthesis on.
#[allow(dead_code)]
pub const ENV_ANSWER_SYNTHESIS: &str = "QUARRY_EDGE__ANSWER_SYNTHESIS";

/// What boot decided about the answer pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerPipelineStatus {
    /// The pipeline is built and wired.
    Ready,
    /// `QUARRY_EDGE__ANSWER_SYNTHESIS` is not on (the default).
    SynthesisDisabled,
    /// No search provider is configured.
    NoSearchProvider,
    /// `QUARRY_EDGE__MODEL_PLANE_URL` is unset or blank.
    NoModelPlaneUrl,
    /// The URL is set but the Model Plane client could not be built from it.
    ModelClientInitFailed,
}

impl AnswerPipelineStatus {
    /// The first closed gate of the three that can be read before a client is built. `Ready`
    /// means the caller should now try to build the client, and record
    /// [`AnswerPipelineStatus::ModelClientInitFailed`] if that fails.
    pub fn gate(
        synthesis_enabled: bool,
        has_search_provider: bool,
        has_model_plane_url: bool,
    ) -> Self {
        if !synthesis_enabled {
            Self::SynthesisDisabled
        } else if !has_search_provider {
            Self::NoSearchProvider
        } else if !has_model_plane_url {
            Self::NoModelPlaneUrl
        } else {
            Self::Ready
        }
    }

    /// The reason a closed gate puts on the wire; `None` for `Ready`.
    pub fn reason(self) -> Option<AnswerReason> {
        match self {
            Self::Ready => None,
            Self::SynthesisDisabled => Some(AnswerReason::SynthesisDisabled),
            Self::NoSearchProvider => Some(AnswerReason::NoSearchProvider),
            Self::NoModelPlaneUrl => Some(AnswerReason::NoModelPlaneUrl),
            Self::ModelClientInitFailed => Some(AnswerReason::ModelClientInitFailed),
        }
    }
}

/// The closed set of reasons an answer is unavailable. Serialized `snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerReason {
    SynthesisDisabled,
    NoSearchProvider,
    NoModelPlaneUrl,
    ModelClientInitFailed,
    /// Auth Core refused to mint the edge's Model Plane token, or the Model Plane answered 401 or
    /// 403 to it. Cross-plane enablement is an owner decision (INNH-8b).
    ModelPlaneAuthRefused,
    ModelPlaneRateLimited,
    /// Any other failure of the model hop (transport, 5xx, undecodable body).
    ModelPlaneError,
    /// The search or page-fetch half of the pipeline failed before a model was asked.
    AnswerSearchFailed,
}

impl AnswerReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SynthesisDisabled => "synthesis_disabled",
            Self::NoSearchProvider => "no_search_provider",
            Self::NoModelPlaneUrl => "no_model_plane_url",
            Self::ModelClientInitFailed => "model_client_init_failed",
            Self::ModelPlaneAuthRefused => "model_plane_auth_refused",
            Self::ModelPlaneRateLimited => "model_plane_rate_limited",
            Self::ModelPlaneError => "model_plane_error",
            Self::AnswerSearchFailed => "answer_search_failed",
        }
    }

    /// The exact next step, naming the env var. Fixed text: it never carries provider output.
    pub fn hint(self) -> &'static str {
        match self {
            Self::SynthesisDisabled => "set QUARRY_EDGE__ANSWER_SYNTHESIS=1 (default 0) on quarry-edge; it also needs a search provider and QUARRY_EDGE__MODEL_PLANE_URL; see docs/RUNBOOKS.md, Answer synthesis",
            Self::NoSearchProvider => "configure a search provider on quarry-edge (SEARXNG_URL, BRAVE_SEARCH_KEY or SERPER_KEY); see docs/RUNBOOKS.md, Answer synthesis",
            Self::NoModelPlaneUrl => "set QUARRY_EDGE__MODEL_PLANE_URL (MODEL_PLANE_URL in compose) to the Model Plane gateway base URL; see docs/RUNBOOKS.md, Answer synthesis",
            Self::ModelClientInitFailed => "the Model Plane client could not be built from QUARRY_EDGE__MODEL_PLANE_URL; check that the URL is a valid http(s) base URL and read the quarry-edge boot log; see docs/RUNBOOKS.md, Answer synthesis",
            Self::ModelPlaneAuthRefused => "the Model Plane refused quarry-edge's service token: the Control Plane registry must grant quarry-edge the model-gateway audience and the gateway must allow its route (owner decision INNH-8b); see docs/RUNBOOKS.md, Answer synthesis",
            Self::ModelPlaneRateLimited => "the Model Plane rate limited the synthesis call; retry later",
            Self::ModelPlaneError => "the Model Plane failed the synthesis call; retry later and check the model gateway",
            Self::AnswerSearchFailed => "the search or page-fetch step failed before synthesis; retry later",
        }
    }
}

/// The `answer_status` of a `POST /v1/search` response that asked for `include_answer`. A response
/// that did not ask carries no status at all, so a default search response is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerStatus {
    /// A synthesized answer is in `answer`.
    Ok,
    /// The pipeline ran and no source gave it usable text; `answer` is empty.
    NoSources,
    /// Zero data retention is on: nothing was sent to a model and `answer` is absent.
    SkippedZdr,
    /// No answer; `answer_reason` says why.
    Unavailable,
}

/// The pipeline, or the exact reason there is none.
pub fn gate(state: &AppState) -> Result<&Arc<AnswerPipeline>, AnswerReason> {
    match state.answer_pipeline.as_ref() {
        Some(pipeline) => Ok(pipeline),
        // A state with no pipeline and a `Ready` status is a test double that never wired one:
        // the switch is off in every sense that matters.
        None => Err(state
            .answer_status
            .reason()
            .unwrap_or(AnswerReason::SynthesisDisabled)),
    }
}

/// Which closed reason a failed `AnswerPipeline::answer` call maps to.
pub fn classify_failure(error: &QuarryError) -> AnswerReason {
    let on_model_hop = error
        .details
        .as_ref()
        .and_then(|details| details.get(MODEL_HOP_KEY))
        .and_then(|hop| hop.as_str())
        == Some(MODEL_HOP);
    classify(on_model_hop, error.code)
}

/// Same mapping for a caller that already knows the hop (the stream route calls the model
/// directly, after `prepare`).
pub fn classify(on_model_hop: bool, code: ErrorCode) -> AnswerReason {
    if !on_model_hop {
        return AnswerReason::AnswerSearchFailed;
    }
    match code {
        ErrorCode::Forbidden | ErrorCode::Unauthorized => AnswerReason::ModelPlaneAuthRefused,
        ErrorCode::RateLimited => AnswerReason::ModelPlaneRateLimited,
        _ => AnswerReason::ModelPlaneError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_closed_gate_is_named_in_the_order_an_operator_opens_them() {
        use AnswerPipelineStatus as S;
        assert_eq!(S::gate(false, false, false), S::SynthesisDisabled);
        assert_eq!(S::gate(false, true, true), S::SynthesisDisabled);
        assert_eq!(S::gate(true, false, false), S::NoSearchProvider);
        assert_eq!(S::gate(true, false, true), S::NoSearchProvider);
        assert_eq!(S::gate(true, true, false), S::NoModelPlaneUrl);
        assert_eq!(S::gate(true, true, true), S::Ready);
    }

    #[test]
    fn a_ready_status_has_no_reason_and_every_closed_gate_has_one() {
        use AnswerPipelineStatus as S;
        assert_eq!(S::Ready.reason(), None);
        assert_eq!(
            S::SynthesisDisabled.reason(),
            Some(AnswerReason::SynthesisDisabled)
        );
        assert_eq!(
            S::NoSearchProvider.reason(),
            Some(AnswerReason::NoSearchProvider)
        );
        assert_eq!(
            S::NoModelPlaneUrl.reason(),
            Some(AnswerReason::NoModelPlaneUrl)
        );
        assert_eq!(
            S::ModelClientInitFailed.reason(),
            Some(AnswerReason::ModelClientInitFailed)
        );
    }

    #[test]
    fn reasons_serialize_as_their_wire_string_and_every_hint_names_the_runbook() {
        for reason in [
            AnswerReason::SynthesisDisabled,
            AnswerReason::NoSearchProvider,
            AnswerReason::NoModelPlaneUrl,
            AnswerReason::ModelClientInitFailed,
            AnswerReason::ModelPlaneAuthRefused,
            AnswerReason::ModelPlaneRateLimited,
            AnswerReason::ModelPlaneError,
            AnswerReason::AnswerSearchFailed,
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                serde_json::json!(reason.as_str())
            );
            assert!(!reason.hint().is_empty());
        }
        assert!(AnswerReason::SynthesisDisabled
            .hint()
            .contains(ENV_ANSWER_SYNTHESIS));
        assert!(AnswerReason::NoModelPlaneUrl
            .hint()
            .contains("QUARRY_EDGE__MODEL_PLANE_URL"));
    }

    #[test]
    fn a_failure_is_told_by_its_hop_and_its_code() {
        let tagged = |code| {
            QuarryError::new(code, "x")
                .with_details(serde_json::json!({ MODEL_HOP_KEY: MODEL_HOP }))
        };
        assert_eq!(
            classify_failure(&tagged(ErrorCode::Forbidden)),
            AnswerReason::ModelPlaneAuthRefused
        );
        assert_eq!(
            classify_failure(&tagged(ErrorCode::Unauthorized)),
            AnswerReason::ModelPlaneAuthRefused
        );
        assert_eq!(
            classify_failure(&tagged(ErrorCode::RateLimited)),
            AnswerReason::ModelPlaneRateLimited
        );
        assert_eq!(
            classify_failure(&tagged(ErrorCode::DriverFailed)),
            AnswerReason::ModelPlaneError
        );
        // The search half never reports the model's reasons, even for a 403.
        assert_eq!(
            classify_failure(&QuarryError::new(ErrorCode::Forbidden, "search said no")),
            AnswerReason::AnswerSearchFailed
        );
    }
}
