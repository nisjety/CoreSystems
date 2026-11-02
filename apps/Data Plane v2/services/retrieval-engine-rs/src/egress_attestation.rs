//! Egress attestation for ZDR-restricted content (Z-41).
//!
//! Restricted (ZDR) content may leave the engine for a retaining provider only when an operator
//! has attested that the exact Azure resource behind that route is covered by a no-retention
//! approval. This module reads that attestation as data, once at boot, and answers the two
//! questions the egress guards ask: may the query embedding go to this route, and may the rerank
//! route call its provider. Attestation is data, not code: nothing in the engine flips a route to
//! attested.
//!
//! The evidence has the shape the Model Plane uses for its own Azure routes (inference-core
//! `provider/zdr.rs`): five named fields plus a digest that binds them to each other, in the same
//! canonical form the runbook's `printf ... | sha256sum` produces. One attestation covers one Azure
//! resource, and two are read:
//!
//! * `AZURE_COHERE_ZDR_*`, the Cohere Embed v4 Foundry resource. It covers the query embedding
//!   (`COHERE_EMBED_V4_ENDPOINT`) and the rerank route (`RERANK_ENDPOINT`), each only when that
//!   endpoint is a host of the attested resource.
//! * `AZURE_OPENAI_ZDR_*`, the Azure OpenAI resource behind the direct embedding path
//!   (`AZURE_OPENAI_ENDPOINT`).
//!
//! Presenting one resource's evidence for another is the conflation this module exists to stop.
//!
//! The default is no attestation, and every route stays fail closed. The embedding guards refuse a
//! ZDR call before any network call, and a ZDR retrieval degrades to lexical results with an
//! explicit notice (see `pipeline::orchestrator`). A field set that does not validate, or an
//! attestation that does not describe a configured endpoint, is logged at boot as a problem. It
//! never aborts the engine: tenant retrieval does not depend on it, and the degrade stays visible.
//!
//! The digest proves only that the five fields agree with each other. The endpoint binding is the
//! host rule in [`ZdrAttestation::describes`]: the endpoint's host must be the attested resource
//! name followed by one of that surface's Azure domains (see `FOUNDRY_FAMILIES`). A same-named
//! resource under another Azure domain is a different resource, so it does not match. That is the
//! most this code can check from a string. The approval
//! itself is an operator fact. The attestation is read once, at boot. A review-by date that passes
//! while the engine runs therefore takes effect at the next restart, as the Model Plane's
//! attestation does, so a renewal also needs a restart.

use chrono::NaiveDate;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

use crate::config::Config;
use crate::rerank_api;

/// Environment prefix of the attestation over the Cohere Embed v4 Foundry resource.
const COHERE_PREFIX: &str = "AZURE_COHERE_ZDR";
/// Environment prefix of the attestation over the Azure OpenAI resource.
const AZURE_OPENAI_PREFIX: &str = "AZURE_OPENAI_ZDR";
/// The Azure domains a Foundry (Cohere) resource is reached through.
const FOUNDRY_FAMILIES: &[&str] = &["services.ai.azure.com", "cognitiveservices.azure.com"];
/// The Azure domains an Azure OpenAI resource is reached through.
const OPENAI_FAMILIES: &[&str] = &["openai.azure.com", "cognitiveservices.azure.com"];

/// The Azure surfaces an embedding route can run on. Each one has its own attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedSurface {
    /// Cohere Embed v4 on Azure AI Foundry: the text query embedder.
    CohereFoundry,
    /// The direct Azure OpenAI embedding path.
    AzureOpenAi,
}

/// The attestation fields of one resource, as configured. Borrowed from the configuration.
#[derive(Debug, Clone, Copy)]
struct RawAttestation<'a> {
    resource_id: &'a str,
    approval_ref: &'a str,
    effective_date: &'a str,
    review_by: &'a str,
    reviewer: &'a str,
    digest: &'a str,
}

