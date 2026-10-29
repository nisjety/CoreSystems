//! Evidence-bound Zero Data Retention attestation.
//!
//! Replaces the previous self-reported `AZURE_OPENAI_ZDR_CONFIRMED=true`
//! boolean. A boolean records only that *someone typed true*; it survives a
//! copy-pasted `.env`, a stale deployment, and an approval that was never
//! actually granted. Every one of those produces a service that advertises
//! `supports_zdr` — and therefore accepts ZDR-flagged traffic — on no evidence
//! at all.
//!
//! An attestation instead names the exact artifacts a reviewer can check:
//! which cloud resource the claim covers, which retention-exception approval
//! backs it, when it took effect, and who signed off. The digest is what makes
//! it non-forgeable-by-accident: it is recomputed here from the other five
//! fields, so a placeholder (`CHANGEME`, an empty string, a digest copied from
//! a different resource) fails validation instead of silently granting ZDR.
//!
//! # What the digest does not prove
//!
//! The digest binds the five fields *to each other*, not to the endpoint traffic
//! is actually sent to. Move the whole five-variable block to an environment whose
//! `AZURE_*_ENDPOINT` points at a resource with no retention exception and it
//! still validates — the fields remain mutually consistent, they just describe
//! somewhere else. [`ZdrAttestation::warn_on_endpoint_mismatch`] closes the common
//! case by comparing the attested resource id against the configured endpoint host
//! and logging when they disagree, but that is a heuristic on an opaque
//! operator-authored string, not a proof.
//!
//! It does bound validity at both ends. `_ZDR_EFFECTIVE_DATE` is the lower bound
//! and `_ZDR_REVIEW_BY` the upper one, so a lapsed retention approval stops
//! granting ZDR instead of coasting on the fact that nobody remembered it. That is
//! the whole point of putting the posture in config rather than in someone's head:
//! a mechanism that never expires is still a mechanism that depends on memory.
//!
//! # Generating the digest
//!
//! The canonical form is the five fields in declaration order, each followed by
//! a newline. An operator reproduces it with:
//!
//! ```sh
//! printf '%s\n%s\n%s\n%s\n%s\n' \
//!   "$RESOURCE_ID" "$APPROVAL_REF" "$EFFECTIVE_DATE" "$REVIEW_BY" "$REVIEWER" \
//!   | sha256sum
//! ```
//!
//! [`ZdrAttestation::validate`] is pure and holds that contract; `from_env` is a
//! thin reader over it, so every rule below is unit-tested without touching
//! process-global environment state.
//!
//! # Scope
//!
//! One attestation covers one provider surface. Azure `OpenAI` and Azure AI
//! Foundry Claude are separate Azure resources under separate retention
//! approvals, so they carry separate attestations (`AZURE_OPENAI_ZDR_*` and
//! `AZURE_ANTHROPIC_ZDR_*`) rather than sharing one. Presenting one resource's
//! evidence for another is exactly the conflation this module exists to stop.

use chrono::NaiveDate;
use sha2::{Digest as _, Sha256};

/// Environment-variable suffixes read for every attestation, in canonical order.
const FIELD_SUFFIXES: [&str; 5] = [
    "_ZDR_RESOURCE_ID",
    "_ZDR_APPROVAL_REF",
    "_ZDR_EFFECTIVE_DATE",
    "_ZDR_REVIEW_BY",
    "_ZDR_REVIEWER",
];

/// Suffix of the digest variable, kept out of [`FIELD_SUFFIXES`] because it is
/// derived from the others rather than being an input to the canonical form.
const DIGEST_SUFFIX: &str = "_ZDR_DIGEST";

