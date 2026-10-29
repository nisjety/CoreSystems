//! gRPC `GrantValidator` implementation calling Model Plane's
//! `BrowserBroker` over tonic.
//!
//! Quarry brokers browser leases internally, but Model Plane issues *grants*
//! that authorize a specific Model Plane session to use Quarry's browser. The
//! `BrowserBroker` is the authority — Quarry calls `ValidateGrant`
//! before any privileged browser action.

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use super::model_plane::v1::browser_broker_client::BrowserBrokerClient;
use super::model_plane::v1::{
    AcquireGrantRequest, AcquireGrantResponse, RevokeGrantRequest, ValidateGrantRequest,
};
use crate::grant_validator::{GrantValidation, GrantValidator};

#[derive(Clone)]
pub struct GrpcGrantValidator {
    channel: Channel,
    auth_token: Option<String>,
}

impl GrpcGrantValidator {
    /// Connect to the Model Plane browser broker over gRPC.
    /// Endpoint examples: `http://browser-broker:9090`, `https://broker.modelplane.svc:443`.
    ///
    /// Production-grade settings:
    /// - HTTP/2 keep-alive ping every 30s with 10s timeout — detects dead
    ///   connections quickly so the next call doesn't surface a stale-conn
    ///   error.
    /// - `keep_alive_while_idle = true` — keeps the connection alive even
    ///   when no requests are in-flight (otherwise tonic only pings during
    ///   active streams).
    /// - Connect timeout 5s, request timeout 10s.
    /// - Native root CAs for HTTPS endpoints.
    ///
    /// tonic's load-balanced channel automatically reconnects when a
    /// connection breaks, so callers get transparent recovery.
    pub async fn connect(endpoint: impl Into<String>) -> QuarryResult<Self> {
        let endpoint_str = endpoint.into();
        let mut ep = Endpoint::from_shared(endpoint_str.clone()).map_err(|e| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("invalid grpc endpoint {endpoint_str}: {e}"),
            )
        })?;
        ep = ep
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true)
            .tcp_keepalive(Some(Duration::from_secs(60)));
        if endpoint_str.starts_with("https://") {
            ep = ep
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("tls config failed: {e}"))
                })?;
        }

        let channel = ep.connect().await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, format!("grpc connect failed: {e}"))
        })?;

        Ok(Self {
            channel,
            auth_token: None,
        })
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    fn client(&self) -> BrowserBrokerClient<Channel> {
        BrowserBrokerClient::new(self.channel.clone())
    }

    fn apply_auth<T>(&self, mut req: tonic::Request<T>) -> tonic::Request<T> {
        if let Some(token) = &self.auth_token {
            if let Ok(value) = format!("Bearer {token}").parse() {
                req.metadata_mut().insert("authorization", value);
            }
        }
        req
    }

    /// Acquire a grant (used when Quarry is the issuer, less common path).
    pub async fn acquire(
        &self,
        session_key: &str,
        mode: &str,
        org_id: &str,
    ) -> QuarryResult<AcquiredGrant> {
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(AcquireGrantRequest {
            session_key: session_key.to_string(),
            mode: mode.to_string(),
            org_id: org_id.to_string(),
            // Quarry normally validates Model Plane-issued grants rather than
            // issuing them. Keep this legacy acquisition path deny-by-default
            // until its caller can supply a broker-owned domain policy.
            allowed_domains: Vec::new(),
            allowed_actions: Vec::new(),
            allowed_frame_ids: Vec::new(),
            allowed_dialog_ids: Vec::new(),
            allowed_artifact_ids: Vec::new(),
            // Proto3 represents an absent parent grant as the empty string.
            // This legacy issuance path remains deny-by-default above and does
            // not claim delegation from a parent grant.
            parent_grant_id: String::new(),
        }));
        let resp: AcquireGrantResponse = client
            .acquire_grant(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        Ok(AcquiredGrant {
            grant_id: resp.grant_id,
            endpoint: resp.endpoint,
            expires_at: resp.expires_at.and_then(prost_ts_to_chrono),
        })
    }

    /// Revoke a grant (Quarry calls this when a session ends abnormally).
    pub async fn revoke(&self, grant_id: &str, reason: &str) -> QuarryResult<bool> {
        let mut client = self.client();
        let request = self.apply_auth(tonic::Request::new(RevokeGrantRequest {
            grant_id: grant_id.to_string(),
            reason: reason.to_string(),
        }));
        let resp = client
            .revoke_grant(request)
            .await
            .map_err(grpc_status_to_error)?
            .into_inner();
        Ok(resp.revoked)
    }
}

