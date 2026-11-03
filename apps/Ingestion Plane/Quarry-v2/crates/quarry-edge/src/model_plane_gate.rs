//! The Model Plane features of quarry-edge, decided once at boot.
//!
//! A feature that calls the Model Plane has a switch, and every switch is OFF by default. A switch
//! that is on still needs a model path: a Model Plane URL, a service identity, and, when Auth Core
//! mints that identity, a row of the cross-plane token-request manifest that is `granted` for each
//! request the feature makes. A switch that is on without that path stays off, and boot logs an
//! error that names it. Nothing degrades silently: the feature's routes answer their unavailable
//! response and make no Model Plane call.
//!
//! Today every feature here is refused. `model-invoke` and `model-audio` are `granted: false`, so
//! the gate keeps each switch off and Auth Core would answer each call with 403. Granting a row
//! does not by itself open a path: the Model Plane gateway's `authorize_principal_route` closes
//! `/v1/invoke` and `/v1/invoke/stream` to service principals whatever the registry says. The chat
//! route a service may use is `POST /v1/ai/chat` with `models:invoke`, and the manifest notes name
//! `model-extract` and `model-extract-inference` (granted since WS16.S2) as its two tokens. This
//! gate still checks `model-invoke` for these features (see `ModelPlaneFeature::rows`), so it is
//! stricter than the route it will use. The org-bound, metered lane is decision DR-15.1 (proposed,
//! not built) and is not in this module.

use quarry_runtime::service_tokens::token_request_manifest;

use crate::config::EdgeConfig;

/// Which Model Plane features may run. All `false` is the safe default. Routes read their own field
/// here before they call the Model Plane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelPlaneFeatures {
    /// LLM intent classification of queries (`QUARRY_EDGE__LLM_CLASSIFY_INTENT`).
    pub intent_classifier: bool,
    /// Query rewriting before fan-out (`QUARRY_EDGE__AUTOPROMPT`).
    pub autoprompt: bool,
    /// Page titles for pages without a usable `<title>` (`QUARRY_EDGE__PAGE_TITLE_ENRICH`).
    pub page_title: bool,
    /// `POST /v1/answer` and `include_answer` on search (`QUARRY_EDGE__ANSWER_SYNTHESIS`).
    pub answer_synthesis: bool,
    /// Structured extraction on `POST /v1/extract` (`QUARRY_EDGE__AI_FORMATS`).
    pub ai_formats: bool,
    /// `POST /v1/search/suggest` (`QUARRY_EDGE__SEARCH_SUGGEST`).
    pub search_suggest: bool,
    /// `POST /v1/audio` (`QUARRY_EDGE__MODEL_AUDIO`).
    pub model_audio: bool,
}

/// One Model Plane feature, named by its switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    IntentClassifier,
    Autoprompt,
    PageTitle,
    AnswerSynthesis,
    AiFormats,
    SearchSuggest,
    ModelAudio,
}

/// Every feature, in the order boot decides them.
pub const FEATURES: [Feature; 7] = [
    Feature::IntentClassifier,
    Feature::Autoprompt,
    Feature::PageTitle,
    Feature::AnswerSynthesis,
    Feature::AiFormats,
    Feature::SearchSuggest,
    Feature::ModelAudio,
];

impl Feature {
    /// The environment switch that asks for the feature.
    pub fn switch(self) -> &'static str {
        match self {
            Self::IntentClassifier => "QUARRY_EDGE__LLM_CLASSIFY_INTENT",
            Self::Autoprompt => "QUARRY_EDGE__AUTOPROMPT",
            Self::PageTitle => "QUARRY_EDGE__PAGE_TITLE_ENRICH",
            Self::AnswerSynthesis => "QUARRY_EDGE__ANSWER_SYNTHESIS",
            Self::AiFormats => "QUARRY_EDGE__AI_FORMATS",
            Self::SearchSuggest => "QUARRY_EDGE__SEARCH_SUGGEST",
            Self::ModelAudio => "QUARRY_EDGE__MODEL_AUDIO",
        }
    }

    /// What the feature is, for the boot log.
    pub fn label(self) -> &'static str {
        match self {
            Self::IntentClassifier => "LLM intent classification",
            Self::Autoprompt => "autoprompt (query rewriting)",
            Self::PageTitle => "page-title enrichment",
            Self::AnswerSynthesis => "answer synthesis (/v1/answer)",
            Self::AiFormats => "structured extraction (/v1/extract with a schema)",
            Self::SearchSuggest => "search suggestions (/v1/search/suggest)",
            Self::ModelAudio => "audio (/v1/audio)",
        }
    }

    /// The rows of the cross-plane token-request manifest that the feature's Model Plane calls mint
    /// for. Every call of these features is `model-invoke`; audio is `model-audio`.
    pub fn rows(self) -> &'static [&'static str] {
        match self {
            Self::ModelAudio => &["model-audio"],
            _ => &["model-invoke"],
        }
    }

    fn requested(self, cfg: &EdgeConfig) -> bool {
        match self {
            Self::IntentClassifier => cfg.llm_classify_intent,
            Self::Autoprompt => cfg.autoprompt,
            Self::PageTitle => cfg.page_title_enrich,
            Self::AnswerSynthesis => cfg.answer_synthesis,
            Self::AiFormats => cfg.ai_formats,
            Self::SearchSuggest => cfg.search_suggest,
            Self::ModelAudio => cfg.model_audio,
        }
    }

    fn enable(self, features: &mut ModelPlaneFeatures) {
        match self {
            Self::IntentClassifier => features.intent_classifier = true,
            Self::Autoprompt => features.autoprompt = true,
            Self::PageTitle => features.page_title = true,
            Self::AnswerSynthesis => features.answer_synthesis = true,
            Self::AiFormats => features.ai_formats = true,
            Self::SearchSuggest => features.search_suggest = true,
            Self::ModelAudio => features.model_audio = true,
        }
    }
}