/// Why a configured attestation was refused. Logged at boot; the routes it names stay fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AttestationError {
    #[error("{prefix} is incomplete (missing {missing}): set every {prefix}_* field, or none")]
    Incomplete {
        prefix: &'static str,
        missing: String,
    },
    #[error("{prefix}_RESOURCE_ID names no resource (it ends in a separator)")]
    NoResourceName { prefix: &'static str },
    #[error("{field} contains a line break, which makes the digest ambiguous")]
    FieldContainsNewline { field: String },
    #[error("{name} ({value:?}) is not a YYYY-MM-DD date")]
    InvalidDate { name: String, value: String },
    #[error("{prefix}: review-by {review_by} is not after the effective date {effective}")]
    ReviewBeforeEffective {
        prefix: &'static str,
        effective: NaiveDate,
        review_by: NaiveDate,
    },
    #[error(
        "{prefix}_DIGEST does not match the five fields; recompute it with printf ... | sha256sum \
         (expected {expected})"
    )]
    DigestMismatch {
        prefix: &'static str,
        expected: String,
    },
    #[error("{prefix} is not in force until {effective} (today {today})")]
    NotYetEffective {
        prefix: &'static str,
        effective: NaiveDate,
        today: NaiveDate,
    },
    #[error(
        "{prefix} lapsed on {review_by} (today {today}): re-verify the approval and update the \
         attestation"
    )]
    Lapsed {
        prefix: &'static str,
        review_by: NaiveDate,
        today: NaiveDate,
    },
}

/// A validated attestation over one resource. Holding a value is the proof that it validated.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ZdrAttestation {
    /// The attested resource's name: the last segment of its resource id, lower case.
    resource_name: String,
    approval_ref: String,
    review_by: NaiveDate,
    /// The Azure domains, after `<name>.`, that this surface's resources are reached through.
    families: &'static [&'static str],
}

impl ZdrAttestation {
    /// Validates one resource's fields. `Ok(None)` when every field is blank, the ordinary "this
    /// deployment makes no ZDR claim" case. A partly filled attestation is an error, because it
    /// means someone meant a claim and mistyped it.
    fn validate(
        prefix: &'static str,
        raw: &RawAttestation<'_>,
        today: NaiveDate,
    ) -> Result<Option<Self>, AttestationError> {
        let fields = [
            ("RESOURCE_ID", raw.resource_id),
            ("APPROVAL_REF", raw.approval_ref),
            ("EFFECTIVE_DATE", raw.effective_date),
            ("REVIEW_BY", raw.review_by),
            ("REVIEWER", raw.reviewer),
            ("DIGEST", raw.digest),
        ];
        let missing: Vec<String> = fields
            .iter()
            .filter(|(_, value)| value.trim().is_empty())
            .map(|(name, _)| format!("{prefix}_{name}"))
            .collect();
        if missing.len() == fields.len() {
            return Ok(None);
        }
        if !missing.is_empty() {
            return Err(AttestationError::Incomplete {
                prefix,
                missing: missing.join(", "),
            });
        }

        // An interior line break would let two different tuples hash the same, because the
        // canonical form is line-delimited. Edge whitespace is an ordinary `.env` artifact.
        for (name, value) in &fields[..5] {
            if value.trim().contains(['\n', '\r']) {
                return Err(AttestationError::FieldContainsNewline {
                    field: format!("{prefix}_{name}"),
                });
            }
        }

        let resource_name = raw
            .resource_id
            .trim()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if resource_name.is_empty() {
            return Err(AttestationError::NoResourceName { prefix });
        }

        let effective = parse_date(&format!("{prefix}_EFFECTIVE_DATE"), raw.effective_date)?;
        let review_by = parse_date(&format!("{prefix}_REVIEW_BY"), raw.review_by)?;
        if review_by <= effective {
            return Err(AttestationError::ReviewBeforeEffective {
                prefix,
                effective,
                review_by,
            });
        }

        let expected = canonical_digest(raw);
        if !raw.digest.trim().eq_ignore_ascii_case(&expected) {
            return Err(AttestationError::DigestMismatch { prefix, expected });
        }

        if effective > today {
            return Err(AttestationError::NotYetEffective {
                prefix,
                effective,
                today,
            });
        }
        if review_by < today {
            return Err(AttestationError::Lapsed {
                prefix,
                review_by,
                today,
            });
        }

        let families = match prefix {
            COHERE_PREFIX => FOUNDRY_FAMILIES,
            _ => OPENAI_FAMILIES,
        };
        Ok(Some(Self {
            resource_name,
            approval_ref: raw.approval_ref.trim().to_owned(),
            review_by,
            families,
        }))
    }