/// Why an operator-supplied ZDR attestation was refused.
///
/// Every variant is a boot-time failure rather than a downgrade. A downgrade
/// would leave an operator who explicitly asserted ZDR running without it, and
/// the only signal would be `ZdrUnavailable` errors surfacing later under load
/// — long after the change that caused them.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ZdrAttestationError {
    /// Some but not all attestation fields were supplied.
    #[error(
        "ZDR attestation for {prefix} is incomplete: missing {missing:?}. Supply every \
         {prefix}_ZDR_* field (RESOURCE_ID, APPROVAL_REF, EFFECTIVE_DATE, REVIEW_BY, REVIEWER, \
         DIGEST), \
         or unset them all to run without ZDR."
    )]
    Incomplete {
        prefix: String,
        missing: Vec<String>,
    },

    /// `_ZDR_EFFECTIVE_DATE` was not an ISO-8601 calendar date.
    #[error(
        "ZDR attestation for {prefix}: {prefix}_ZDR_EFFECTIVE_DATE ({value:?}) is not a \
         YYYY-MM-DD date"
    )]
    InvalidEffectiveDate { prefix: String, value: String },

    /// The supplied digest did not match the digest recomputed from the fields.
    ///
    /// Carries the expected value so an operator can fix the variable without
    /// re-deriving the `printf` pipeline by hand. This leaks nothing: the digest
    /// is over operator-authored configuration identifiers, not a secret.
    #[error(
        "ZDR attestation for {prefix}: {prefix}_ZDR_DIGEST ({supplied:?}) does not match the \
         digest of the supplied fields ({expected:?}). Recompute with: printf \
         '%s\\n%s\\n%s\\n%s\\n%s\\n' \"$RESOURCE_ID\" \"$APPROVAL_REF\" \
         \"$EFFECTIVE_DATE\" \"$REVIEW_BY\" \"$REVIEWER\" | sha256sum"
    )]
    DigestMismatch {
        prefix: String,
        supplied: String,
        expected: String,
    },

    /// `_ZDR_REVIEW_BY` was not an ISO-8601 calendar date.
    #[error(
        "ZDR attestation for {prefix}: {prefix}_ZDR_REVIEW_BY ({value:?}) is not a YYYY-MM-DD date"
    )]
    InvalidReviewBy { prefix: String, value: String },

    /// `_ZDR_REVIEW_BY` is not after `_ZDR_EFFECTIVE_DATE`.
    #[error(
        "ZDR attestation for {prefix}: review-by {review_by} is not after the effective date \
         {effective}. An approval cannot lapse before it starts."
    )]
    ReviewBeforeEffective {
        prefix: String,
        effective: NaiveDate,
        review_by: NaiveDate,
    },

    /// The retention approval has passed its review-by date.
    #[error(
        "ZDR attestation for {prefix} lapsed on {review_by} (today {today}). Re-verify the \
         retention approval and update {prefix}_ZDR_REVIEW_BY with a recomputed \
         {prefix}_ZDR_DIGEST, or unset the attestation to run without ZDR. Refusing to keep \
         advertising ZDR on an unreviewed approval."
    )]
    Expired {
        prefix: String,
        review_by: NaiveDate,
        today: NaiveDate,
    },

    /// The attestation is well-formed but its effective date is in the future.
    #[error(
        "ZDR attestation for {prefix} is not yet in force (effective {effective}, today \
         {today}). Refusing to advertise ZDR before the approval takes effect."
    )]
    NotYetEffective {
        prefix: String,
        effective: NaiveDate,
        today: NaiveDate,
    },

    /// A field contained a newline, making the canonical form ambiguous.
    ///
    /// The canonical form is newline-delimited, so a field containing a newline
    /// lets two different field tuples hash identically: `resource_id="a\nb"` with
    /// `approval_ref="c"` produces the same bytes as `resource_id="a"` with
    /// `approval_ref="b\nc"`. That is not exploitable — anyone who can set these
    /// variables controls the whole attestation anyway — but a digest that binds
    /// two different inputs to one value is not a binding, so the ambiguity is
    /// rejected rather than documented. No legitimate Azure resource id, approval
    /// reference, date or reviewer contains a newline.
    #[error(
        "ZDR attestation for {prefix}: {field} contains a newline, which makes the canonical \
         digest form ambiguous. Remove the line break."
    )]
    FieldContainsNewline { prefix: String, field: String },

    /// The legacy boolean asserted ZDR but no attestation backs it.
    #[error(
        "{legacy_var}=true but no ZDR attestation was supplied. A boolean is not evidence. \
         Set {prefix}_ZDR_RESOURCE_ID, {prefix}_ZDR_APPROVAL_REF, {prefix}_ZDR_EFFECTIVE_DATE, \
         {prefix}_ZDR_REVIEWER and {prefix}_ZDR_DIGEST, or unset {legacy_var} to run without ZDR."
    )]
    LegacyBooleanWithoutEvidence {
        legacy_var: &'static str,
        prefix: String,
    },
}