/// A feature that was switched on and refused, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub feature: Feature,
    pub reason: String,
}

/// What boot decided: the features that may run, and the switches that were refused.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub features: ModelPlaneFeatures,
    pub refusals: Vec<Refusal>,
}

/// Decides every feature. `minted` says whether Auth Core mints quarry-edge's Model Plane identity
/// (`QUARRY_EDGE__QUARRY_SERVICE_API_KEY` is set). `granted` says whether the manifest grants a row.
pub fn decide(cfg: &EdgeConfig, minted: bool, granted: &dyn Fn(&str) -> bool) -> Decision {
    let mut decision = Decision::default();
    for feature in FEATURES {
        if !feature.requested(cfg) {
            continue;
        }
        match path_refusal(cfg, minted, feature.rows(), granted) {
            None => feature.enable(&mut decision.features),
            Some(reason) => decision.refusals.push(Refusal { feature, reason }),
        }
    }
    decision
}

/// Why a feature that was asked for has no model path, or `None` when it has one.
fn path_refusal(
    cfg: &EdgeConfig,
    minted: bool,
    rows: &[&str],
    granted: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let has_url = cfg
        .model_plane_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    if !has_url {
        return Some("MODEL_PLANE_URL is unset".to_owned());
    }
    if minted {
        let refused: Vec<&str> = rows.iter().copied().filter(|row| !granted(*row)).collect();
        if refused.is_empty() {
            return None;
        }
        return Some(format!(
            "the cross-plane token-request manifest does not grant quarry-edge {}, so Auth Core refuses each call",
            refused.join(", ")
        ));
    }
    let dev_token = cfg
        .model_plane_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty());
    if cfg.cross_plane_auth_dev_bypass && dev_token {
        return None;
    }
    Some(
        "no service identity: QUARRY_EDGE__QUARRY_SERVICE_API_KEY is unset and no dev bypass token is set"
            .to_owned(),
    )
}

/// Logs each refusal at error level, so a switch that was set without a model path is loud at boot.
pub fn log_refusals(refusals: &[Refusal]) {
    for refusal in refusals {
        tracing::error!(
            switch = refusal.feature.switch(),
            feature = refusal.feature.label(),
            reason = %refusal.reason,
            "model plane feature is switched on without a model path: it stays OFF, and its routes answer unavailable without calling the Model Plane"
        );
    }
}