    /// Whether `endpoint` is a host of the attested resource: `<resource_name>.<family>`, where the
    /// family is one of this surface's Azure domains. The path, port and any user part are ignored.
    /// A bare `azure.com`, a host that merely starts with the resource name, a same-named resource
    /// under another Azure domain, or a non-Azure host never matches.
    fn describes(&self, endpoint: &str) -> bool {
        let Some(host) = endpoint_host(endpoint) else {
            return false;
        };
        let Some((label, family)) = host.split_once('.') else {
            return false;
        };
        label == self.resource_name.as_str() && self.families.contains(&family)
    }
}

/// The resolved egress attestation of this engine: which routes may carry restricted content, and
/// what the boot found wrong. `Default` is the safe state: nothing is attested.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EgressAttestation {
    embed_cohere: bool,
    embed_azure_openai: bool,
    rerank_cohere: bool,
    problems: Vec<String>,
}

impl EgressAttestation {
    /// Resolves the attestation from configuration as of `today`. Never fails: a refused
    /// attestation is recorded in [`Self::problems`] and leaves its routes unattested.
    #[must_use]
    pub fn from_config(cfg: &Config, today: NaiveDate) -> Self {
        let mut egress = Self::default();

        let cohere = resolve(COHERE_PREFIX, &cohere_raw(cfg), today, &mut egress.problems);
        if let Some(attestation) = &cohere {
            egress.embed_cohere = bind(
                attestation,
                "COHERE_EMBED_V4_ENDPOINT",
                &cfg.cohere_embed_v4_endpoint,
                &mut egress.problems,
            );
            egress.rerank_cohere = bind(
                attestation,
                "RERANK_ENDPOINT",
                &cfg.rerank_endpoint,
                &mut egress.problems,
            );
        }

        let azure_openai = resolve(
            AZURE_OPENAI_PREFIX,
            &azure_openai_raw(cfg),
            today,
            &mut egress.problems,
        );
        if let Some(attestation) = &azure_openai {
            egress.embed_azure_openai = bind(
                attestation,
                "AZURE_OPENAI_ENDPOINT",
                &cfg.azure_openai_endpoint,
                &mut egress.problems,
            );
        }

        egress
    }

    /// Whether the embedding route on `surface` may carry ZDR content.
    #[must_use]
    pub const fn permits_embed(&self, surface: EmbedSurface) -> bool {
        match surface {
            EmbedSurface::CohereFoundry => self.embed_cohere,
            EmbedSurface::AzureOpenAi => self.embed_azure_openai,
        }
    }

    /// Whether the rerank route (`RERANK_ENDPOINT`) may carry ZDR content.
    #[must_use]
    pub const fn permits_rerank(&self) -> bool {
        self.rerank_cohere
    }

    /// What the boot found wrong with the configured attestation, one line per problem.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }
}

/// The QB-T05a rerank route reads its answer through this trait, which defaults to refusing.
impl rerank_api::EgressAttestation for EgressAttestation {
    fn permits_rerank(&self) -> bool {
        EgressAttestation::permits_rerank(self)
    }
}

fn cohere_raw(cfg: &Config) -> RawAttestation<'_> {
    RawAttestation {
        resource_id: &cfg.azure_cohere_zdr_resource_id,
        approval_ref: &cfg.azure_cohere_zdr_approval_ref,
        effective_date: &cfg.azure_cohere_zdr_effective_date,
        review_by: &cfg.azure_cohere_zdr_review_by,
        reviewer: &cfg.azure_cohere_zdr_reviewer,
        digest: &cfg.azure_cohere_zdr_digest,
    }
}

fn azure_openai_raw(cfg: &Config) -> RawAttestation<'_> {
    RawAttestation {
        resource_id: &cfg.azure_openai_zdr_resource_id,
        approval_ref: &cfg.azure_openai_zdr_approval_ref,
        effective_date: &cfg.azure_openai_zdr_effective_date,
        review_by: &cfg.azure_openai_zdr_review_by,
        reviewer: &cfg.azure_openai_zdr_reviewer,
        digest: &cfg.azure_openai_zdr_digest,
    }
}

