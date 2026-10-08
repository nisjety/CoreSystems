use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use inference_client_rs::{
    Clock, InferenceError, InferenceTokenClient, Reason, RetentionPosture, SystemClock,
    TokenClientConfig,
};
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use usage_ledger_rs::{Feature, LedgerSink};
use uuid::Uuid;

use crate::config::Config;
use crate::model::ExtractionResult;

// Model Plane inference client (compiled by build.rs from the shared
// InferenceCore proto). Graph LLM extraction routes through inference-core's
// Infer RPC and nowhere else: there is no direct-Azure backend (see `GraphExtractor::new`).
// The generated documentation of the shared Model Plane proto is not this crate's to reflow.
#[allow(clippy::doc_lazy_continuation)]
pub(crate) mod model_plane {
    pub mod v1 {
        tonic::include_proto!("model_plane.v1");
    }
}

const SYSTEM_PROMPT: &str =
    "You are a knowledge graph extraction engine. Extract entities, relationships, and claims. Return only valid JSON.";

/// The organization is zero-retention (the bearer's `zdr` claim says so): nothing is sent. A
/// typed refusal, so a caller that forgot to ask first still cannot make the call, and the
/// outcome is a skip with a marker and not a failure.
#[derive(Debug)]
pub struct ZdrOrganization;

impl fmt::Display for ZdrOrganization {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("graph extraction withheld: the organization is zero-retention")
    }
}

impl std::error::Error for ZdrOrganization {}

/// The client's own deadline for one inference call ran out.
#[derive(Debug)]
pub struct ExtractionTimeout {
    pub after: Duration,
}

impl fmt::Display for ExtractionTimeout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "model-plane inference timed out after {:?}",
            self.after
        )
    }
}

impl std::error::Error for ExtractionTimeout {}

/// The model answered and the answer is not an extraction. Carries no detail on purpose: a
/// parse error can quote the text it choked on, and that text is derived from a document.
#[derive(Debug)]
pub struct InvalidModelOutput;

impl fmt::Display for InvalidModelOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the model's answer is not a graph extraction")
    }
}

impl std::error::Error for InvalidModelOutput {}

/// The model answered with nothing.
#[derive(Debug)]
pub struct EmptyModelOutput;

impl fmt::Display for EmptyModelOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("model-plane inference returned empty content")
    }
}

impl std::error::Error for EmptyModelOutput {}

/// What the organization's retention posture is, as the bearer it would call under says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Posture {
    /// The organization is zero-retention. Nothing may be sent.
    pub zdr: bool,
}

/// An extraction, with what actually served it (`InferResponse.model_used` and
/// `provider_used`), which is what the chunk's state is stamped with.
#[derive(Clone, Debug)]
pub struct Extracted {
    pub result: ExtractionResult,
    pub model: String,
    pub provider: String,
}

/// How a request reaches inference-core. The one place a test stands in: it hands back what
/// inference-core's own statuses and responses look like, and counts the calls it was asked
/// for. Everything above it (the bearer, the posture, the metering, the typed errors) is real.
enum Transport {
    Grpc(model_plane::v1::inference_core_client::InferenceCoreClient<Channel>),
    #[cfg(test)]
    Canned(Arc<dyn crate::testing::CannedInference>),
}

impl Transport {
    async fn infer(
        &self,
        request: tonic::Request<model_plane::v1::InferRequest>,
    ) -> Result<tonic::Response<model_plane::v1::InferResponse>, tonic::Status> {
        match self {
            Self::Grpc(client) => client.clone().infer(request).await,
            #[cfg(test)]
            Self::Canned(canned) => match canned.infer(request) {
                crate::testing::Canned::Answer(response) => Ok(tonic::Response::new(response)),
                crate::testing::Canned::Fail(status) => Err(status),
                crate::testing::Canned::Hang => std::future::pending().await,
            },
        }
    }
}

pub struct GraphExtractor {
    inner: ModelPlaneExtractor,
    max_entities: usize,
    /// Where what a call cost is recorded. `None` while `USAGE_EMIT_ENABLED` is off.
    ledger: Option<Arc<dyn LedgerSink>>,
}

struct ModelPlaneExtractor {
    transport: Transport,
    model: String,
    provider: String,
    timeout: Duration,
    token_client: InferenceTokenClient,
}

impl GraphExtractor {
    /// Graph extraction calls inference-core and nothing else. The direct-Azure backend this
    /// service used to carry (`GRAPH_EXTRACTION_PROVIDER=azure_openai`) is gone: flipping that
    /// one variable bypassed the Auth Core service token, the organization's retention
    /// posture, metering and the typed errors. A deployment that still asks for it is a startup
    /// error that says so, not a service that quietly runs on the model plane.
    pub fn new(cfg: &Config) -> anyhow::Result<Self> {
        Self::with_clock(cfg, Arc::new(SystemClock))
    }

