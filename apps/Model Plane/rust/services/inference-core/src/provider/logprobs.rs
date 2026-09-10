//! Provider token-level certainty (logprobs) → [`TokenConfidence`].
//!
//! OpenAI-family chat completions return a `logprob` for every token the model
//! emitted when the request carries `logprobs: true`. That is the only signal
//! available anywhere in this stack that reports how sure the MODEL was, rather
//! than how well-backed its answer was by our own retrieval and tools. The
//! model-gateway's answer-quality scorer previously had no access to it and
//! could only read the finished text, so a near-certain fact ("Oslo.") and a
//! confident-sounding guess scored identically.
//!
//! This module owns three things so no provider adapter has to:
//!   * whether logprobs may be requested at all (model and operator gates),
//!   * parsing them out of a completion or a stream chunk,
//!   * reducing them to the aggregate the contract carries.
//!
//! Everything here fails soft. A provider that ignores the parameter, a model
//! that omits the field, malformed numbers — all yield `None`, which consumers
//! must read as "unknown", never as "low confidence".

use super::TokenConfidence;

/// Per-token floor.
///
/// Not hypothetical: Azure `OpenAI` (gpt-4o-mini, api-version 2025-01-01-preview)
/// was observed returning a single enormously negative logprob inside otherwise
/// ordinary answers — enough to drag a 220-token answer whose other tokens were
/// unremarkable to a geometric-mean probability of 0.000. One such token is not
/// a zero-confidence answer, and without a floor the aggregate is at the mercy
/// of it. `-20` is already a probability of ~2e-9, so the clamp bounds the
/// damage without discarding any meaningful signal.
const MIN_TOKEN_LOGPROB: f64 = -20.0;