/// Validates one resource's attestation, recording a refusal as a boot problem.
fn resolve(
    prefix: &'static str,
    raw: &RawAttestation<'_>,
    today: NaiveDate,
    problems: &mut Vec<String>,
) -> Option<ZdrAttestation> {
    match ZdrAttestation::validate(prefix, raw, today) {
        Ok(attestation) => attestation,
        Err(error) => {
            problems.push(error.to_string());
            None
        }
    }
}

/// Binds a valid attestation to one route's endpoint. An unset endpoint is not a route. A
/// configured endpoint the attestation does not describe is a boot problem, and stays unattested.
fn bind(
    attestation: &ZdrAttestation,
    endpoint_var: &'static str,
    endpoint: &str,
    problems: &mut Vec<String>,
) -> bool {
    if endpoint.trim().is_empty() {
        return false;
    }
    if attestation.describes(endpoint) {
        tracing::info!(
            route = endpoint_var,
            approval_ref = %attestation.approval_ref,
            review_by = %attestation.review_by,
            "ZDR egress attestation covers this route"
        );
        return true;
    }
    problems.push(format!(
        "{endpoint_var} is not a host of the attested resource {} (that name under one of its \
         surface's Azure domains): the route stays unattested",
        attestation.resource_name
    ));
    false
}

fn parse_date(name: &str, value: &str) -> Result<NaiveDate, AttestationError> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| AttestationError::InvalidDate {
        name: name.to_owned(),
        value: value.to_owned(),
    })
}

/// The canonical digest: the five fields, trimmed, each followed by a line break. These are the
/// exact bytes the runbook's `printf '%s\n%s\n%s\n%s\n%s\n' ... | sha256sum` hashes.
fn canonical_digest(raw: &RawAttestation<'_>) -> String {
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        raw.resource_id.trim(),
        raw.approval_ref.trim(),
        raw.effective_date.trim(),
        raw.review_by.trim(),
        raw.reviewer.trim()
    );
    Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The lower-case host of an https endpoint URL, without user part, port or path. `None` for an
