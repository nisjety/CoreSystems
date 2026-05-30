//! NATS-based implementation of `EventPublisher`.
//!
//! Connects to the NATS server specified by `NATS_URL` env var
//! (default: `nats://localhost:4222`).

use mp_events::{
    envelope::Envelope,
    publisher::{EventPublisher, PublishError},
    subjects::{self, CompatMode},
};

/// Publishes event envelopes to a NATS server.
pub struct NatsPublisher {
    client: async_nats::Client,
    mode: CompatMode,
}

impl NatsPublisher {
    /// Connect to the NATS server at the given URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(url: &str) -> Result<Self, async_nats::ConnectError> {
        let client = async_nats::connect(url).await?;
        let mode = CompatMode::from_env();
        tracing::info!(url = %url, compat_mode = mode.as_str(), "connected to NATS");
        Ok(Self { client, mode })
    }

    fn dispatch_subjects(mode: CompatMode, subject: &str) -> Result<Vec<String>, PublishError> {
        match mode {
            CompatMode::V1Only | CompatMode::DualRead => Ok(vec![subject.to_owned()]),
            CompatMode::DualWrite => {
                let mut subjects = vec![subject.to_owned()];
                if let Some(legacy_subject) = subjects::translate_new_to_legacy(subject) {
                    subjects.push(legacy_subject);
                }
                Ok(subjects)
            }
            CompatMode::LegacyOnly => subjects::translate_new_to_legacy(subject)
                .map(|legacy_subject| vec![legacy_subject])
                .ok_or_else(|| {
                    PublishError::Transport(format!(
                        "no legacy mapping for subject {subject} in legacy_only mode"
                    ))
                }),
        }
    }
}

impl EventPublisher for NatsPublisher {
    async fn publish(&self, subject: &str, envelope: &Envelope) -> Result<(), PublishError> {
        let bytes = envelope.to_json_bytes()?;

        for publish_subject in Self::dispatch_subjects(self.mode, subject)? {
            self.client
                .publish(publish_subject, bytes.clone().into())
                .await
                .map_err(|e| PublishError::Transport(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_subjects_v1_only_keeps_canonical_subject() {
        assert_eq!(
            NatsPublisher::dispatch_subjects(CompatMode::V1Only, "mp.v1.run.run-1.event")
                .expect("subjects"),
            vec!["mp.v1.run.run-1.event".to_owned()]
        );
    }

    #[test]
    fn dispatch_subjects_dual_write_adds_legacy_mirror_when_available() {
        assert_eq!(
            NatsPublisher::dispatch_subjects(CompatMode::DualWrite, "mp.v1.run.run-1.event")
                .expect("subjects"),
            vec![
                "mp.v1.run.run-1.event".to_owned(),
                "velion.agent.run.run-1.event".to_owned(),
            ]
        );
    }

    #[test]
    fn dispatch_subjects_dual_write_mirrors_ingress_usage_to_aqencia() {
        assert_eq!(
            NatsPublisher::dispatch_subjects(CompatMode::DualWrite, "mp.v1.ingress.usage")
                .expect("subjects"),
            vec![
                "mp.v1.ingress.usage".to_owned(),
                "aqencia.reasoning.usage.recorded".to_owned(),
            ]
        );
    }

    #[test]
    fn dispatch_subjects_legacy_only_rejects_unmapped_subjects() {
        let error =
            NatsPublisher::dispatch_subjects(CompatMode::LegacyOnly, "mp.v1.ingress.accepted")
                .expect_err("legacy_only should reject unmapped subjects");
        assert!(error
            .to_string()
            .contains("no legacy mapping for subject mp.v1.ingress.accepted"));
    }
}
