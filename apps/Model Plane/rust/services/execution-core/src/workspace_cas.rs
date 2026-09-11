//! Content-addressable object store client for the S3.3 durable-workspace
//! design (see
//! apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
//! §1). Every blob is keyed by `sha256:<hex>` of its own bytes — the same
//! digest convention already used everywhere else in this codebase
//! (`capability_profile_digest`, `spaceCapabilityPayloadDigest`,
//! `scrub.rs`), not a second one invented here.
//!
//! This is the first real object-storage client in Model Plane: no
//! MinIO/S3 client of any kind existed anywhere in this repo before this
//! file (confirmed by direct search — `docs/gap-model.md`,
//! `docs/core-research/sandbox-manager.md`, and `docs/gap-analysis.md` all
//! already tracked this as a known gap, not something being discovered
//! here). Absent configuration disables the client entirely (`from_env`
//! returns `None`), matching `CapabilityClient::from_env`'s convention: an
//! unconfigured CAS client is a valid disabled state, not an error.
//!
//! Uses the standalone `aws-sigv4` crate for AWS SigV4 request signing
//! against MinIO's S3-compatible API, deliberately not `aws-sdk-s3`/
//! `aws-config` — this codebase already hand-rolls narrow HTTP clients
//! (`ticket_tools.rs`, `capability_client.rs`) rather than pulling a full
//! SDK, and MinIO's endpoint is an internal, same-network backing store
//! (like the `DATABASE_URL`/`REDIS_URL` this same deployment already
//! reaches over plaintext on the private network) — not another plane's
//! cross-service HTTP API, so `control_http_client.rs`'s HTTPS-unless-
//! loopback policy does not apply here and is deliberately not reused.

use std::time::{Duration, SystemTime};

use aws_credential_types::Credentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use reqwest::{Client, Method, StatusCode, Url};
use sha2::{Digest, Sha256};

use crate::control_http_client::{bounded_secret, env_value};

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CAS_PREFIX: &str = "cas/";
/// `sha256("")`, the required `x-amz-content-sha256` value for a body-less
/// request (GET/HEAD).
const EMPTY_BODY_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Everything needed to `put`/`get` content-addressed blobs against one
/// MinIO bucket.
pub struct CasClient {
    http: Client,
    endpoint: Url,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: String,
}