/// Operator-supplied attestation fields as read, before validation.
///
/// Exists so [`ZdrAttestation::validate`] can be a pure function over owned
/// strings: the validation rules are the security-relevant part and are tested
/// directly, with no environment involved.
#[derive(Debug, Clone, Default)]
pub struct RawAttestation {
    pub resource_id: String,
    pub approval_ref: String,
    /// Kept as the operator's raw string: the digest is defined over exactly the
    /// bytes they hashed, not over a reformatted date.
    pub effective_date: String,
    /// The date the retention approval must be re-verified by. Past this, the
    /// attestation stops granting ZDR — see the module docs on bounding validity.
    pub review_by: String,
    pub reviewer: String,
    pub digest: String,
}

/// A validated operator attestation that one provider surface is covered by an
/// independently verified Zero Data Retention contract.
///
/// Construction validates, and the fields are private, so holding a value *is*
/// the proof — there is no way to build an unvalidated instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZdrAttestation {
    resource_id: String,
    approval_ref: String,
    effective_date: NaiveDate,
    review_by: NaiveDate,
    reviewer: String,
    digest: String,
}

impl ZdrAttestation {
    /// Validate raw operator fields into an attestation.
    ///
    /// `today` is injected rather than read from the clock so the in-force check
    /// is deterministic in tests and so one boot uses a single consistent date
    /// across every provider surface.
    ///
    /// # Errors
    ///
    /// Returns [`ZdrAttestationError`] when any field is blank, the date is
    /// unparseable, the digest does not match the supplied fields, or the
    /// effective date has not yet arrived.
    pub fn validate(
        prefix: &str,
        raw: &RawAttestation,
        today: NaiveDate,
    ) -> Result<Self, ZdrAttestationError> {
        let present = [
            &raw.resource_id,
            &raw.approval_ref,
            &raw.effective_date,
            &raw.review_by,
            &raw.reviewer,
        ];
        let mut missing: Vec<String> = FIELD_SUFFIXES
            .iter()
            .zip(present)
            .filter(|(_, value)| value.trim().is_empty())
            .map(|(suffix, _)| format!("{prefix}{suffix}"))
            .collect();
        if raw.digest.trim().is_empty() {
            missing.push(format!("{prefix}{DIGEST_SUFFIX}"));
        }
        if !missing.is_empty() {
            return Err(ZdrAttestationError::Incomplete {
                prefix: prefix.to_owned(),
                missing,
            });
        }

        // Reject *interior* newlines before hashing: the canonical form is
        // newline-delimited, so a field containing one lets two different tuples
        // hash identically. Leading/trailing whitespace is an ordinary `.env`
        // artifact and is trimmed before hashing, so it is harmless — only a break
        // inside the value shifts the delimiters.
        for (suffix, value) in FIELD_SUFFIXES.iter().zip(present) {
            let trimmed = value.trim();
            if trimmed.contains('\n') || trimmed.contains('\r') {
                return Err(ZdrAttestationError::FieldContainsNewline {
                    prefix: prefix.to_owned(),
                    field: format!("{prefix}{suffix}"),
                });
            }
        }

        let effective_date = NaiveDate::parse_from_str(raw.effective_date.trim(), "%Y-%m-%d")
            .map_err(|_| ZdrAttestationError::InvalidEffectiveDate {
                prefix: prefix.to_owned(),
                value: raw.effective_date.clone(),
            })?;

        let review_by =
            NaiveDate::parse_from_str(raw.review_by.trim(), "%Y-%m-%d").map_err(|_| {
                ZdrAttestationError::InvalidReviewBy {
                    prefix: prefix.to_owned(),
                    value: raw.review_by.clone(),
                }
            })?;
        if review_by <= effective_date {
            return Err(ZdrAttestationError::ReviewBeforeEffective {
                prefix: prefix.to_owned(),
                effective: effective_date,
                review_by,
            });
        }

        let expected = canonical_digest(raw);
        if !raw.digest.trim().eq_ignore_ascii_case(&expected) {
            return Err(ZdrAttestationError::DigestMismatch {
                prefix: prefix.to_owned(),
                supplied: raw.digest.trim().to_owned(),
                expected,
            });
        }

        if effective_date > today {
            return Err(ZdrAttestationError::NotYetEffective {
                prefix: prefix.to_owned(),
                effective: effective_date,
                today,
            });
        }

        // Upper bound. An approval that lapsed months ago is not evidence of
        // anything, and the operator who set it is not going to be reminded by
        // anything else.
        if review_by < today {
            return Err(ZdrAttestationError::Expired {
                prefix: prefix.to_owned(),
                review_by,
                today,
            });
        }

        Ok(Self {
            resource_id: raw.resource_id.trim().to_owned(),
            approval_ref: raw.approval_ref.trim().to_owned(),
            effective_date,
            review_by,
            reviewer: raw.reviewer.trim().to_owned(),
            digest: expected,
        })
    }