/// endpoint with a backslash, a control character or a space, which the WHATWG parser the HTTP
/// client uses reads differently. For http(s) a backslash ends the authority, so an endpoint like
/// `https://attacker.example\@<attested host>` is dialled at `attacker.example`. It is refused here
/// rather than read as the attested host.
fn endpoint_host(endpoint: &str) -> Option<String> {
    let trimmed = endpoint.trim();
    if trimmed
        .chars()
        .any(|c| c == '\\' || c.is_control() || c.is_whitespace())
    {
        return None;
    }
    // Only https carries an attested route's content. A plaintext endpoint would send the query and
    // the key in the clear, so it is never the attested host, whatever its name.
    let (scheme, without_scheme) = trimmed.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = without_scheme.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rerank_api::{EgressAttestation as RerankAttestation, NoAttestation};

    const RESOURCE: &str = "/subscriptions/0000/resourceGroups/rg-data/providers/Microsoft.CognitiveServices/accounts/aquatiq-cohere";
    /// `printf '%s\n%s\n%s\n%s\n%s\n' <the five fields below> | sha256sum`, computed outside this
    /// code so the canonical form is pinned to the runbook's pipeline, not to this implementation.
    const DIGEST: &str = "b287e12686ffe907defa848e8887ed6389b8da3910041e5e8e6bd33ef368f76c";
    const FOUNDRY: &str = "https://aquatiq-cohere.services.ai.azure.com";
    const AZURE_OPENAI: &str = "https://aquatiq-openai.openai.azure.com";

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 11).expect("static date")
    }

    fn raw() -> RawAttestation<'static> {
        RawAttestation {
            resource_id: RESOURCE,
            approval_ref: "MAM-2026-0042",
            effective_date: "2026-06-01",
            review_by: "2027-06-01",
            reviewer: "ima@aquatiq.com",
            digest: DIGEST,
        }
    }

    /// The same five fields with a digest computed over them, so a test can change one field
    /// and still reach the check it is about.
    fn signed<'a>(digest: &'a mut String, raw: RawAttestation<'a>) -> RawAttestation<'a> {
        *digest = canonical_digest(&raw);
        RawAttestation {
            digest: digest.as_str(),
            ..raw
        }
    }

    fn config(pairs: &[(&str, &str)]) -> Config {
        let mut env = vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://test.invalid/test".to_string(),
            ),
            (
                "QDRANT_URL".to_string(),
                "http://test.invalid:6334".to_string(),
            ),
        ];
        env.extend(
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string())),
        );
        envy::from_iter(env).expect("the configuration deserializes")
    }

    fn cohere_attestation(endpoint: &str) -> Vec<(&str, &str)> {
        vec![
            ("AZURE_COHERE_ZDR_RESOURCE_ID", RESOURCE),
            ("AZURE_COHERE_ZDR_APPROVAL_REF", "MAM-2026-0042"),
            ("AZURE_COHERE_ZDR_EFFECTIVE_DATE", "2026-06-01"),
            ("AZURE_COHERE_ZDR_REVIEW_BY", "2027-06-01"),
            ("AZURE_COHERE_ZDR_REVIEWER", "ima@aquatiq.com"),
            ("AZURE_COHERE_ZDR_DIGEST", DIGEST),
            ("COHERE_EMBED_V4_ENDPOINT", endpoint),
        ]
    }

    #[test]
    fn the_digest_is_the_runbook_pipeline_over_the_five_fields() {
        assert_eq!(canonical_digest(&raw()), DIGEST);
    }

    #[test]
    fn a_complete_attestation_with_its_digest_validates() {
        let attestation = ZdrAttestation::validate(COHERE_PREFIX, &raw(), today())
            .expect("valid attestation")
            .expect("present attestation");
        assert_eq!(attestation.resource_name, "aquatiq-cohere");
        assert_eq!(attestation.approval_ref, "MAM-2026-0042");
    }

    #[test]
    fn no_field_at_all_is_no_attestation_rather_than_an_error() {
        let blank = RawAttestation {
            resource_id: "",
            approval_ref: "",
            effective_date: "",
            review_by: "",
            reviewer: "",
            digest: "",
        };
        assert_eq!(
            ZdrAttestation::validate(COHERE_PREFIX, &blank, today()),
            Ok(None)
        );
    }

    #[test]
    fn a_partial_attestation_is_refused_and_names_what_is_missing() {
        let partial = RawAttestation {
            digest: "",
            reviewer: "",
            ..raw()
        };
        let error = ZdrAttestation::validate(COHERE_PREFIX, &partial, today())
            .expect_err("a partial attestation must be refused, not ignored");
        assert_eq!(
            error,
            AttestationError::Incomplete {
                prefix: COHERE_PREFIX,
                missing: "AZURE_COHERE_ZDR_REVIEWER, AZURE_COHERE_ZDR_DIGEST".to_owned(),
            }
        );
    }

    #[test]
    fn a_placeholder_digest_does_not_grant_zdr() {
        let placeholder = RawAttestation {
            digest: "CHANGEME",
            ..raw()
        };
        assert!(matches!(
            ZdrAttestation::validate(COHERE_PREFIX, &placeholder, today()),
            Err(AttestationError::DigestMismatch { .. })
        ));
    }

    #[test]
    fn a_line_break_inside_a_field_is_refused_before_hashing() {
        let broken = RawAttestation {
            approval_ref: "MAM-2026\n0042",
            ..raw()
        };
        assert_eq!(
            ZdrAttestation::validate(COHERE_PREFIX, &broken, today()),
            Err(AttestationError::FieldContainsNewline {
                field: "AZURE_COHERE_ZDR_APPROVAL_REF".to_owned(),
            })
        );
    }

    #[test]
    fn the_dates_are_read_strictly_and_review_must_follow_effect() {
        let malformed = RawAttestation {
            effective_date: "01.06.2026",
            ..raw()
        };
        assert!(matches!(
            ZdrAttestation::validate(COHERE_PREFIX, &malformed, today()),
            Err(AttestationError::InvalidDate { .. })
        ));

        let inverted = RawAttestation {
            review_by: "2026-05-01",
            ..raw()
        };
        assert!(matches!(
            ZdrAttestation::validate(COHERE_PREFIX, &inverted, today()),
            Err(AttestationError::ReviewBeforeEffective { .. })
        ));
    }

    #[test]
    fn an_approval_not_yet_in_force_grants_nothing() {
        let mut digest = String::new();
        let future = signed(
            &mut digest,
            RawAttestation {
                effective_date: "2026-12-01",
                review_by: "2027-12-01",
                ..raw()
            },
        );
        assert!(matches!(
            ZdrAttestation::validate(COHERE_PREFIX, &future, today()),
            Err(AttestationError::NotYetEffective { .. })
        ));
    }

    #[test]
    fn a_lapsed_approval_grants_nothing() {
        let mut digest = String::new();
        let lapsed = signed(
            &mut digest,
            RawAttestation {
                review_by: "2026-09-01",
                ..raw()
            },
        );
        assert!(matches!(
            ZdrAttestation::validate(COHERE_PREFIX, &lapsed, today()),
            Err(AttestationError::Lapsed { .. })
        ));
    }

    #[test]
    fn a_resource_id_that_names_nothing_is_refused() {
        let nameless = RawAttestation {
            resource_id: "/subscriptions/0000/accounts/",
            ..raw()
        };
        assert_eq!(
            ZdrAttestation::validate(COHERE_PREFIX, &nameless, today()),
            Err(AttestationError::NoResourceName {
                prefix: COHERE_PREFIX,
            })
        );
    }

    #[test]
    fn the_endpoint_must_be_a_host_of_the_attested_resource() {
        let attestation = ZdrAttestation::validate(COHERE_PREFIX, &raw(), today())
            .expect("valid")
            .expect("present");
        for endpoint in [
            FOUNDRY,
            "https://aquatiq-cohere.services.ai.azure.com/openai/deployments/embed/embeddings",
            "https://aquatiq-cohere.services.ai.azure.com:443",
            "https://user@aquatiq-cohere.services.ai.azure.com",
            "HTTPS://AQUATIQ-COHERE.SERVICES.AI.AZURE.COM",
            "https://aquatiq-cohere.cognitiveservices.azure.com",
        ] {
            assert!(attestation.describes(endpoint), "must describe {endpoint}");
        }
        for endpoint in [
            "https://other.services.ai.azure.com",
            "https://aquatiq-cohere-evil.services.ai.azure.com",
            "https://aquatiq-cohere.services.ai.azure.com.evil.example",
            "https://aquatiq-cohere.example.com",
            "https://api.cohere.ai/v1/rerank",
            "https://azure.com",
            "http://aquatiq-cohere.services.ai.azure.com",
            "aquatiq-cohere.services.ai.azure.com",
            "https://aquatiq-cohere.openai.azure.com",
            "https://aquatiq-cohere.services.ai.azure.com.",
            "https://attacker.example\\@aquatiq-cohere.services.ai.azure.com",
            "https://aquatiq-cohere.services.ai.azure.com\\@attacker.example",
            "https://aquatiq-cohere.services.ai.azure.com\taquatiq.evil.example",
            "",
            "   ",
        ] {
            assert!(
                !attestation.describes(endpoint),
                "must not describe {endpoint:?}"
            );
        }
    }

    #[test]
    fn an_attestation_describes_only_the_domains_of_its_own_surface() {
        let openai = RawAttestation {
            resource_id: "/subscriptions/0000/accounts/aquatiq-openai",
            approval_ref: "MAM-2026-0043",
            effective_date: "2026-06-01",
            review_by: "2027-06-01",
            reviewer: "ima@aquatiq.com",
            digest: "",
        };
        let mut digest = String::new();
        let signed_openai = signed(&mut digest, openai);
        let attestation = ZdrAttestation::validate(AZURE_OPENAI_PREFIX, &signed_openai, today())
            .expect("valid")
            .expect("present");
        assert!(attestation.describes("https://aquatiq-openai.openai.azure.com"));
        assert!(attestation.describes("https://aquatiq-openai.cognitiveservices.azure.com"));
        assert!(
            !attestation.describes("https://aquatiq-openai.services.ai.azure.com"),
            "a Foundry domain is not an Azure OpenAI surface"
        );
        assert!(!attestation.describes("https://aquatiq-openai.azure.com"));
    }

    #[test]
    fn no_configuration_permits_nothing() {
        let egress = EgressAttestation::from_config(&config(&[]), today());
        assert!(!egress.permits_embed(EmbedSurface::CohereFoundry));
        assert!(!egress.permits_embed(EmbedSurface::AzureOpenAi));
        assert!(!egress.permits_rerank());
        assert!(egress.problems().is_empty());
        assert_eq!(egress, EgressAttestation::default());
    }

    #[test]
    fn a_valid_cohere_attestation_permits_the_embedding_route_it_describes() {
        let cfg = config(&cohere_attestation(FOUNDRY));
        let egress = EgressAttestation::from_config(&cfg, today());
        assert!(egress.permits_embed(EmbedSurface::CohereFoundry));
        assert!(!egress.permits_embed(EmbedSurface::AzureOpenAi));
        // RERANK_ENDPOINT still defaults to the public Cohere API, which no attestation covers.
        assert!(!egress.permits_rerank());
        assert!(
            egress
                .problems()
                .iter()
                .any(|problem| problem.starts_with("RERANK_ENDPOINT")),
            "a configured endpoint that is not attested is reported: {:?}",
            egress.problems()
        );
    }

    #[test]
    fn a_valid_cohere_attestation_permits_rerank_on_an_attested_host() {
        let mut pairs = cohere_attestation(FOUNDRY);
        pairs.push(("RERANK_ENDPOINT", FOUNDRY));
        let egress = EgressAttestation::from_config(&config(&pairs), today());
        assert!(egress.permits_rerank());
        assert!(RerankAttestation::permits_rerank(&egress));
        assert!(egress.problems().is_empty(), "{:?}", egress.problems());
    }

    #[test]
    fn the_rerank_trait_default_still_refuses() {
        assert!(!RerankAttestation::permits_rerank(&NoAttestation));
    }

    #[test]
    fn a_refused_attestation_permits_nothing_and_says_why() {
        let partial = [("AZURE_COHERE_ZDR_RESOURCE_ID", RESOURCE)];
        let mut pairs: Vec<(&str, &str)> = partial.to_vec();
        pairs.push(("COHERE_EMBED_V4_ENDPOINT", FOUNDRY));
        pairs.push(("RERANK_ENDPOINT", FOUNDRY));
        let egress = EgressAttestation::from_config(&config(&pairs), today());
        assert!(!egress.permits_embed(EmbedSurface::CohereFoundry));
        assert!(!egress.permits_rerank());
        assert_eq!(egress.problems().len(), 1, "{:?}", egress.problems());
        assert!(egress.problems()[0].contains("AZURE_COHERE_ZDR_APPROVAL_REF"));
    }

    #[test]
    fn an_attestation_for_another_resource_describes_no_endpoint() {
        let mut pairs = cohere_attestation(AZURE_OPENAI);
        pairs.push(("RERANK_ENDPOINT", AZURE_OPENAI));
        let egress = EgressAttestation::from_config(&config(&pairs), today());
        assert!(!egress.permits_embed(EmbedSurface::CohereFoundry));
        assert!(!egress.permits_rerank());
    }

    #[test]
    fn an_azure_openai_attestation_permits_only_the_direct_embedding_route() {
        const OPENAI_RESOURCE: &str = "/subscriptions/0000/accounts/aquatiq-openai";
        let unsigned = RawAttestation {
            resource_id: OPENAI_RESOURCE,
            approval_ref: "MAM-2026-0043",
            effective_date: "2026-06-01",
            review_by: "2027-06-01",
            reviewer: "ima@aquatiq.com",
            digest: "",
        };
        let digest = canonical_digest(&unsigned);
        let pairs = [
            ("AZURE_OPENAI_ZDR_RESOURCE_ID", OPENAI_RESOURCE),
            ("AZURE_OPENAI_ZDR_APPROVAL_REF", "MAM-2026-0043"),
            ("AZURE_OPENAI_ZDR_EFFECTIVE_DATE", "2026-06-01"),
            ("AZURE_OPENAI_ZDR_REVIEW_BY", "2027-06-01"),
            ("AZURE_OPENAI_ZDR_REVIEWER", "ima@aquatiq.com"),
            ("AZURE_OPENAI_ZDR_DIGEST", digest.as_str()),
            ("AZURE_OPENAI_ENDPOINT", AZURE_OPENAI),
        ];
        let egress = EgressAttestation::from_config(&config(&pairs), today());
        assert!(
            egress.permits_embed(EmbedSurface::AzureOpenAi),
            "{:?}",
            egress.problems()
        );
        assert!(!egress.permits_embed(EmbedSurface::CohereFoundry));
        assert!(!egress.permits_rerank());
    }
}