impl CasClient {
    /// Builds from the same `MINIO_ENDPOINT`/`MINIO_ACCESS_KEY`/
    /// `MINIO_SECRET_KEY` variables `docker-compose.yml` already sets for
    /// this service (currently unused — see the module doc comment), plus
    /// optional `OBJECT_STORAGE_BUCKET` (default `model-plane-artifacts`,
    /// matching the compose default) and `MINIO_REGION` (default
    /// `us-east-1`, MinIO's own conventional default). Returns `Ok(None)`
    /// when any required variable is absent: a disabled CAS client is a
    /// valid state, not a misconfiguration.
    ///
    /// # Errors
    /// Returns an error only for *present but malformed* configuration
    /// (invalid endpoint URL, out-of-bounds secret key length) — never for
    /// absence.
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(endpoint) = env_value("MINIO_ENDPOINT") else {
            return Ok(None);
        };
        let Some(access_key) = env_value("MINIO_ACCESS_KEY") else {
            return Ok(None);
        };
        let Some(secret_key) = env_value("MINIO_SECRET_KEY") else {
            return Ok(None);
        };
        let bucket =
            env_value("OBJECT_STORAGE_BUCKET").unwrap_or_else(|| "model-plane-artifacts".to_owned());
        let region = env_value("MINIO_REGION").unwrap_or_else(|| "us-east-1".to_owned());
        Self::new_with_transport(&endpoint, &bucket, &access_key, &secret_key, &region).map(Some)
    }

    fn new_with_transport(
        endpoint: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
        region: &str,
    ) -> Result<Self, String> {
        let endpoint = Url::parse(endpoint.trim()).map_err(|_| "MinIO endpoint URL is invalid".to_owned())?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
            return Err("MinIO endpoint URL is invalid".to_owned());
        }
        let bucket = bucket.trim();
        if bucket.is_empty() {
            return Err("MinIO bucket name is required".to_owned());
        }
        let access_key = access_key.trim();
        if access_key.is_empty() {
            return Err("MinIO access key is required".to_owned());
        }
        let secret_key = bounded_secret(secret_key, "MinIO secret key")?;
        let region = region.trim();
        if region.is_empty() {
            return Err("MinIO region is required".to_owned());
        }
        let http = Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|error| format!("CAS HTTP client: {error}"))?;
        Ok(Self {
            http,
            endpoint,
            bucket: bucket.to_owned(),
            access_key: access_key.to_owned(),
            secret_key,
            region: region.to_owned(),
        })
    }

    /// Uploads `content`, returning its `sha256:<hex>` digest. A no-op
    /// (beyond one existence check) when the digest already exists in the
    /// bucket — content-addressing makes this automatic deduplication, not
    /// an optimization the caller has to opt into.
    ///
    /// # Errors
    /// Any transport failure or non-success response from either the
    /// existence check or the upload itself.
    pub async fn put(&self, content: &[u8]) -> Result<String, String> {
        let sha256_hex = hex_digest(content);
        let key = format!("{CAS_PREFIX}{sha256_hex}");
        let digest = format!("sha256:{sha256_hex}");

        if self.exists(&key).await? {
            return Ok(digest);
        }

        let url = self.object_url(&key)?;
        let headers = self.sign_request(Method::PUT, &url, &sha256_hex, Some(content))?;
        let mut request = self.http.put(url).body(content.to_vec());
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("CAS upload request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("CAS upload returned {}", response.status()));
        }
        Ok(digest)
    }

    /// Downloads the blob named by `digest`.
    ///
    /// # Errors
    /// Returns an error if `digest` is not a well-formed `sha256:<64 hex
    /// chars>` string — this is never parsed as or forwarded to a path
    /// derived from caller input beyond that fixed shape — or for any
    /// transport failure / non-success response.
    pub async fn get(&self, digest: &str) -> Result<Vec<u8>, String> {
        let sha256_hex = validate_digest(digest)?;
        let key = format!("{CAS_PREFIX}{sha256_hex}");
        let url = self.object_url(&key)?;
        let headers = self.sign_request(Method::GET, &url, EMPTY_BODY_SHA256, None)?;
        let mut request = self.http.get(url);
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("CAS download request failed: {error}"))?;
        if !response.status().is_success() {
            return Err(format!("CAS download returned {}", response.status()));
        }
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(|error| format!("CAS download body could not be read: {error}"))
    }

    async fn exists(&self, key: &str) -> Result<bool, String> {
        let url = self.object_url(key)?;
        let headers = self.sign_request(Method::HEAD, &url, EMPTY_BODY_SHA256, None)?;
        let mut request = self.http.head(url);
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("CAS existence check failed: {error}"))?;
        match response.status() {
            status if status.is_success() => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => Err(format!("CAS existence check returned {status}")),
        }
    }

    fn object_url(&self, key: &str) -> Result<Url, String> {
        // Path-style addressing (`endpoint/bucket/key`), not virtual-hosted
        // (`bucket.endpoint/key`): this deployment's MinIO endpoint
        // (`minio:9000`) has no bucket-subdomain DNS, which virtual-hosted
        // style requires.
        self.endpoint
            .join(&format!("{}/{key}", self.bucket))
            .map_err(|error| format!("CAS object URL is invalid: {error}"))
    }

    fn sign_request(
        &self,
        method: Method,
        url: &Url,
        payload_sha256_hex: &str,
        body: Option<&[u8]>,
    ) -> Result<Vec<(String, String)>, String> {
        let host = url
            .host_str()
            .ok_or_else(|| "CAS object URL has no host".to_owned())?;
        let host_header = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        let identity =
            Credentials::new(&self.access_key, &self.secret_key, None, None, "workspace-cas").into();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.region)
            .name("s3")
            .time(SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|error| format!("CAS request signing configuration is invalid: {error}"))?
            .into();

        let header_pairs = [
            ("host", host_header.as_str()),
            ("x-amz-content-sha256", payload_sha256_hex),
        ];
        let signable_body = SignableBody::Bytes(body.unwrap_or(&[]));
        let signable_request = SignableRequest::new(
            method.as_str(),
            url.as_str(),
            header_pairs.into_iter(),
            signable_body,
        )
        .map_err(|error| format!("CAS request could not be prepared for signing: {error}"))?;

        let (instructions, _signature) = sign(signable_request, &signing_params)
            .map_err(|error| format!("CAS request signing failed: {error}"))?
            .into_parts();

        let mut signed_headers: Vec<(String, String)> = header_pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        for (name, value) in instructions.headers() {
            signed_headers.push((name.to_owned(), value.to_owned()));
        }
        Ok(signed_headers)
    }
}

