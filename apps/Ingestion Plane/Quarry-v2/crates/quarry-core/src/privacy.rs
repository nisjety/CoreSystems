//! Privacy and third-party processing policy metadata.
//!
//! This is the Quarry-side shape of the cross-plane GDPR policy contract. It
//! travels with runs, fetch hints, persisted outputs, and Data Plane ingest
//! requests so egress/provider decisions are explicit and auditable.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{ErrorCode, QuarryError, QuarryResult};
use crate::zdr::ZdrMode;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyClassification {
    PublicNonPersonal,
    #[default]
    CustomerPrivate,
    Personal,
    SensitivePersonal,
    CredentialOrSecret,
    ZdrEphemeral,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lawful_basis: Option<String>,
    pub privacy_classification: PrivacyClassification,
    pub zdr: ZdrMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residency: Option<String>,
    #[serde(default)]
    pub allow_third_party_processing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processor_id: Option<String>,
}

impl Default for PrivacyPolicy {
    fn default() -> Self {
        Self {
            purpose_id: None,
            lawful_basis: None,
            privacy_classification: PrivacyClassification::CustomerPrivate,
            zdr: ZdrMode::Off,
            retention_policy: None,
            residency: None,
            allow_third_party_processing: false,
            processor_id: None,
        }
    }
}

impl PrivacyPolicy {
    pub fn with_zdr(mut self, zdr: ZdrMode) -> Self {
        self.zdr = zdr;
        self
    }

    pub fn guard_third_party_processing(
        &self,
        provider_type: &str,
        processor_id: &str,
    ) -> QuarryResult<()> {
        let processor_id = processor_id.trim();
        if processor_id.is_empty() {
            return Err(self.denial(provider_type, processor_id, "processor_id_missing"));
        }
        if self.zdr.is_active()
            || self.privacy_classification == PrivacyClassification::ZdrEphemeral
        {
            return Err(self.denial(provider_type, processor_id, "zdr_denies_third_party"));
        }
        if self.privacy_classification == PrivacyClassification::CredentialOrSecret {
            return Err(self.denial(
                provider_type,
                processor_id,
                "credential_or_secret_denies_third_party",
            ));
        }
        if !self.allow_third_party_processing {
            return Err(self.denial(provider_type, processor_id, "third_party_not_allowed"));
        }
        match self
            .processor_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(allowed) if allowed == processor_id => Ok(()),
            Some(_) => Err(self.denial(provider_type, processor_id, "processor_mismatch")),
            None => Err(self.denial(provider_type, processor_id, "processor_id_required")),
        }
    }

    fn denial(&self, provider_type: &str, processor_id: &str, reason: &str) -> QuarryError {
        QuarryError::new(
            ErrorCode::Forbidden,
            format!("third-party {provider_type} denied by privacy policy: {reason}"),
        )
        .with_details(json!({
            "reason": reason,
            "provider_type": provider_type,
            "processor_id": processor_id,
            "policy_processor_id": self.processor_id,
            "privacy_classification": self.privacy_classification,
            "zdr": self.zdr.is_active(),
            "allow_third_party_processing": self.allow_third_party_processing,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_private_and_denies_third_party() {
        let policy = PrivacyPolicy::default();

        assert_eq!(
            policy.privacy_classification,
            PrivacyClassification::CustomerPrivate
        );
        assert!(!policy.allow_third_party_processing);
        let err = policy
            .guard_third_party_processing("proxy", "quarry_proxy_pool")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[test]
    fn matching_processor_allows_third_party_when_explicit() {
        let policy = PrivacyPolicy {
            allow_third_party_processing: true,
            processor_id: Some("browserbase".into()),
            ..PrivacyPolicy::default()
        };

        assert!(policy
            .guard_third_party_processing("browser", "browserbase")
            .is_ok());
    }

    #[test]
    fn mismatched_processor_is_denied() {
        let policy = PrivacyPolicy {
            allow_third_party_processing: true,
            processor_id: Some("browserbase".into()),
            ..PrivacyPolicy::default()
        };

        let err = policy
            .guard_third_party_processing("proxy", "quarry_proxy_pool")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }

    #[test]
    fn zdr_denies_third_party_even_when_processor_matches() {
        let policy = PrivacyPolicy {
            allow_third_party_processing: true,
            processor_id: Some("browserbase".into()),
            ..PrivacyPolicy::default()
        }
        .with_zdr(ZdrMode::On);

        let err = policy
            .guard_third_party_processing("browser", "browserbase")
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }
}