#[derive(Debug, Clone)]
pub struct AcquiredGrant {
    pub grant_id: String,
    pub endpoint: String,
    pub expires_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl GrantValidator for GrpcGrantValidator {
    /// Validate with bounded retry on transient failures.
    ///
    /// Retries up to 2 additional attempts (3 total) on `Unavailable`,
    /// `Internal`, `DeadlineExceeded` with exponential backoff (50ms, 200ms).
    /// Other statuses (NotFound, PermissionDenied, InvalidArgument) bubble
    /// up immediately so callers don't waste time retrying things that
    /// won't change.
    async fn validate(&self, grant_id: &str) -> QuarryResult<GrantValidation> {
        if grant_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "grant_id must not be empty",
            ));
        }

        let mut last_err: Option<QuarryError> = None;
        for (attempt, backoff_ms) in [(1u32, 0u64), (2, 50), (3, 200)] {
            if backoff_ms > 0 {
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
            let mut client = self.client();
            let request = self.apply_auth(tonic::Request::new(ValidateGrantRequest {
                grant_id: grant_id.to_string(),
            }));
            match client.validate_grant(request).await {
                Ok(r) => {
                    let resp = r.into_inner();
                    return Ok(GrantValidation {
                        grant_id: resp.grant_id,
                        active: resp.active,
                        expires_at: resp.expires_at.and_then(prost_ts_to_chrono),
                        allowed_domains: resp.allowed_domains,
                        allowed_actions: resp.allowed_actions,
                        allowed_frame_ids: resp.allowed_frame_ids,
                        allowed_dialog_ids: resp.allowed_dialog_ids,
                        allowed_artifact_ids: resp.allowed_artifact_ids,
                        parent_grant_id: (!resp.parent_grant_id.is_empty())
                            .then_some(resp.parent_grant_id),
                    });
                }
                Err(status) if status.code() == tonic::Code::NotFound => {
                    return Ok(GrantValidation {
                        grant_id: grant_id.to_string(),
                        active: false,
                        expires_at: None,
                        allowed_domains: Vec::new(),
                        allowed_actions: Vec::new(),
                        allowed_frame_ids: Vec::new(),
                        allowed_dialog_ids: Vec::new(),
                        allowed_artifact_ids: Vec::new(),
                        parent_grant_id: None,
                    });
                }
                Err(status) if is_transient(status.code()) => {
                    tracing::warn!(
                        attempt,
                        code = ?status.code(),
                        "ValidateGrant transient failure; retrying"
                    );
                    last_err = Some(grpc_status_to_error(status));
                    continue;
                }
                Err(status) => return Err(grpc_status_to_error(status)),
            }
        }
        Err(last_err.unwrap_or_else(|| {
            QuarryError::new(ErrorCode::DriverFailed, "validate_grant exhausted retries")
        }))
    }
}

/// gRPC status codes considered transient for retry purposes.
pub(crate) fn is_transient(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::Unavailable | tonic::Code::Internal | tonic::Code::DeadlineExceeded
    )
}

fn prost_ts_to_chrono(ts: prost_types::Timestamp) -> Option<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(ts.seconds, ts.nanos.max(0) as u32)
}

pub(crate) fn grpc_status_to_error(status: tonic::Status) -> QuarryError {
    let code = match status.code() {
        tonic::Code::Unauthenticated | tonic::Code::PermissionDenied => ErrorCode::Forbidden,
        tonic::Code::NotFound => ErrorCode::NotFound,
        tonic::Code::InvalidArgument => ErrorCode::BadRequest,
        tonic::Code::DeadlineExceeded => ErrorCode::Timeout,
        tonic::Code::ResourceExhausted => ErrorCode::RateLimited,
        _ => ErrorCode::DriverFailed,
    };
    QuarryError::new(
        code,
        format!("grpc {}: {}", status.code(), status.message()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prost_ts_to_chrono_handles_zero() {
        let ts = prost_types::Timestamp {
            seconds: 0,
            nanos: 0,
        };
        let dt = prost_ts_to_chrono(ts).unwrap();
        assert_eq!(dt.timestamp(), 0);
    }

    #[test]
    fn prost_ts_to_chrono_handles_negative_nanos() {
        // protobuf spec says nanos must be in [0, 999_999_999], but we handle
        // accidental negatives without crashing.
        let ts = prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: -10,
        };
        let dt = prost_ts_to_chrono(ts).unwrap();
        assert_eq!(dt.timestamp(), 1_700_000_000);
    }

    #[test]
    fn grpc_status_to_error_maps_codes() {
        let err = grpc_status_to_error(tonic::Status::not_found("missing"));
        assert_eq!(err.code, ErrorCode::NotFound);

        let err = grpc_status_to_error(tonic::Status::deadline_exceeded("slow"));
        assert_eq!(err.code, ErrorCode::Timeout);

        let err = grpc_status_to_error(tonic::Status::resource_exhausted("rate"));
        assert_eq!(err.code, ErrorCode::RateLimited);

        let err = grpc_status_to_error(tonic::Status::permission_denied("nope"));
        assert_eq!(err.code, ErrorCode::Forbidden);
    }
}