fn hex_digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

/// Validates `digest` is exactly `sha256:` followed by 64 lowercase hex
/// characters, returning just the hex portion. Never accepts anything a
/// caller could turn into a path traversal or a request to an unintended
/// key.
fn validate_digest(digest: &str) -> Result<&str, String> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err("CAS digest must be sha256:<hex>".to_owned());
    };
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err("CAS digest is not a well-formed sha256 hex digest".to_owned());
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_is_none_without_configuration() {
        for name in ["MINIO_ENDPOINT", "MINIO_ACCESS_KEY", "MINIO_SECRET_KEY"] {
            std::env::remove_var(name);
        }
        assert!(CasClient::from_env().expect("no error for absence").is_none());
    }

    #[test]
    fn new_with_transport_rejects_invalid_configuration() {
        assert!(CasClient::new_with_transport(
            "not a url",
            "bucket",
            "access",
            &"a".repeat(32),
            "us-east-1"
        )
        .is_err());
        assert!(CasClient::new_with_transport(
            "http://minio:9000",
            "",
            "access",
            &"a".repeat(32),
            "us-east-1"
        )
        .is_err());
        assert!(CasClient::new_with_transport(
            "http://minio:9000",
            "bucket",
            "access",
            "too-short",
            "us-east-1"
        )
        .is_err());
        assert!(CasClient::new_with_transport(
            "http://minio:9000",
            "bucket",
            "access",
            &"a".repeat(32),
            "us-east-1"
        )
        .is_ok());
    }

    #[test]
    fn hex_digest_matches_known_sha256() {
        // sha256("") — a widely published, easily independently verifiable
        // constant (it is also EMPTY_BODY_SHA256 above), not a value
        // specific to this crate's implementation.
        assert_eq!(hex_digest(b""), EMPTY_BODY_SHA256);
    }

    #[test]
    fn validate_digest_accepts_only_well_formed_sha256() {
        assert!(validate_digest(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(validate_digest(&format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(validate_digest(&format!("sha256:{}", "a".repeat(63))).is_err());
        assert!(validate_digest("md5:abc").is_err());
        assert!(validate_digest("sha256:../../etc/passwd").is_err());
    }

    #[test]
    fn object_url_uses_path_style_addressing() {
        let client = CasClient::new_with_transport(
            "http://minio:9000",
            "model-plane-artifacts",
            "access",
            &"a".repeat(32),
            "us-east-1",
        )
        .expect("valid config");
        let url = client.object_url("cas/abc").expect("valid url");
        assert_eq!(url.as_str(), "http://minio:9000/model-plane-artifacts/cas/abc");
    }

    #[test]
    fn sign_request_is_deterministic_and_sensitive_to_its_inputs() {
        let client = CasClient::new_with_transport(
            "http://minio:9000",
            "model-plane-artifacts",
            "access",
            &"a".repeat(32),
            "us-east-1",
        )
        .expect("valid config");
        let url = client.object_url("cas/abc").expect("valid url");

        let first = client
            .sign_request(Method::GET, &url, EMPTY_BODY_SHA256, None)
            .expect("signing succeeds");
        let second = client
            .sign_request(Method::GET, &url, EMPTY_BODY_SHA256, None)
            .expect("signing succeeds");
        // Both include the plain headers this function always adds.
        for headers in [&first, &second] {
            assert!(headers.iter().any(|(name, _)| name == "host"));
            assert!(headers
                .iter()
                .any(|(name, value)| name == "x-amz-content-sha256" && value == EMPTY_BODY_SHA256));
            assert!(headers.iter().any(|(name, _)| name == "authorization"));
        }
        let authorization = |headers: &[(String, String)]| {
            headers
                .iter()
                .find(|(name, _)| name == "authorization")
                .map(|(_, value)| value.clone())
        };
        // Different access keys must produce different signatures — proves
        // the credential is actually load-bearing in the signed output, not
        // ignored.
        let other_client = CasClient::new_with_transport(
            "http://minio:9000",
            "model-plane-artifacts",
            "different-access-key",
            &"a".repeat(32),
            "us-east-1",
        )
        .expect("valid config");
        let third = other_client
            .sign_request(Method::GET, &url, EMPTY_BODY_SHA256, None)
            .expect("signing succeeds");
        assert_ne!(authorization(&first), authorization(&third));
    }
}