    /// Read and validate an attestation from `{prefix}_ZDR_*` variables.
    ///
    /// Returns `Ok(None)` when no attestation field is present at all — the
    /// ordinary "this deployment makes no ZDR claim" case. A partially supplied
    /// attestation is an error, not a silent `None`: it means someone intended a
    /// claim and mistyped it.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::validate`].
    pub fn from_env(prefix: &str, today: NaiveDate) -> Result<Option<Self>, ZdrAttestationError> {
        let read = |suffix: &str| -> String {
            std::env::var(format!("{prefix}{suffix}"))
                .map(|value| value.trim().to_owned())
                .unwrap_or_default()
        };

        let raw = RawAttestation {
            resource_id: read(FIELD_SUFFIXES[0]),
            approval_ref: read(FIELD_SUFFIXES[1]),
            effective_date: read(FIELD_SUFFIXES[2]),
            review_by: read(FIELD_SUFFIXES[3]),
            reviewer: read(FIELD_SUFFIXES[4]),
            digest: read(DIGEST_SUFFIX),
        };

        if raw.resource_id.is_empty()
            && raw.approval_ref.is_empty()
            && raw.effective_date.is_empty()
            && raw.review_by.is_empty()
            && raw.reviewer.is_empty()
            && raw.digest.is_empty()
        {
            return Ok(None);
        }

        Self::validate(prefix, &raw, today).map(Some)
    }

    /// Resolve the attestation for one provider surface, rejecting a legacy
    /// boolean that asserts ZDR without evidence.
    ///
    /// `legacy_confirmed` is the previous `*_ZDR_CONFIRMED` boolean. It is
    /// deliberately no longer sufficient on its own — the whole point of this
    /// module — but silently ignoring it would strip ZDR from a deployment whose
    /// operator believes it is enabled, so an unbacked `true` fails boot.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::from_env`], and returns
    /// [`ZdrAttestationError::LegacyBooleanWithoutEvidence`] when
    /// `legacy_confirmed` is set with no attestation present.
    pub fn resolve(
        prefix: &str,
        legacy_var: &'static str,
        legacy_confirmed: bool,
        today: NaiveDate,
    ) -> Result<Option<Self>, ZdrAttestationError> {
        let attestation = Self::from_env(prefix, today)?;
        if attestation.is_none() && legacy_confirmed {
            return Err(ZdrAttestationError::LegacyBooleanWithoutEvidence {
                legacy_var,
                prefix: prefix.to_owned(),
            });
        }
        Ok(attestation)
    }

    /// Warn when the attested resource id does not appear to describe `endpoint`.
    ///
    /// Catches the realistic failure: the five `*_ZDR_*` variables are a contiguous
    /// block, so they get copied to a new environment together while the endpoint
    /// is repointed at a resource that has no retention exception. The digest still
    /// matches there — it only proves the fields agree with each other — so this is
    /// the one signal that the attestation may be describing somewhere else.
    ///
    /// A heuristic, and deliberately only a warning: an Azure resource id is an
    /// opaque operator-authored string whose final path segment is *usually* the
    /// resource name that also appears in the endpoint host, but nothing guarantees
    /// it. Failing boot on a string-shape mismatch would reject correct
    /// configurations, so this reports and lets the operator judge.
    pub fn warn_on_endpoint_mismatch(&self, surface: &str, endpoint: &str) {
        if !self.endpoint_appears_to_match(endpoint) {
            let host_label = endpoint
                .trim()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('.')
                .next()
                .unwrap_or_default();
            tracing::warn!(
                surface = %surface,
                attested_resource = %self.resource_id,
                endpoint_host = %host_label,
                approval_ref = %self.approval_ref,
                "ZDR attestation may not describe the configured endpoint: the attested resource \
                 name does not appear in the endpoint host. Verify the attestation covers THIS \
                 resource — the digest proves only that its own fields agree with each other."
            );
        }
    }