    fn with_clock(cfg: &Config, clock: Arc<dyn Clock>) -> anyhow::Result<Self> {
        let inner = match normalize_provider(&cfg.extraction_provider).as_str() {
            "model_plane" => {
                let timeout = Duration::from_millis(cfg.model_plane_extraction_timeout_ms.max(1));
                let channel = Endpoint::from_shared(cfg.model_plane_ai_core_grpc_url.clone())
                    .with_context(|| {
                        format!(
                            "invalid MODEL_PLANE_AI_CORE_GRPC_URL `{}`",
                            cfg.model_plane_ai_core_grpc_url
                        )
                    })?
                    .connect_timeout(timeout)
                    .timeout(timeout)
                    .connect_lazy();
                // Strict in a real deployment: a missing credential is a startup
                // error. Standalone startup (`APP_ENV=standalone`) binds without
                // the principal and leaves the client unusable, so every call
                // fails closed instead of reaching inference-core unauthenticated.
                let token_client = InferenceTokenClient::with_clock(
                    TokenClientConfig {
                        token_url: cfg.model_plane_inference_token_url.clone(),
                        expected_issuer: cfg.model_plane_inference_token_issuer.clone(),
                        service_id: cfg.model_plane_inference_service_id.clone(),
                        service_api_key: cfg.model_plane_inference_service_api_key.clone(),
                        posture: RetentionPosture::parse(
                            &cfg.model_plane_inference_retention_posture,
                        )?,
                        allow_unconfigured: standalone_startup(),
                    },
                    clock,
                )?;
                ModelPlaneExtractor {
                    transport: Transport::Grpc(
                        model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
                    ),
                    model: cfg.model_plane_extraction_model.clone(),
                    provider: cfg.model_plane_extraction_provider.clone(),
                    timeout,
                    token_client,
                }
            }
            "azure_openai" => anyhow::bail!(
                "GRAPH_EXTRACTION_PROVIDER=azure_openai is no longer supported: graph extraction \
                 calls inference-core only. The direct-Azure backend bypassed the Auth Core service \
                 token, the organization's retention posture, metering and the typed errors. Unset \
                 GRAPH_EXTRACTION_PROVIDER (or set it to model_plane) and remove AZURE_OPENAI_API_KEY \
                 and AZURE_OPENAI_ENDPOINT from this service's environment"
            ),
            other => anyhow::bail!(
                "unsupported GRAPH_EXTRACTION_PROVIDER `{other}`; the only backend is `model_plane`"
            ),
        };
        Ok(Self {
            inner,
            max_entities: cfg.max_entities_per_chunk,
            ledger: None,
        })
    }

    /// An extractor whose requests are answered by `canned` instead of inference-core, under a
    /// clock the test controls. The bearer, the posture, the metering and the typed errors are
    /// the real ones.
    #[cfg(test)]
    pub(crate) fn with_canned_transport(
        cfg: &Config,
        clock: Arc<dyn Clock>,
        canned: Arc<dyn crate::testing::CannedInference>,
    ) -> anyhow::Result<Self> {
        let mut extractor = Self::with_clock(cfg, clock)?;
        extractor.inner.transport = Transport::Canned(canned);
        Ok(extractor)
    }

    /// Record what each call cost in the usage ledger. `None` leaves metering off.
    #[must_use]
    pub fn with_ledger(mut self, ledger: Option<Arc<dyn LedgerSink>>) -> Self {
        self.ledger = ledger;
        self
    }

    /// What the organization's retention posture is, before anything is sent: one bearer under
    /// the given reason (cached, so the call that follows reuses it). A zero-retention
    /// organization gets no call at all, and a posture Auth Core could not resolve is a typed
    /// wait ([`InferenceError`] at the mint stage), neither a send nor a skip for good.
    pub async fn posture(&self, org_id: &str, reason: Reason) -> Result<Posture, InferenceError> {
        let bearer = self
            .inner
            .token_client
            .bearer(org_id, reason)
            .await
            .map_err(|error| InferenceError::from(error).during("model-plane inference bearer"))?;
        Ok(Posture { zdr: bearer.zdr() })
    }

    /// Extracts entities, relationships and claims from one chunk. The failure keeps its type:
    /// an [`InferenceError`] (with the gRPC code and the provider's kind), or one of this
    /// module's own typed errors; see `state::outcome_for_error`.
    pub async fn extract(&self, text: &str, org_id: &str) -> anyhow::Result<Extracted> {
        let prompt = build_prompt(self.max_entities, text);
        let inferred = self
            .inner
            .infer(
                org_id,
                &prompt,
                Reason::GraphExtraction,
                Feature::GraphExtraction,
                self.ledger.as_deref(),
            )
            .await?;
        let result = parse_extraction(&inferred.content).map_err(|_| InvalidModelOutput)?;
        tracing::info!(
            org_id,
            model = %inferred.model,
            provider = %inferred.provider,
            entities = result.entities.len(),
            relationships = result.relationships.len(),
            claims = result.claims.len(),
            "extraction complete"
        );
        Ok(Extracted {
            result,
            model: inferred.model,
            provider: inferred.provider,
        })
    }

    /// Summarises one GraphRAG community from its member entities (plan P1-5).
    ///
    /// This is GraphRAG's "global search" input: a short thematic description of
    /// a cluster, which answers questions no single chunk can ("what areas does
    /// this organisation work in?"). `graph_communities.summary` and
    /// `retrieval-engine`'s `community_summary_search` already existed; only the
    /// generator was missing.
    ///
    /// Under the same bearer rules as [`GraphExtractor::extract`], with its own reason
    /// (`graph-community-summary`) and its own ledger feature: a zero-retention organization's
    /// bearer refuses the call here too ([`ZdrOrganization`]), which is what keeps that
    /// guarantee from resting on the assumption that graph entities only ever exist for
    /// non-restrictive content.
    ///
    /// Returns prose, not JSON — there is nothing to parse, so a provider that
    /// wraps or chats around the answer degrades to a slightly noisier summary
    /// rather than a hard failure.
    pub async fn summarize_community(
        &self,
        entity_labels: &[String],
        org_id: &str,
    ) -> anyhow::Result<String> {
        if entity_labels.is_empty() {
            anyhow::bail!("cannot summarise a community with no member entities");
        }
        let prompt = build_community_summary_prompt(entity_labels);
        let inferred = self
            .inner
            .infer(
                org_id,
                &prompt,
                Reason::GraphCommunitySummary,
                Feature::GraphCommunitySummary,
                self.ledger.as_deref(),
            )
            .await?;
        let summary = inferred.content.trim().to_string();
        if summary.is_empty() {
            return Err(EmptyModelOutput.into());
        }
        Ok(summary)
    }
}