/// Operator kill-switch for the request-side parameter.
///
/// Requesting `logprobs` is a per-deployment compatibility question, and a
/// deployment that rejects it fails the whole completion with a 400 rather than
/// degrading. `INFERENCE_LOGPROBS_ENABLED=0` turns the request parameter off
/// across every provider without a rebuild or a redeploy of the callers; the
/// default is on.
#[must_use]
pub fn logprobs_enabled() -> bool {
    !matches!(
        std::env::var("INFERENCE_LOGPROBS_ENABLED")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// Whether `model` accepts the `logprobs` request parameter.
///
/// Reasoning models (o1/o3, gpt-5) reject it outright — the sampled answer is
/// not the model's only output and the API does not expose per-token
/// probabilities for it — so asking would 400 a working completion. Callers
/// must also gate on their own endpoint flavor: only OpenAI-shaped chat
/// deployments are known to honor it.
#[must_use]
pub fn model_supports_logprobs(model: &str) -> bool {
    let normalized = model.to_ascii_lowercase();
    !(normalized.starts_with("gpt-5")
        || normalized.starts_with("o1")
        || normalized.starts_with("o3")
        || normalized.starts_with("o4"))
}

/// Accumulates token logprobs across a completion or a whole stream.
///
/// Streaming delivers logprobs the same way it delivers content — a few tokens
/// per chunk — so the summary can only be computed once the stream ends. This
/// keeps a running sum rather than the token array: the aggregate is all the
/// contract carries, and the array is both large and re-identifying (it is the
/// answer's shape in probability space).
#[derive(Debug, Clone, Default)]
pub struct LogprobAccumulator {
    sum: f64,
    count: u32,
    claim_sum: f64,
    claim_count: u32,
    /// The question, lowercased, used to recognize tokens that merely echo it.
    question: String,
}

impl LogprobAccumulator {
    /// Accumulate without a question: every content token counts as a claim.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate against the question the answer is responding to, so tokens
    /// that restate it can be told apart from tokens that assert something.
    #[must_use]
    pub fn for_question(question: &str) -> Self {
        Self {
            question: question.to_lowercase(),
            ..Self::default()
        }
    }

    /// Whether a token asserts anything.
    ///
    /// Two exclusions, both necessary. Punctuation and whitespace carry no
    /// claim and are almost always near-certain, so counting them dilutes the
    /// statistic toward 1.0. Tokens already present in the question are the
    /// answer restating the prompt — "Hovedstaden i Portugal er" is equally
    /// predictable whether the capital that follows is right or invented.
    fn is_claim_token(&self, token: &str) -> bool {
        let trimmed = token.trim();
        if !trimmed.chars().any(char::is_alphanumeric) {
            return false;
        }
        !self.question.contains(&trimmed.to_lowercase())
    }

    /// Absorb the `logprobs.content[]` entries carried by one completion body
    /// or one stream chunk. Safe to call on any JSON: a body without logprobs
    /// contributes nothing.
    pub fn absorb(&mut self, json: &serde_json::Value) {
        let Some(tokens) = json["choices"][0]["logprobs"]["content"].as_array() else {
            return;
        };
        for token in tokens {
            let Some(logprob) = token["logprob"].as_f64() else {
                continue;
            };
            if !logprob.is_finite() {
                continue;
            }
            // A positive logprob is impossible (probabilities are <= 1) and
            // means the field is not what we think it is; drop it rather than
            // let it inflate the mean above certainty.
            if logprob > 0.0 {
                continue;
            }
            let clamped = logprob.max(MIN_TOKEN_LOGPROB);
            self.sum += clamped;
            self.count = self.count.saturating_add(1);
            if token["token"]
                .as_str()
                .is_some_and(|text| self.is_claim_token(text))
            {
                self.claim_sum += clamped;
                self.claim_count = self.claim_count.saturating_add(1);
            }
        }
    }

    /// The aggregate, or `None` when no usable token probabilities arrived.
    #[must_use]
    pub fn summarize(&self) -> Option<TokenConfidence> {
        if self.count == 0 {
            return None;
        }
        Some(TokenConfidence {
            token_count: self.count,
            mean_logprob: self.sum / f64::from(self.count),
            claim_token_count: self.claim_count,
            claim_mean_logprob: if self.claim_count == 0 {
                0.0
            } else {
                self.claim_sum / f64::from(self.claim_count)
            },
        })
    }
}

/// One-shot summary for a non-streamed completion body, scored against the
/// question so claim tokens can be separated from framing.
#[must_use]
pub fn from_completion(json: &serde_json::Value, question: &str) -> Option<TokenConfidence> {
    let mut accumulator = LogprobAccumulator::for_question(question);
    accumulator.absorb(json);
    accumulator.summarize()
}

/// The question an answer responds to: the last user message in the request.
///
/// Only the user's own turn, never the assembled context — a token found in a
/// retrieved document is the answer USING its evidence, which is the opposite
/// of a claim that needs checking.
#[must_use]
pub fn question_from_messages(messages: &[super::ChatMessage]) -> &str {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map_or("", |message| message.content.as_str())
}

/// Transport projection, shared by the unary and streaming gRPC surfaces so
/// the aggregate is mapped in exactly one place.
impl From<TokenConfidence> for mp_contracts::model_plane::v1::TokenConfidence {
    fn from(value: TokenConfidence) -> Self {
        Self {
            token_count: value.token_count,
            mean_logprob: value.mean_logprob,
            claim_token_count: value.claim_token_count,
            claim_mean_logprob: value.claim_mean_logprob,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completion(logprobs: &[f64]) -> serde_json::Value {
        let content: Vec<serde_json::Value> = logprobs
            .iter()
            .map(|lp| serde_json::json!({ "token": "x", "logprob": lp }))
            .collect();
        serde_json::json!({
            "choices": [{
                "message": { "content": "x" },
                "logprobs": { "content": content }
            }]
        })
    }

    #[test]
    fn a_near_certain_answer_summarizes_near_zero() {
        // "Oslo." — every token the obvious one.
        let summary = from_completion(&completion(&[-0.0001, -0.02]), "").unwrap();
        assert_eq!(summary.token_count, 2);
        assert!(
            summary.mean_logprob.exp() > 0.98,
            "geometric-mean probability = {}",
            summary.mean_logprob.exp()
        );
    }

    #[test]
    fn a_token_by_token_guess_summarizes_low() {
        let summary = from_completion(&completion(&[-2.3, -1.6, -3.0, -2.0]), "").unwrap();
        assert!(
            summary.mean_logprob.exp() < 0.2,
            "geometric-mean probability = {}",
            summary.mean_logprob.exp()
        );
    }

    #[test]
    fn absent_or_malformed_logprobs_are_unknown_not_low() {
        assert!(from_completion(&serde_json::json!({}), "").is_none());
        assert!(from_completion(&serde_json::json!({ "choices": [{}] }), "").is_none());
        assert!(
            from_completion(
                &serde_json::json!({ "choices": [{ "logprobs": { "content": [] } }] }),
                ""
            )
            .is_none()
        );
        // Present but unusable: no numeric `logprob` on any entry.
        assert!(
            from_completion(
                &serde_json::json!({
                    "choices": [{ "logprobs": { "content": [{ "token": "x" }] } }]
                }),
                ""
            )
            .is_none()
        );
    }

    /// A sentinel like `-9999` must not be able to drag an otherwise certain
    /// answer to zero — one impossible token is not a zero-confidence answer.
    #[test]
    fn a_sentinel_token_is_clamped() {
        let summary = from_completion(&completion(&[-0.01, -9999.0, -0.01]), "").unwrap();
        assert_eq!(summary.token_count, 3);
        assert!(
            (summary.mean_logprob - (MIN_TOKEN_LOGPROB + -0.02) / 3.0).abs() < 1e-9,
            "mean_logprob = {}",
            summary.mean_logprob
        );
    }

    #[test]
    fn positive_and_non_finite_values_are_dropped() {
        let summary = from_completion(&completion(&[-0.5, 1.5, f64::NAN, -0.5]), "").unwrap();
        assert_eq!(summary.token_count, 2, "only the two valid tokens count");
        assert!((summary.mean_logprob - -0.5).abs() < 1e-9);
    }

    /// Streaming delivers a few tokens per chunk; the summary is the whole
    /// answer, not the last chunk.
    #[test]
    fn accumulates_across_stream_chunks() {
        let mut accumulator = LogprobAccumulator::new();
        assert!(accumulator.summarize().is_none(), "nothing seen yet");
        accumulator.absorb(&completion(&[-0.1, -0.3]));
        accumulator.absorb(&completion(&[-0.2]));
        // A usage-only trailer carries no choices at all.
        accumulator.absorb(&serde_json::json!({ "usage": { "completion_tokens": 3 } }));
        let summary = accumulator.summarize().unwrap();
        assert_eq!(summary.token_count, 3);
        assert!((summary.mean_logprob - -0.2).abs() < 1e-9);
    }

    /// Build a completion from (token text, logprob) pairs so claim
    /// classification can be exercised the way a real answer exercises it.
    fn answer(tokens: &[(&str, f64)]) -> serde_json::Value {
        let content: Vec<serde_json::Value> = tokens
            .iter()
            .map(|(text, lp)| serde_json::json!({ "token": text, "logprob": lp }))
            .collect();
        serde_json::json!({ "choices": [{ "logprobs": { "content": content } }] })
    }

    /// The measurement this statistic exists for. Framing that restates the
    /// question is near-certain whatever follows it, so averaging it in lets a
    /// confident sentence hide an invented claim: here the whole answer reads
    /// 0.63 — ordinary-prose territory — while the one token that asserts
    /// anything reads 0.11.
    #[test]
    fn framing_is_excluded_so_an_invented_claim_shows_through() {
        let summary = from_completion(
            &answer(&[
                ("Hoved", -0.001),
                ("staden", -0.001),
                (" i", -0.002),
                (" Zub", -0.01),
                ("rowka", -0.01),
                (" er", -0.003),
                (" Krok", -2.2),
                ("owa", -2.2),
                (".", -0.001),
            ]),
            "Hva er hovedstaden i Zubrowka? Svar kort.",
        )
        .unwrap();
        assert_eq!(summary.token_count, 9);
        assert_eq!(summary.claim_token_count, 2, "only the invented capital");
        assert!(
            summary.mean_logprob.exp() > 0.55,
            "whole-answer mean hides it: {}",
            summary.mean_logprob.exp()
        );
        assert!(
            summary.claim_mean_logprob.exp() < 0.15,
            "claim mean must expose it: {}",
            summary.claim_mean_logprob.exp()
        );
    }

    /// The same shape with a fact the model knows: the claim token is as
    /// certain as the framing, so the statistic stays high.
    #[test]
    fn a_known_fact_scores_high_on_its_claim_token_too() {
        let summary = from_completion(
            &answer(&[
                ("Hoved", -0.001),
                ("staden", -0.001),
                (" i", -0.002),
                (" Norge", -0.001),
                (" er", -0.003),
                (" Oslo", -0.0004),
                (".", -0.001),
            ]),
            "Hva er hovedstaden i Norge? Svar kort.",
        )
        .unwrap();
        assert_eq!(summary.claim_token_count, 1);
        assert!(summary.claim_mean_logprob.exp() > 0.99);
    }

    #[test]
    fn punctuation_and_whitespace_never_count_as_claims() {
        let summary = from_completion(
            &answer(&[("4", -0.5), (" ", -0.001), ("😊", -0.001), (".", -0.001)]),
            "Hva er 2 pluss 2?",
        )
        .unwrap();
        assert_eq!(summary.token_count, 4);
        assert_eq!(summary.claim_token_count, 1, "only the digit asserts");
    }

    /// An answer that is entirely framing leaves nothing to score; consumers
    /// fall back to the whole-answer mean rather than reading 0 as certainty.
    #[test]
    fn an_answer_that_only_echoes_the_question_has_no_claim_tokens() {
        let summary = from_completion(
            &answer(&[("Oslo", -0.2), (".", -0.001)]),
            "Er hovedstaden Oslo?",
        )
        .unwrap();
        assert_eq!(summary.claim_token_count, 0);
        assert!(summary.token_count > 0);
    }

    /// Without a question every content token is a claim — the no-context
    /// fallback must not silently classify everything as framing.
    #[test]
    fn with_no_question_every_content_token_is_a_claim() {
        let mut accumulator = LogprobAccumulator::new();
        accumulator.absorb(&answer(&[("Oslo", -0.01), (" er", -0.02), (".", -0.001)]));
        let summary = accumulator.summarize().unwrap();
        assert_eq!(summary.token_count, 3);
        assert_eq!(summary.claim_token_count, 2);
    }

    #[test]
    fn the_question_is_the_last_user_message() {
        let messages = vec![
            super::super::ChatMessage {
                role: "system".to_owned(),
                content: "kontekst".to_owned(),
                name: String::new(),
            },
            super::super::ChatMessage {
                role: "user".to_owned(),
                content: "første".to_owned(),
                name: String::new(),
            },
            super::super::ChatMessage {
                role: "assistant".to_owned(),
                content: "svar".to_owned(),
                name: String::new(),
            },
            super::super::ChatMessage {
                role: "user".to_owned(),
                content: "siste spørsmål".to_owned(),
                name: String::new(),
            },
        ];
        assert_eq!(question_from_messages(&messages), "siste spørsmål");
        assert_eq!(question_from_messages(&[]), "");
    }

    #[test]
    fn reasoning_models_do_not_accept_the_parameter() {
        for model in ["gpt-5", "gpt-5-mini", "o1-preview", "o3-mini", "o4-mini"] {
            assert!(!model_supports_logprobs(model), "{model} must be excluded");
        }
        for model in ["gpt-4o", "gpt-4o-mini", "gpt-4.1", "GPT-4O"] {
            assert!(model_supports_logprobs(model), "{model} must be included");
        }
    }
}