    /// Whether the attested resource name plausibly appears in `endpoint`.
    ///
    /// Answers `true` when it cannot tell — a blank or hostless endpoint, or an
    /// empty resource name — so the caller warns only on a positive mismatch.
    #[must_use]
    pub fn endpoint_appears_to_match(&self, endpoint: &str) -> bool {
        let host = endpoint
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let Some(host_label) = host.split('.').next().filter(|label| !label.is_empty()) else {
            return true;
        };
        let resource_name = self
            .resource_id
            .rsplit('/')
            .next()
            .unwrap_or(&self.resource_id);
        if resource_name.is_empty() {
            return true;
        }
        let host_label = host_label.to_ascii_lowercase();
        let resource_name = resource_name.to_ascii_lowercase();
        host_label == resource_name
            || host_label.contains(&resource_name)
            || resource_name.contains(&host_label)
    }

    /// The cloud resource this attestation covers.
    #[must_use]
    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }

    /// The retention-exception approval reference backing the claim.
    #[must_use]
    pub fn approval_ref(&self) -> &str {
        &self.approval_ref
    }

    /// The date the approval took effect.
    #[allow(dead_code)] // surfaced by the provenance receipt (strategy doc Phase 4)
    #[must_use]
    pub const fn effective_date(&self) -> NaiveDate {
        self.effective_date
    }

    /// The date the retention approval must be re-verified by. Past this the
    /// attestation stops validating.
    #[allow(dead_code)] // surfaced by the provenance receipt (strategy doc Phase 4)
    #[must_use]
    pub const fn review_by(&self) -> NaiveDate {
        self.review_by
    }

    /// Who signed off on the claim.
    #[allow(dead_code)] // surfaced by the provenance receipt (strategy doc Phase 4)
    #[must_use]
    pub fn reviewer(&self) -> &str {
        &self.reviewer
    }

    /// The validated lowercase-hex digest, suitable for logs and for the
    /// per-request provenance receipt.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Canonical digest over the four attestation fields.
///
/// Trimmed field values joined by newlines with a trailing newline — the exact
/// bytes the documented `printf ... | sha256sum` pipeline produces.
pub(crate) fn canonical_digest(raw: &RawAttestation) -> String {
    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        raw.resource_id.trim(),
        raw.approval_ref.trim(),
        raw.effective_date.trim(),
        raw.review_by.trim(),
        raw.reviewer.trim()
    );
    to_lower_hex(&Sha256::digest(canonical.as_bytes()))
}