/// What one inference call came back with.
struct Inferred {
    content: String,
    /// `InferResponse.model_used`, or the model asked for when the response named none.
    model: String,
    provider: String,
}

/// Caps how many member labels are sent for one summary.
///
/// A community can span hundreds of entities; sending all of them would blow the
/// prompt budget and cost for no gain, since a thematic summary is determined by
/// the dominant members. Labels arrive ordered by the caller (highest-confidence
/// first), so truncation drops the weakest.
const MAX_SUMMARY_LABELS: usize = 60;

fn build_community_summary_prompt(entity_labels: &[String]) -> String {
    let shown: Vec<&str> = entity_labels
        .iter()
        .take(MAX_SUMMARY_LABELS)
        .map(String::as_str)
        .collect();
    let omitted = entity_labels.len().saturating_sub(shown.len());
    let more = if omitted > 0 {
        format!("\n(and {omitted} further related entities)")
    } else {
        String::new()
    };
    format!(
        r#"These entities form one cluster in a knowledge graph built from an organisation's own documents:

{labels}{more}

Write 2-3 sentences describing what this cluster is about, as a factual topic
summary someone could use to decide whether it is relevant to their question.

Rules:
- Use ONLY the entities listed. Do not add facts, figures, or claims that are not present.
- If the entities are too disparate to share a theme, say exactly that.
- Answer in the dominant language of the entity names.
- Return the summary text only — no preamble, no headings, no bullet points."#,
        labels = shown.join("\n"),
        more = more,
    )
}

fn standalone_startup() -> bool {
    std::env::var("APP_ENV")
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("standalone"))
        .unwrap_or(false)
}

impl ModelPlaneExtractor {
    /// Build the Infer request. Split out so a unit test can assert the prompt +
    /// routing hints are carried onto the wire without a live inference-core.
    fn build_request(
        &self,
        org_id: &str,
        prompt: &str,
        zdr: bool,
    ) -> model_plane::v1::InferRequest {
        model_plane::v1::InferRequest {
            request_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            model: self.model.clone(),
            provider_hint: self.provider.clone(),
            messages: vec![
                model_plane::v1::ChatMessage {
                    role: "system".to_string(),
                    content: SYSTEM_PROMPT.to_string(),
                    name: String::new(),
                    // Provider-native compaction checkpoint; only assistant
                    // history ever carries one, never this two-message prompt.
                    compaction_summary: String::new(),
                },
                model_plane::v1::ChatMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                    name: String::new(),
                    compaction_summary: String::new(),
                },
            ],
            temperature: 0.1,
            max_tokens: 4096,
            structured_output_schema: String::new(),
            zdr,
            tools: vec![],
            tool_choice: String::new(),
            // The three fields the merged inference contract added. All set to
            // their zero values, which the proto documents as byte-identical to
            // the pre-existing behaviour: UNSPECIFIED imposes no privacy-tier
            // constraint, an empty `min_residency` imposes no residency floor,
            // and a 0 thinking budget requests no extended thinking.
            //
            // Deliberately NOT tightened here. Both are real provider-selection
            // controls — a tier or residency floor SKIPS providers that cannot
            // meet it and fails the call rather than downgrading silently — so
            // raising them is a deployment posture decision, not a build fix.
            // This path's retention posture is already asserted at mint time via
            // MODEL_PLANE_INFERENCE_RETENTION_POSTURE, and `zdr` above still
            // gates retaining providers. Under the `tenant` posture the
            // organization's own setting is the bearer's `zdr` claim, and
            // `infer` sends nothing for a zero-retention organization, so the
            // `zdr` here is always the claim that was honoured. Revisit when the
            // sovereignty posture is set per-tenant rather than per-service.
            min_privacy_tier: model_plane::v1::PrivacyTier::Unspecified as i32,
            min_residency: String::new(),
            thinking_budget_tokens: 0,
            // Subscription routing belongs to a user's own chat, never to
            // background extraction: no subscription connection, normal tier.
            subscription_connection_id: String::new(),
            prefer_priority_service_tier: false,
        }
    }

    fn build_authenticated_request(
        &self,
        bearer: &str,
        org_id: &str,
        prompt: &str,
        zdr: bool,
    ) -> anyhow::Result<tonic::Request<model_plane::v1::InferRequest>> {
        anyhow::ensure!(
            !bearer.trim().is_empty()
                && bearer == bearer.trim()
                && !bearer.chars().any(char::is_whitespace),
            "verified bearer is required"
        );
        anyhow::ensure!(
            !org_id.trim().is_empty() && org_id == org_id.trim(),
            "org_id is required"
        );

        let mut request = tonic::Request::new(self.build_request(org_id, prompt, zdr));
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {bearer}"))?,
        );
        Ok(request)
    }

    /// One inference call. The bearer is taken from the token client at the moment
    /// of the call (its cache makes that free), with the `reason` of the feature
    /// that asked: extraction and community summaries are audited separately.
    ///
    /// **Zero retention.** The bearer's `zdr` claim is the organization's own setting under
    /// the `tenant` posture. A zero-retention organization gets no request at all
    /// ([`ZdrOrganization`]): this is the last line, behind the gate every caller consults
    /// first, so a caller that forgot to ask cannot send.
    ///
    /// **Metering.** What the call cost is recorded as soon as the provider has answered, before
    /// the answer is looked at and before anything is written to the database, so nothing that
    /// fails after the call (an unparsable answer, a database error) can lose the spend. A call
    /// that failed before an answer (a refusal, a timeout) cost nothing and records nothing.
    ///
    /// Failures keep their type. A refusal to issue a bearer or a status from
    /// inference-core becomes an `InferenceError` that is the error itself, not a
    /// context wrapped around it: callers log `%e`, and a bare context printed
    /// only "model-plane inference failed", with the gRPC code, the typed kind
    /// and the cause nowhere in the log. A caller that has to decide what to do
    /// with the work recovers the type with `downcast_ref::<InferenceError>()`
    /// (a posture Auth Core could not resolve is a wait, not a failure).
    async fn infer(
        &self,
        org_id: &str,
        prompt: &str,
        reason: Reason,
        feature: Feature,
        ledger: Option<&dyn LedgerSink>,
    ) -> anyhow::Result<Inferred> {
        let bearer = self
            .token_client
            .bearer(org_id, reason)
            .await
            .map_err(|error| {
                anyhow::Error::new(
                    InferenceError::from(error).during("model-plane inference bearer"),
                )
            })?;
        if bearer.zdr() {
            return Err(ZdrOrganization.into());
        }
        let request = self.build_authenticated_request(bearer.as_str(), org_id, prompt, false)?;
        let body = tokio::time::timeout(self.timeout, self.transport.infer(request))
            .await
            .map_err(|_| ExtractionTimeout {
                after: self.timeout,
            })?
            .map_err(|status| {
                anyhow::Error::new(
                    InferenceError::from_status(&status).during("model-plane inference"),
                )
            })?
            .into_inner();
        if let Some(ledger) = ledger {
            crate::usage::record_call(ledger, feature, org_id, bearer.zdr(), &body).await;
        }
        if body.content.trim().is_empty() {
            return Err(EmptyModelOutput.into());
        }
        // What served the call, as the response says. A response that names nothing is stamped
        // `unknown`: valid, and visibly wrong, where the model that was ASKED for would read as
        // a fact about what ran.
        let named = |value: &str| {
            let value = value.trim();
            if value.is_empty() {
                "unknown".to_owned()
            } else {
                value.to_owned()
            }
        };
        Ok(Inferred {
            model: named(&body.model_used),
            provider: named(&body.provider_used),
            content: body.content,
        })
    }
}