/// Whether the shipped cross-plane token-request manifest grants `name`. A manifest that does not
/// parse grants nothing.
pub fn manifest_granted(name: &str) -> bool {
    token_request_manifest()
        .ok()
        .and_then(|manifest| manifest.row(name))
        .is_some_and(|row| row.granted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// The registry as it is shipped.
    fn shipped(name: &str) -> bool {
        manifest_granted(name)
    }

    /// A registry that grants nothing to the Model Plane.
    fn refuses_all(_: &str) -> bool {
        false
    }

    fn with_model_plane(on: impl FnOnce(&mut EdgeConfig)) -> EdgeConfig {
        let mut cfg = EdgeConfig::defaults();
        cfg.model_plane_url = Some("http://model-gateway:8080".to_owned());
        on(&mut cfg);
        cfg
    }

    fn everything_on(cfg: &mut EdgeConfig) {
        cfg.llm_classify_intent = true;
        cfg.autoprompt = true;
        cfg.page_title_enrich = true;
        cfg.answer_synthesis = true;
        cfg.ai_formats = true;
        cfg.search_suggest = true;
        cfg.model_audio = true;
    }

    #[test]
    fn every_model_plane_switch_is_off_in_the_shipped_defaults() {
        let cfg = EdgeConfig::defaults();
        assert!(!cfg.llm_classify_intent);
        assert!(!cfg.autoprompt);
        assert!(!cfg.page_title_enrich);
        assert!(!cfg.answer_synthesis);
        assert!(!cfg.ai_formats);
        assert!(!cfg.search_suggest);
        assert!(!cfg.model_audio);
    }

    #[test]
    fn nothing_is_on_or_refused_when_no_switch_is_set() {
        let decision = decide(&with_model_plane(|_| {}), true, &refuses_all);
        assert_eq!(decision, Decision::default());
    }

    #[test]
    fn a_switch_without_a_granted_row_is_refused_and_stays_off() {
        let cfg = with_model_plane(|cfg| cfg.answer_synthesis = true);
        let decision = decide(&cfg, true, &refuses_all);
        assert_eq!(decision.features, ModelPlaneFeatures::default());
        assert_eq!(decision.refusals.len(), 1);
        assert_eq!(decision.refusals[0].feature, Feature::AnswerSynthesis);
        assert!(
            decision.refusals[0].reason.contains("model-invoke"),
            "{}",
            decision.refusals[0].reason
        );
    }

    #[test]
    fn every_switch_is_refused_while_the_registry_refuses_the_model_plane() {
        let cfg = with_model_plane(everything_on);
        let decision = decide(&cfg, true, &refuses_all);
        assert_eq!(decision.features, ModelPlaneFeatures::default());
        let refused: Vec<Feature> = decision
            .refusals
            .iter()
            .map(|refusal| refusal.feature)
            .collect();
        assert_eq!(refused, FEATURES.to_vec());
    }

    #[test]
    fn a_missing_model_plane_url_is_refused_with_its_reason() {
        let mut cfg = with_model_plane(|cfg| cfg.autoprompt = true);
        cfg.model_plane_url = None;
        let decision = decide(&cfg, true, &|_| true);
        assert_eq!(decision.features, ModelPlaneFeatures::default());
        assert_eq!(decision.refusals.len(), 1);
        assert!(decision.refusals[0].reason.contains("MODEL_PLANE_URL"));
    }

    #[test]
    fn granting_the_row_is_the_only_change_a_feature_needs() {
        let cfg = with_model_plane(|cfg| cfg.answer_synthesis = true);
        let decision = decide(&cfg, true, &|name| name == "model-invoke");
        assert!(decision.refusals.is_empty());
        assert!(decision.features.answer_synthesis);
        assert!(!decision.features.autoprompt);
    }

    #[test]
    fn audio_needs_its_own_grant_not_the_invoke_one() {
        let cfg = with_model_plane(|cfg| cfg.model_audio = true);
        let invoke_only = decide(&cfg, true, &|name| name == "model-invoke");
        assert_eq!(invoke_only.refusals.len(), 1);
        assert!(invoke_only.refusals[0].reason.contains("model-audio"));
        let audio = decide(&cfg, true, &|name| name == "model-audio");
        assert!(audio.features.model_audio);
    }

    #[test]
    fn the_dev_bypass_token_is_a_path_that_needs_no_mint() {
        let cfg = with_model_plane(|cfg| {
            cfg.autoprompt = true;
            cfg.cross_plane_auth_dev_bypass = true;
            cfg.model_plane_token = Some("dev-only-token".to_owned());
        });
        let decision = decide(&cfg, false, &refuses_all);
        assert!(decision.refusals.is_empty());
        assert!(decision.features.autoprompt);
    }

    #[test]
    fn a_switch_with_no_identity_at_all_is_refused() {
        let cfg = with_model_plane(|cfg| cfg.autoprompt = true);
        let decision = decide(&cfg, false, &refuses_all);
        assert!(!decision.features.autoprompt);
        assert_eq!(decision.refusals.len(), 1);
        assert!(decision.refusals[0].reason.contains("service identity"));
    }

    #[test]
    fn the_shipped_registry_refuses_every_model_plane_row_today() {
        // Pinned on purpose. Flipping a row is the one change that lets its feature run, so this
        // expectation is updated with that flip; the refusal tests above do not depend on it.
        assert!(!shipped("model-invoke"));
        assert!(!shipped("model-audio"));
        let cfg = with_model_plane(everything_on);
        assert_eq!(
            decide(&cfg, true, &shipped).features,
            ModelPlaneFeatures::default()
        );
    }

    #[test]
    fn every_row_a_feature_names_exists_in_the_shipped_manifest() {
        let manifest = quarry_runtime::service_tokens::token_request_manifest()
            .expect("the shipped manifest parses");
        for feature in FEATURES {
            for name in feature.rows() {
                assert!(
                    manifest.row(name).is_some(),
                    "{} names the row {name}, which the manifest lacks",
                    feature.switch()
                );
            }
        }
    }

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_refusal_is_logged_at_error_level_and_names_its_switch() {
        let sink = Capture(Arc::new(Mutex::new(Vec::new())));
        let writer = sink.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::ERROR)
            .with_ansi(false)
            .finish();
        let refusals = vec![Refusal {
            feature: Feature::Autoprompt,
            reason: "MODEL_PLANE_URL is unset".to_owned(),
        }];
        tracing::subscriber::with_default(subscriber, || log_refusals(&refusals));
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("ERROR"), "{text}");
        assert!(text.contains("QUARRY_EDGE__AUTOPROMPT"), "{text}");
        assert!(text.contains("MODEL_PLANE_URL is unset"), "{text}");
        assert!(text.contains("stays OFF"), "{text}");
    }
}