/// Lowercase hex encoding, matching `execution-core`'s attestation helper so
/// digests are comparable by eye across services.
fn to_lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        // Writing to a String is infallible; the Result exists only to satisfy
        // the fmt::Write contract.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::{canonical_digest, RawAttestation, ZdrAttestation, ZdrAttestationError};
    use chrono::NaiveDate;

    const PREFIX: &str = "AZURE_OPENAI";

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 17).expect("static date is valid")
    }

    /// A well-formed attestation with a correctly computed digest.
    fn valid() -> RawAttestation {
        let mut raw = RawAttestation {
            resource_id: "/subscriptions/abc/rg/eu/openai-swedencentral".to_owned(),
            approval_ref: "MAM-2026-0042".to_owned(),
            effective_date: "2026-06-01".to_owned(),
            review_by: "2027-06-01".to_owned(),
            reviewer: "ima@aquatiq.com".to_owned(),
            digest: String::new(),
        };
        raw.digest = canonical_digest(&raw);
        raw
    }

    #[test]
    fn complete_and_matching_attestation_validates() {
        let attestation =
            ZdrAttestation::validate(PREFIX, &valid(), today()).expect("valid attestation");
        assert_eq!(attestation.approval_ref(), "MAM-2026-0042");
        assert_eq!(attestation.reviewer(), "ima@aquatiq.com");
        assert_eq!(
            attestation.effective_date(),
            NaiveDate::from_ymd_opt(2026, 6, 1).expect("static date")
        );
        assert_eq!(attestation.digest().len(), 64);
    }

    #[test]
    fn every_blank_field_is_reported_as_missing() {
        let raw = RawAttestation::default();
        let error = ZdrAttestationError::Incomplete {
            prefix: PREFIX.to_owned(),
            missing: vec![
                "AZURE_OPENAI_ZDR_RESOURCE_ID".to_owned(),
                "AZURE_OPENAI_ZDR_APPROVAL_REF".to_owned(),
                "AZURE_OPENAI_ZDR_EFFECTIVE_DATE".to_owned(),
                "AZURE_OPENAI_ZDR_REVIEW_BY".to_owned(),
                "AZURE_OPENAI_ZDR_REVIEWER".to_owned(),
                "AZURE_OPENAI_ZDR_DIGEST".to_owned(),
            ],
        };
        assert_eq!(
            ZdrAttestation::validate(PREFIX, &raw, today()),
            Err(error),
            "an all-blank attestation must name every missing variable"
        );
    }

    #[test]
    fn partial_attestation_is_rejected_rather_than_ignored() {
        let raw = RawAttestation {
            resource_id: "/subscriptions/abc".to_owned(),
            ..RawAttestation::default()
        };
        let error = ZdrAttestation::validate(PREFIX, &raw, today()).expect_err("partial must fail");
        assert!(matches!(error, ZdrAttestationError::Incomplete { .. }));
    }

    /// The load-bearing test: a placeholder digest must not grant ZDR.
    #[test]
    fn placeholder_digest_is_rejected() {
        let raw = RawAttestation {
            digest: "CHANGEME".to_owned(),
            ..valid()
        };
        let error =
            ZdrAttestation::validate(PREFIX, &raw, today()).expect_err("placeholder must fail");
        assert!(matches!(error, ZdrAttestationError::DigestMismatch { .. }));
    }

    /// A digest that is well-formed but computed over *different* fields — the
    /// copy-paste-from-another-resource case — must also fail.
    #[test]
    fn digest_from_a_different_resource_is_rejected() {
        let staging = RawAttestation {
            resource_id: "/subscriptions/abc/rg/eu/openai-STAGING".to_owned(),
            ..valid()
        };
        let raw = RawAttestation {
            digest: canonical_digest(&staging),
            ..valid()
        };
        let error = ZdrAttestation::validate(PREFIX, &raw, today())
            .expect_err("a digest from another resource must fail");
        assert!(matches!(error, ZdrAttestationError::DigestMismatch { .. }));
    }

    /// Changing any single field must invalidate the digest — otherwise the
    /// binding is decorative.
    #[test]
    fn mutating_any_field_invalidates_the_digest() {
        let base = valid();
        let mutations = [
            RawAttestation {
                resource_id: "/subscriptions/other".to_owned(),
                ..base.clone()
            },
            RawAttestation {
                approval_ref: "MAM-9999-0001".to_owned(),
                ..base.clone()
            },
            RawAttestation {
                effective_date: "2026-06-02".to_owned(),
                ..base.clone()
            },
            RawAttestation {
                review_by: "2028-06-01".to_owned(),
                ..base.clone()
            },
            RawAttestation {
                reviewer: "someone.else@example.com".to_owned(),
                ..base.clone()
            },
        ];
        for (index, raw) in mutations.iter().enumerate() {
            let error = ZdrAttestation::validate(PREFIX, raw, today())
                .expect_err("a field changed without recomputing the digest must not validate");
            assert!(
                matches!(error, ZdrAttestationError::DigestMismatch { .. }),
                "mutation {index} produced {error:?}, expected DigestMismatch"
            );
        }
    }

    #[test]
    fn unparseable_effective_date_is_rejected() {
        let mut raw = RawAttestation {
            effective_date: "01/06/2026".to_owned(),
            ..valid()
        };
        // review_by stays valid so the failure is unambiguously the effective date.
        raw.digest = canonical_digest(&raw);
        let error = ZdrAttestation::validate(PREFIX, &raw, today()).expect_err("bad date fails");
        assert!(matches!(
            error,
            ZdrAttestationError::InvalidEffectiveDate { .. }
        ));
    }

    #[test]
    fn future_effective_date_does_not_grant_zdr_yet() {
        let mut raw = RawAttestation {
            effective_date: "2027-01-01".to_owned(),
            review_by: "2028-01-01".to_owned(),
            ..valid()
        };
        raw.digest = canonical_digest(&raw);
        let error = ZdrAttestation::validate(PREFIX, &raw, today()).expect_err("future date fails");
        assert!(matches!(error, ZdrAttestationError::NotYetEffective { .. }));
    }

    /// An attestation effective exactly today is in force.
    #[test]
    fn effective_today_is_in_force() {
        let mut raw = RawAttestation {
            effective_date: "2026-08-17".to_owned(),
            review_by: "2027-08-17".to_owned(),
            ..valid()
        };
        raw.digest = canonical_digest(&raw);
        ZdrAttestation::validate(PREFIX, &raw, today()).expect("effective today is in force");
    }

    /// Surrounding whitespace is an ordinary `.env` artifact and must not change
    /// the digest — the canonical form is over trimmed values.
    #[test]
    fn whitespace_is_trimmed_before_hashing() {
        let base = valid();
        let padded = RawAttestation {
            resource_id: format!("  {}  ", base.resource_id),
            reviewer: format!("\t{}\n", base.reviewer),
            digest: base.digest.clone(),
            ..base.clone()
        };
        let attestation = ZdrAttestation::validate(PREFIX, &padded, today())
            .expect("padded fields must validate against the trimmed digest");
        assert_eq!(attestation.resource_id(), base.resource_id);
    }

    #[test]
    fn legacy_boolean_off_with_no_attestation_is_fine() {
        let resolved = ZdrAttestation::resolve(
            "TEST_ABSENT_PREFIX_NOT_SET",
            "LEGACY_ZDR_CONFIRMED",
            false,
            today(),
        )
        .expect("no claim is not an error");
        assert_eq!(resolved, None);
    }

    #[test]
    fn legacy_boolean_alone_no_longer_grants_zdr() {
        let error = ZdrAttestation::resolve(
            "TEST_ABSENT_PREFIX_NOT_SET",
            "LEGACY_ZDR_CONFIRMED",
            true,
            today(),
        )
        .expect_err("an unbacked legacy boolean must fail boot");
        assert!(matches!(
            error,
            ZdrAttestationError::LegacyBooleanWithoutEvidence { .. }
        ));
    }

    /// The canonical form is newline-delimited, so a field containing a newline
    /// would let two different field tuples hash identically. Proven here, then
    /// rejected: without the guard, these two tuples collide.
    #[test]
    fn newline_in_a_field_is_rejected_because_it_would_collide() {
        let split_in_resource = RawAttestation {
            review_by: "2027-06-01".to_owned(),
            resource_id: "a\nb".to_owned(),
            approval_ref: "c".to_owned(),
            effective_date: "2026-01-01".to_owned(),
            reviewer: "d".to_owned(),
            digest: String::new(),
        };
        let split_in_approval = RawAttestation {
            review_by: "2027-06-01".to_owned(),
            resource_id: "a".to_owned(),
            approval_ref: "b\nc".to_owned(),
            effective_date: "2026-01-01".to_owned(),
            reviewer: "d".to_owned(),
            digest: String::new(),
        };
        // The collision is real: the canonical bytes are identical.
        assert_eq!(
            canonical_digest(&split_in_resource),
            canonical_digest(&split_in_approval),
            "these tuples must collide -- that is why the guard below exists"
        );

        // Both are therefore refused before the digest is ever compared.
        for raw in [&split_in_resource, &split_in_approval] {
            let mut candidate = raw.clone();
            candidate.digest = canonical_digest(raw);
            let error = ZdrAttestation::validate(PREFIX, &candidate, today())
                .expect_err("a newline in a field must be refused");
            assert!(
                matches!(error, ZdrAttestationError::FieldContainsNewline { .. }),
                "expected FieldContainsNewline, got {error:?}"
            );
        }
    }

    /// Carriage returns are the same hazard via CRLF `.env` files.
    #[test]
    fn carriage_return_in_a_field_is_rejected() {
        let raw = RawAttestation {
            reviewer: "ops\r\nteam".to_owned(),
            ..valid()
        };
        let error = ZdrAttestation::validate(PREFIX, &raw, today())
            .expect_err("an interior CRLF must be refused");
        assert!(matches!(
            error,
            ZdrAttestationError::FieldContainsNewline { .. }
        ));
    }

    /// A trailing newline from a CRLF `.env` is trimmed, not rejected — only an
    /// interior break shifts the canonical delimiters.
    #[test]
    fn trailing_newline_is_trimmed_not_rejected() {
        let base = valid();
        let raw = RawAttestation {
            reviewer: format!("{}\r\n", base.reviewer),
            digest: base.digest.clone(),
            ..base.clone()
        };
        let attestation = ZdrAttestation::validate(PREFIX, &raw, today())
            .expect("a trailing CRLF is an ordinary .env artifact");
        assert_eq!(attestation.reviewer(), base.reviewer);
    }

    /// The realistic drift case: the five-variable block is copied to a new
    /// environment whose endpoint points somewhere else. The digest still matches
    /// there, so this heuristic is the only signal.
    #[test]
    fn endpoint_mismatch_is_detected_by_resource_name() {
        let attestation = ZdrAttestation::validate(PREFIX, &valid(), today()).expect("valid");
        // valid()'s resource id ends in `openai-swedencentral`.
        assert!(
            !attestation
                .endpoint_appears_to_match("https://core-ai-rg.cognitiveservices.azure.com"),
            "a different resource must not appear to match"
        );
        assert!(
            attestation.endpoint_appears_to_match("https://openai-swedencentral.openai.azure.com"),
            "the attested resource's own endpoint must match"
        );
    }

    /// The heuristic must not fire on an empty or malformed endpoint, or it would
    /// warn on every deployment that configures the resource id differently.
    #[test]
    fn endpoint_match_is_lenient_about_missing_input() {
        let attestation = ZdrAttestation::validate(PREFIX, &valid(), today()).expect("valid");
        for endpoint in ["", "   ", "https://"] {
            assert!(
                attestation.endpoint_appears_to_match(endpoint),
                "a missing endpoint must not be reported as a mismatch ({endpoint:?})"
            );
        }
    }

    /// The gap this field closes: an approval whose term has run out must stop
    /// granting ZDR rather than coasting on nobody remembering it.
    #[test]
    fn a_lapsed_approval_stops_granting_zdr() {
        let mut raw = RawAttestation {
            effective_date: "2025-06-01".to_owned(),
            review_by: "2026-06-01".to_owned(),
            ..valid()
        };
        raw.digest = canonical_digest(&raw);
        // today() is 2026-08-17, two months past review-by.
        let error = ZdrAttestation::validate(PREFIX, &raw, today()).expect_err("lapsed must fail");
        assert!(matches!(error, ZdrAttestationError::Expired { .. }));
    }

    /// The boundary: still in force on the review-by date itself.
    #[test]
    fn review_by_today_is_still_in_force() {
        let mut raw = RawAttestation {
            effective_date: "2026-01-01".to_owned(),
            review_by: "2026-08-17".to_owned(),
            ..valid()
        };
        raw.digest = canonical_digest(&raw);
        ZdrAttestation::validate(PREFIX, &raw, today())
            .expect("an approval reviewed today is still in force");
    }

    #[test]
    fn review_by_must_be_after_the_effective_date() {
        let mut raw = RawAttestation {
            effective_date: "2026-06-01".to_owned(),
            review_by: "2026-06-01".to_owned(),
            ..valid()
        };
        raw.digest = canonical_digest(&raw);
        let error = ZdrAttestation::validate(PREFIX, &raw, today())
            .expect_err("an approval cannot lapse before it starts");
        assert!(matches!(
            error,
            ZdrAttestationError::ReviewBeforeEffective { .. }
        ));
    }

    #[test]
    fn unparseable_review_by_is_rejected() {
        let mut raw = RawAttestation {
            review_by: "next year".to_owned(),
            ..valid()
        };
        raw.digest = canonical_digest(&raw);
        let error =
            ZdrAttestation::validate(PREFIX, &raw, today()).expect_err("bad review-by fails");
        assert!(matches!(error, ZdrAttestationError::InvalidReviewBy { .. }));
    }

    /// Digest stability: the canonical form is a documented operator contract
    /// (`printf '%s\n%s\n%s\n%s\n' ... | sha256sum`). Changing it silently
    /// invalidates every deployed attestation, so pin a known-good vector
    /// computed independently of this code.
    #[test]
    fn canonical_digest_matches_an_independently_computed_vector() {
        let raw = RawAttestation {
            resource_id: "res".to_owned(),
            approval_ref: "appr".to_owned(),
            effective_date: "2026-01-01".to_owned(),
            review_by: "2027-01-01".to_owned(),
            reviewer: "rev".to_owned(),
            digest: String::new(),
        };
        // Computed outside this code with the documented operator pipeline:
        //   $ printf '%s\n%s\n%s\n%s\n%s\n' res appr 2026-01-01 2027-01-01 rev \
        //       | shasum -a 256
        assert_eq!(
            canonical_digest(&raw),
            "46acd4025f8909cfb4eb98b81027c932cd117b005acf54403a863124721b7aa5",
            "canonical digest changed — every deployed AZURE_*_ZDR_DIGEST is now invalid"
        );
    }
}