fn build_prompt(max_entities: usize, text: &str) -> String {
    // `entity_type` is constrained to the closed ontology (P1-3). Left free, the
    // extractor produced 61 distinct types across 547 entities — including
    // `Aquatiq` as both `Organization` and `Company`, which split one company
    // into two unrelated graph nodes.
    //
    // This prompt only *steers*: there is no provider-side enum here (the schema
    // is prose, and the Anthropic path drops structured-output schemas), so
    // `store::canonical_entity_type` remains the authoritative gate and maps
    // anything unrecognised to `Concept`.
    format!(
        r#"Extract entities, relationships, and claims from the following text.
Return JSON with this schema:
{{
  "entities": [{{ "entity_type": "...", "entity_text": "...", "confidence": 0.0-1.0 }}],
  "relationships": [{{ "source_entity": "...", "target_entity": "...", "relation_type": "...", "confidence": 0.0-1.0 }}],
  "claims": [{{ "claim_text": "...", "related_entities": ["..."], "confidence": 0.0-1.0 }}]
}}

"entity_type" MUST be exactly one of these {type_count} values — never invent another:
{types}

Pick the closest match. Use "Concept" for anything abstract that fits no other
type (an industry, a field, a risk, a property). Do not emit "Claim" as an
entity type; assertions belong in "claims".

Max {max} entities. Only return valid JSON.

Text:
{text}"#,
        type_count = crate::store::ENTITY_TYPES.len(),
        types = crate::store::ENTITY_TYPES.join(", "),
        max = max_entities,
        text = text
    )
}

/// Parse an extraction result, tolerating a ```json fenced``` wrapper — the Infer
/// path does not force a JSON response format, so some providers wrap output.
fn parse_extraction(content: &str) -> anyhow::Result<ExtractionResult> {
    let cleaned = strip_json_fences(content);
    serde_json::from_str(&cleaned).context("parse extraction result")
}

fn strip_json_fences(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches(['\n', '\r']);
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "model-plane" | "modelplane" | "inference-core" | "inference_core" | "ai-core"
        | "ai_core" => "model_plane".to_string(),
        "azure" | "azure-openai" => "azure_openai".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_summary_prompt_lists_members_and_forbids_invention() {
        let labels = vec![
            "Aquatiq".to_string(),
            "Hygieneanlegg".to_string(),
            "Næringsmiddelindustri".to_string(),
        ];
        let p = build_community_summary_prompt(&labels);
        for l in &labels {
            assert!(p.contains(l.as_str()), "prompt must list {l}");
        }
        // Grounding rules: the summary must not become a source of new facts,
        // and it must not silently invent a theme for unrelated members.
        assert!(p.contains("ONLY the entities listed"));
        assert!(p.contains("too disparate"));
        assert!(p.contains("dominant language"), "corpus is NO+EN mixed");
        assert!(
            !p.contains("further related entities"),
            "nothing omitted at n=3"
        );
    }

    #[test]
    fn community_summary_prompt_truncates_and_says_so() {
        // A community can span hundreds of entities; the prompt must cap them
        // and disclose the omission rather than silently dropping members.
        let labels: Vec<String> = (0..MAX_SUMMARY_LABELS + 25)
            .map(|i| format!("entity-{i}"))
            .collect();
        let p = build_community_summary_prompt(&labels);
        assert!(p.contains("entity-0"), "highest-ranked member kept");
        assert!(
            !p.contains(&format!("entity-{}", MAX_SUMMARY_LABELS + 24)),
            "weakest member beyond the cap must be dropped"
        );
        assert!(
            p.contains("and 25 further related entities"),
            "truncation must be disclosed to the model"
        );
    }

    /// A deployment's configuration as `Config::from_env` reads it from an empty environment:
    /// every field at its default, the extractor's own overrides on top.
    fn test_config(provider: &str) -> Config {
        let mut config: Config = envy::from_iter(Vec::<(String, String)>::new()).expect("config");
        config.extraction_provider = provider.to_string();
        config.model_plane_extraction_model = "gpt-4o".to_string();
        config.model_plane_extraction_provider = "azure_openai".to_string();
        config.model_plane_inference_token_url =
            "http://auth-core:3011/api/inference-core/internal-token".to_string();
        config.model_plane_inference_token_issuer =
            "http://auth-core:3011/api/convex-auth".to_string();
        config.model_plane_inference_service_id = "graph-index".to_string();
        config.model_plane_inference_service_api_key =
            "isolated-test-service-credential".to_string();
        config
    }

    #[tokio::test]
    async fn new_defaults_to_model_plane() {
        GraphExtractor::new(&test_config("model_plane")).expect("model-plane extractor");
        // The aliases a deployment may spell it with.
        for alias in ["inference-core", "Model_Plane", "ai_core"] {
            GraphExtractor::new(&test_config(alias))
                .unwrap_or_else(|error| panic!("{alias}: {error}"));
        }
    }

    #[tokio::test]
    async fn model_plane_requires_service_principal_credential() {
        let mut config = test_config("model_plane");
        config.model_plane_inference_service_api_key.clear();
        let error = match GraphExtractor::new(&config) {
            Ok(_) => panic!("model-plane startup must fail closed without a credential"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("SERVICE_API_KEY"));
    }

    /// The direct-Azure backend is deleted. Selecting it is a startup error that names what to
    /// remove, not a service that quietly runs somewhere else, and there is no way left to reach
    /// a provider without the service token, the posture, the metering and the typed errors.
    #[tokio::test]
    async fn the_direct_azure_backend_is_gone_and_asking_for_it_is_a_startup_error() {
        for spelling in ["azure_openai", "azure-openai", "Azure"] {
            let error = match GraphExtractor::new(&test_config(spelling)) {
                Ok(_) => panic!("{spelling}: the Azure backend must not start"),
                Err(error) => error.to_string(),
            };
            assert!(
                error.contains("no longer supported") && error.contains("AZURE_OPENAI_API_KEY"),
                "{spelling}: {error}"
            );
        }
        let error = match GraphExtractor::new(&test_config("openai")) {
            Ok(_) => panic!("an unknown backend must not start"),
            Err(error) => error.to_string(),
        };
        assert!(
            error.contains("the only backend is `model_plane`"),
            "{error}"
        );
    }

    /// And the source says it too: nothing in this module can reach a provider except through
    /// inference-core's `Infer` RPC. A test over the text is crude, and exactly as strong as the
    /// regression it guards: someone adding an HTTP client back.
    #[test]
    fn the_module_has_no_way_to_call_a_provider_directly() {
        let source = include_str!("extractor.rs");
        let production = source
            .split("mod tests {")
            .next()
            .expect("the production part of the file");
        for forbidden in [
            "reqwest",
            "api-key",
            "chat/completions",
            "AzureExtractor",
            "azure_openai_api_key",
            "azure_openai_endpoint",
        ] {
            assert!(
                !production.contains(forbidden),
                "extractor.rs mentions `{forbidden}`: a direct provider path is back"
            );
        }
    }

    #[test]
    fn normalize_provider_aliases() {
        assert_eq!(normalize_provider("inference-core"), "model_plane");
        assert_eq!(normalize_provider("Model_Plane"), "model_plane");
        assert_eq!(normalize_provider("azure-openai"), "azure_openai");
    }

    #[tokio::test]
    async fn model_plane_request_carries_prompt_and_routing() {
        let extractor = GraphExtractor::new(&test_config("model_plane")).unwrap();
        let inner = &extractor.inner;
        let prompt = build_prompt(20, "some chunk text");
        let req = inner.build_request("org-9", &prompt, false);
        assert_eq!(req.org_id, "org-9");
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.provider_hint, "azure_openai");
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, "system");
        assert_eq!(req.messages[1].role, "user");
        assert!(req.messages[1].content.contains("some chunk text"));
        assert!(req.messages[1].content.contains("Only return valid JSON"));
    }

    #[tokio::test]
    async fn model_plane_authenticated_request_uses_bearer_exact_org_and_zdr_without_shared_key() {
        let extractor = GraphExtractor::new(&test_config("model_plane")).unwrap();
        let inner = &extractor.inner;
        let prompt = build_prompt(20, "isolated test text");
        let request = inner
            .build_authenticated_request("header.payload.signature", "org-9", &prompt, true)
            .expect("authenticated request");

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer header.payload.signature")
        );
        assert!(request.metadata().get("x-api-key").is_none());
        assert_eq!(request.get_ref().org_id, "org-9");
        assert!(
            request.get_ref().zdr,
            "restrictive ZDR posture must survive wrapping"
        );

        let non_zdr = inner
            .build_authenticated_request("header.payload.signature", "org-9", &prompt, false)
            .expect("authenticated request");
        assert!(
            !non_zdr.get_ref().zdr,
            "caller ZDR posture must not be overwritten"
        );
    }

    #[tokio::test]
    async fn model_plane_authenticated_request_fails_closed_without_bearer_or_tenant() {
        let extractor = GraphExtractor::new(&test_config("model_plane")).unwrap();
        let inner = &extractor.inner;
        let prompt = build_prompt(20, "isolated test text");

        for bearer in ["", "  ", "header payload"] {
            let error = inner
                .build_authenticated_request(bearer, "org-9", &prompt, false)
                .expect_err("malformed bearer must fail closed");
            assert!(error.to_string().contains("bearer"));
        }
        let error = inner
            .build_authenticated_request("header.payload.signature", "", &prompt, false)
            .expect_err("missing tenant must fail closed");
        assert!(error.to_string().contains("org_id"));
    }

    /// Auth Core when it cannot resolve an organization's retention posture: 503
    /// `retention_posture_unavailable` and no token (the body Auth Core really
    /// sends; see `inference-service-tokens.golden.json`, case
    /// `tenantPostureUnavailable`, in `services/inference-client-rs`). A transport
    /// and nothing else. Returns the token URL and the request bodies received.
    async fn posture_unavailable_auth_core() -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = seen.clone();
        let app = axum::Router::new().route(
            "/api/inference-core/internal-token",
            axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
                let recorded = recorded.clone();
                async move {
                    recorded.lock().expect("recorded requests").push(body);
                    (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        axum::Json(serde_json::json!({
                            "statusCode": 503,
                            "error": "retention_posture_unavailable",
                            "message": "The organization retention posture could not be verified. Retry shortly."
                        })),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("local address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (
            format!("http://{address}/api/inference-core/internal-token"),
            seen,
        )
    }

    /// Extraction and community summaries are audited under their own reasons,
    /// and a refusal to issue a bearer stays typed all the way out of the
    /// extractor. A posture Auth Core could not resolve is a wait, not a failure,
    /// and what the logs show (`%e`) names the kind. The graph state machine
    /// (CR-T12) decides what pending means for a document; this is the type it
    /// reads.
    #[tokio::test]
    async fn each_feature_asks_for_its_own_reason_and_a_posture_wait_stays_typed() {
        let (token_url, seen) = posture_unavailable_auth_core().await;
        let mut config = test_config("model_plane");
        config.model_plane_inference_token_url = token_url;
        let extractor = GraphExtractor::new(&config).expect("model-plane extractor");

        let error = extractor
            .extract("some chunk text", "org-9")
            .await
            .expect_err("no bearer, no extraction");
        let typed = error
            .downcast_ref::<inference_client_rs::InferenceError>()
            .expect("the failure keeps its type");
        assert_eq!(
            typed.failure(),
            &inference_client_rs::Failure::PostureUnavailable
        );
        assert_eq!(
            typed.disposition(),
            inference_client_rs::Disposition::Pending {
                after: Duration::from_secs(300)
            }
        );
        // The line the callers log with `%e` says what happened. It used to say
        // only "model-plane inference failed", with the code nowhere in it.
        assert!(error.to_string().contains("posture_unavailable"), "{error}");

        let labels = vec!["Aquatiq".to_string(), "Hygieneanlegg".to_string()];
        extractor
            .summarize_community(&labels, "org-9")
            .await
            .expect_err("no bearer, no summary");

        let reasons: Vec<String> = seen
            .lock()
            .expect("recorded requests")
            .iter()
            .map(|body| body["reason"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(reasons, ["graph-extraction", "graph-community-summary"]);
    }

    #[test]
    fn parse_extraction_handles_fenced_and_bare_json() {
        let bare = r#"{"entities":[{"entity_type":"org","entity_text":"Acme","confidence":0.9}],"relationships":[],"claims":[]}"#;
        let parsed = parse_extraction(bare).expect("bare json");
        assert_eq!(parsed.entities.len(), 1);

        let fenced = format!("```json\n{bare}\n```");
        let parsed2 = parse_extraction(&fenced).expect("fenced json");
        assert_eq!(parsed2.entities.len(), 1);
    }

    /// A model that has no relationships to report and leaves the key out has answered: that is
    /// not a failure to parse, which would count toward the chunk's poison cap. What is not JSON
    /// at all is.
    #[test]
    fn a_missing_list_is_an_empty_list_and_prose_is_not_an_extraction() {
        let partial = parse_extraction(
            r#"{"entities":[{"entity_type":"org","entity_text":"Acme","confidence":0.9}]}"#,
        )
        .expect("an answer without relationships");
        assert_eq!(
            (
                partial.entities.len(),
                partial.relationships.len(),
                partial.claims.len()
            ),
            (1, 0, 0)
        );
        assert!(
            parse_extraction("{}").is_ok(),
            "an answer with nothing in it is an empty extraction"
        );
        assert!(parse_extraction("I could not find anything in this text.").is_err());
        assert!(parse_extraction("").is_err());
    }

    /// The prompt is a version. Every chunk's state says which prompt produced it, and an `empty`
    /// chunk is attempted again when the version changes, so changing the text of the prompt (or
    /// the ontology it lists) without changing `PROMPT_VERSION` would let two prompts share a
    /// name. This fails when the text changes: bump the version in `state.rs`, then update the
    /// fingerprint.
    #[test]
    fn the_prompt_text_is_pinned_to_its_version() {
        use sha2::{Digest, Sha256};
        // Line endings are the checkout's business, not the prompt's.
        let text =
            format!("{SYSTEM_PROMPT}\n{}", build_prompt(20, "<chunk>")).replace("\r\n", "\n");
        assert_eq!(
            format!("{:x}", Sha256::digest(text.as_bytes())),
            PROMPT_FINGERPRINT,
            "the extraction prompt changed: bump PROMPT_VERSION (state.rs) and update PROMPT_FINGERPRINT"
        );
        assert_eq!(crate::state::PROMPT_VERSION, "graph-v1");
    }

    /// sha256 of the system prompt, a newline and the user prompt for 20 entities and the
    /// text `<chunk>`, which includes the closed ontology.
    const PROMPT_FINGERPRINT: &str =
        "c040082803a6114f21f04d94a2be1ae151d82bd3cb395324eeef6285b4f4eb62";

    // ── end to end through the real bearer client ────────────────────────────

    use crate::testing::{
        answer, capture_logs, config_for, fail, GoldenAuthCore, ManualClock, Scripted,
        EMPTY_EXTRACTION, ORG_PLAIN, ORG_ZDR, RICH_EXTRACTION,
    };
    use usage_ledger_rs::MemoryLedger;

    struct Rig {
        auth: GoldenAuthCore,
        inference: Arc<Scripted>,
        ledger: Arc<MemoryLedger>,
        extractor: GraphExtractor,
    }

    async fn rig_with(timeout_ms: u64) -> Rig {
        let auth = GoldenAuthCore::start().await;
        let inference = Scripted::new();
        let ledger = Arc::new(MemoryLedger::new());
        let mut config = config_for(&auth);
        config.model_plane_extraction_timeout_ms = timeout_ms;
        let extractor = GraphExtractor::with_canned_transport(
            &config,
            ManualClock::at_golden_mint(),
            inference.clone(),
        )
        .expect("an extractor over the stand-ins")
        .with_ledger(Some(ledger.clone() as Arc<dyn LedgerSink>));
        Rig {
            auth,
            inference,
            ledger,
            extractor,
        }
    }

    async fn rig() -> Rig {
        rig_with(60_000).await
    }

    /// The request carries the bearer Auth Core minted for this organization and reason, the
    /// organization's id and no zero-retention flag; what came back is stamped with what served
    /// it and recorded in the ledger with the exact tokens.
    #[tokio::test]
    async fn an_extraction_carries_the_organizations_bearer_and_records_what_it_cost() {
        let rig = rig().await;
        rig.inference.always(answer(RICH_EXTRACTION));

        let extracted = rig
            .extractor
            .extract("a chunk of text", ORG_PLAIN)
            .await
            .expect("an extraction");

        assert_eq!(
            (
                extracted.result.entities.len(),
                extracted.result.relationships.len(),
                extracted.result.claims.len()
            ),
            (2, 1, 1)
        );
        assert_eq!(extracted.model, "gpt-4o-mini-2024-07-18");
        assert_eq!(extracted.provider, "azure-openai");
        let calls = rig.inference.calls();
        assert_eq!(calls.len(), 1);
        assert!(
            uuid::Uuid::parse_str(&calls[0].request_id).is_ok(),
            "every call has an id of its own: it is what makes its ledger event idempotent"
        );
        assert_eq!(calls[0].org_id, ORG_PLAIN);
        assert!(
            !calls[0].zdr,
            "an organization that is not zero-retention is sent with zdr false"
        );
        assert!(calls[0].prompt.contains("a chunk of text"));
        let authorization = calls[0].authorization.as_deref().expect("a bearer");
        assert!(
            authorization.starts_with("Bearer eyJ"),
            "a compact JWT from Auth Core, not a shared key"
        );
        assert_eq!(
            rig.auth.requested(),
            [(ORG_PLAIN.to_owned(), "graph-extraction".to_owned())],
            "one mint, under the reason of the feature"
        );
        let events = rig.ledger.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].feature, "graph-extraction");
        assert_eq!(
            (
                events[0].input_tokens,
                events[0].output_tokens,
                events[0].cache_read_tokens
            ),
            (612, 48, 512)
        );
        assert!(!events[0].zdr);
    }

    /// The bearer is cached: a document of many chunks asks Auth Core once.
    #[tokio::test]
    async fn many_chunks_of_one_organization_mint_one_bearer() {
        let rig = rig().await;
        rig.inference.always(answer(EMPTY_EXTRACTION));
        for _ in 0..5 {
            rig.extractor
                .extract("a chunk", ORG_PLAIN)
                .await
                .expect("an extraction");
        }
        assert_eq!(rig.inference.call_count(), 5);
        assert_eq!(
            rig.auth.requested().len(),
            1,
            "the bearer is reused for its lifetime"
        );
    }

    /// The posture of the organization is the bearer's `zdr` claim, and the extractor itself
    /// never sends for a zero-retention organization even when a caller did not ask first.
    #[tokio::test]
    async fn the_posture_is_the_bearers_claim_and_a_zero_retention_organization_is_never_sent() {
        let rig = rig().await;
        rig.inference.always(answer(RICH_EXTRACTION));

        assert_eq!(
            rig.extractor
                .posture(ORG_PLAIN, Reason::GraphExtraction)
                .await
                .expect("a posture"),
            Posture { zdr: false }
        );
        assert_eq!(
            rig.extractor
                .posture(ORG_ZDR, Reason::GraphExtraction)
                .await
                .expect("a posture"),
            Posture { zdr: true }
        );
        let error = rig
            .extractor
            .extract("the zero-retention organization's chunk", ORG_ZDR)
            .await
            .expect_err("no call for a zero-retention organization");
        assert!(error.downcast_ref::<ZdrOrganization>().is_some(), "{error}");
        let summary = rig
            .extractor
            .summarize_community(&["Aquatiq".to_owned()], ORG_ZDR)
            .await
            .expect_err("nor a summary");
        assert!(
            summary.downcast_ref::<ZdrOrganization>().is_some(),
            "{summary}"
        );
        assert_eq!(rig.inference.call_count(), 0, "zero provider calls");
        assert!(rig.ledger.events().is_empty(), "and nothing was spent");
    }

    /// A call that is answered is paid for whatever is made of the answer: it is recorded before
    /// the answer is judged, and the failure that follows is typed.
    #[tokio::test]
    async fn an_answer_that_is_not_an_extraction_is_recorded_and_typed() {
        let rig = rig().await;
        rig.inference
            .always(answer("Sorry, I cannot help with that."));
        let error = rig
            .extractor
            .extract("a chunk", ORG_PLAIN)
            .await
            .expect_err("the answer is not JSON");
        assert!(
            error.downcast_ref::<InvalidModelOutput>().is_some(),
            "{error}"
        );
        assert!(
            !error.to_string().contains("Sorry"),
            "the model's text (derived from a document) is not in the error: {error}"
        );
        assert_eq!(
            rig.ledger.events().len(),
            1,
            "spend recorded for an unusable answer"
        );

        rig.inference.always(answer("   "));
        let error = rig
            .extractor
            .extract("a chunk", ORG_PLAIN)
            .await
            .expect_err("an empty answer");
        assert!(
            error.downcast_ref::<EmptyModelOutput>().is_some(),
            "{error}"
        );
        assert_eq!(rig.ledger.events().len(), 2);
    }

    /// A call that failed before an answer cost nothing and records nothing; the failure keeps
    /// its type, with the gRPC code and the provider's kind in what is printed.
    #[tokio::test]
    async fn a_refusal_costs_nothing_and_keeps_its_type() {
        let rig = rig().await;
        rig.inference.always(fail("provider_rate_limited"));
        let error = rig
            .extractor
            .extract("a chunk", ORG_PLAIN)
            .await
            .expect_err("throttled");
        let typed = error
            .downcast_ref::<InferenceError>()
            .expect("the type survives");
        assert_eq!(
            typed.failure().kind(),
            "rate_limited",
            "decoded from the trailer, not from the message"
        );
        let printed = error.to_string();
        assert!(
            printed.contains("rate_limited") && printed.contains("grpc_code="),
            "what a log prints says what happened: {printed}"
        );
        assert!(rig.ledger.events().is_empty());
    }

    /// The extractor's own deadline: a provider that never answers is a typed timeout, not a
    /// hang, and nothing is recorded.
    #[tokio::test]
    async fn a_provider_that_never_answers_is_a_typed_timeout() {
        let rig = rig_with(40).await;
        rig.inference.always(crate::testing::Reply::Hang);
        let error = rig
            .extractor
            .extract("a chunk", ORG_PLAIN)
            .await
            .expect_err("the deadline");
        assert!(
            error.downcast_ref::<ExtractionTimeout>().is_some(),
            "{error}"
        );
        assert!(rig.ledger.events().is_empty());
    }

    /// A summary is its own feature and its own reason.
    #[tokio::test]
    async fn a_community_summary_is_recorded_under_its_own_feature_and_reason() {
        let rig = rig().await;
        rig.inference
            .always(answer("The cluster is about hygiene solutions."));
        let summary = rig
            .extractor
            .summarize_community(
                &["Aquatiq".to_owned(), "Hygieneanlegg".to_owned()],
                ORG_PLAIN,
            )
            .await
            .expect("a summary");
        assert_eq!(summary, "The cluster is about hygiene solutions.");
        assert_eq!(
            rig.auth.requested(),
            [(ORG_PLAIN.to_owned(), "graph-community-summary".to_owned())]
        );
        let events = rig.ledger.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].feature, "graph-community-summary");
        assert_eq!(events[0].event_type, "llm");
    }

    /// The token carries an organization and a posture Auth Core decided; nothing in a log line
    /// or an error the extractor produces carries the bearer.
    #[tokio::test]
    async fn the_bearer_never_reaches_a_log_line() {
        let rig = rig().await;
        rig.inference.always(answer(RICH_EXTRACTION));
        let (logs, _guard) = capture_logs();
        rig.extractor
            .extract("a chunk", ORG_PLAIN)
            .await
            .expect("an extraction");
        let bearer = rig.inference.calls()[0]
            .authorization
            .clone()
            .expect("a bearer");
        let token = bearer.trim_start_matches("Bearer ");
        assert!(!logs.text().contains(token), "the bearer is in the log");
    }
}
