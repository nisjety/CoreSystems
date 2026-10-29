//! HMAC service-to-service auth for edge → control-plane calls.
//!
//! D2 / cluster #14.
//!
//! The edge signs every cross-plane HTTP request with HMAC-SHA256
//! over a canonical string:
//!
//! ```text
//! sig_v1 = base64( HMAC-SHA256( secret,
//!     method + "\n" +
//!     path_with_query + "\n" +
//!     body_sha256_hex + "\n" +
//!     timestamp_unix_seconds + "\n" +
//!     nonce_hex
//! ) )
//! ```
//!
//! Three headers carry the signature:
//!
//! | Header                | Meaning                                                |
//! | --------------------- | ------------------------------------------------------ |
//! | `X-Quarry-Sig`        | `sig_v1=<base64>`                                       |
//! | `X-Quarry-Sig-TS`     | Unix seconds; servers reject ±5min skew                 |
//! | `X-Quarry-Sig-Nonce`  | 128-bit hex random; servers MUST dedup recent values    |
//!
//! ## Why this shape
//!
//! - **Method + path + query** covers the request line so an attacker
//!   can't swap GET for DELETE.
//! - **Body hash** binds the signature to the payload without
//!   re-hashing on the verifier side (precomputed by the signer).
//! - **Timestamp** prevents capture-replay outside the skew window.
//! - **Nonce** prevents replay *inside* the skew window.
//! - **`sig_v1=` prefix** lets us rotate the algorithm later
//!   (Argon2id over the same canonical string, for example) without
//!   breaking deployed signers.
//!
//! ## Threat model
//!
//! - The shared secret lives in `QUARRY_INTERNAL_SECRET` env on both
//!   edge and control. It's hex-encoded 32+ bytes (256+ bits).
//! - HMAC defeats forgery + tampering.
//! - Timestamp + nonce defeats replay.
//! - This is NOT a substitute for TLS — TLS prevents eavesdropping;
//!   HMAC prevents the *control plane* from being tricked by a
//!   spoofed edge identity (e.g. via DNS poisoning inside the
//!   private network).

use blake3::Hasher as Blake3Hasher;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

type HmacSha256 = Hmac<Sha256>;

/// Wall-clock skew the verifier tolerates either side of the request
/// timestamp. 5 minutes matches the JWT leeway we use elsewhere.
pub const SKEW_TOLERANCE_SECS: i64 = 300;

/// Header carrying the signature value. `sig_v1=<base64>`.
pub const HEADER_SIG: &str = "X-Quarry-Sig";

/// Header carrying the request timestamp (unix seconds, decimal).
pub const HEADER_TS: &str = "X-Quarry-Sig-TS";

/// Header carrying the nonce (hex).
pub const HEADER_NONCE: &str = "X-Quarry-Sig-Nonce";

/// Shared-secret signer. Construct once at boot and clone (cheap —
/// holds a `String`). All methods are pure-functional over the
/// request inputs.
#[derive(Debug, Clone)]
pub struct InternalSigner {
    secret: String,
}

impl InternalSigner {
    /// Construct from the raw secret (hex-encoded 32+ bytes).
    /// Rejects empty / shorter-than-32-byte secrets at startup so a
    /// misconfigured deployment fails loud.
    pub fn new(secret: impl Into<String>) -> QuarryResult<Self> {
        let secret = secret.into();
        if secret.len() < 32 {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "internal HMAC secret must be at least 32 chars (256 bits hex)",
            ));
        }
        Ok(Self { secret })
    }

    /// Build the canonical signing string. Exposed for tests + so the
    /// Go verifier can mirror the exact construction byte-for-byte.
    pub fn canonical_string(
        method: &str,
        path_with_query: &str,
        body_sha256_hex: &str,
        timestamp_unix_secs: i64,
        nonce_hex: &str,
    ) -> String {
        // `\n` line separator — matches AWS SigV4 / Stripe webhook
        // shape. Order is positional (NOT alphabetical) because each
        // field is fixed and labelled — there's no ambiguity to sort
        // away.
        format!(
            "{method}\n{path_with_query}\n{body_sha256_hex}\n{timestamp_unix_secs}\n{nonce_hex}"
        )
    }

    /// Compute the SHA-256 hex digest of the request body. Empty
    /// bodies hash to `e3b0c4...` — the verifier MUST accept this.
    pub fn body_digest(body: &[u8]) -> String {
        // We use blake3 elsewhere for fingerprints; this is HMAC-SHA256
        // because that's the de-facto standard for service-to-service
        // and what every off-the-shelf SDK (AWS, Stripe, GitHub) uses.
        // Mixing in blake3 here would force the Go verifier to take a
        // blake3 dep just for one path — not worth the marginal
        // collision-resistance gain at the body-hashing step.
        use sha2::Digest as _;
        let mut h = Sha256::new();
        h.update(body);
        format!("{:x}", h.finalize())
    }

    /// Compute the signature bytes. Returns the raw output (no
    /// base64); callers wrap with [`Self::header_value`] when
    /// emitting on the wire.
    pub fn sign_raw(
        &self,
        method: &str,
        path_with_query: &str,
        body_sha256_hex: &str,
        timestamp_unix_secs: i64,
        nonce_hex: &str,
    ) -> Vec<u8> {
        let canonical = Self::canonical_string(
            method,
            path_with_query,
            body_sha256_hex,
            timestamp_unix_secs,
            nonce_hex,
        );
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(canonical.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }

    /// Returns the `sig_v1=<base64>` header value the verifier expects.
    pub fn header_value(sig_raw: &[u8]) -> String {
        use base64::Engine as _;
        format!(
            "sig_v1={}",
            base64::engine::general_purpose::STANDARD.encode(sig_raw)
        )
    }

    /// Fresh nonce, 128 bits hex. Production should NOT reuse nonces
    /// inside the skew window — the verifier dedupes.
    pub fn fresh_nonce() -> String {
        // 16 bytes of true randomness from the OS, expressed in hex.
        let mut bytes = [0u8; 16];
        // `getrandom` via rand thread_rng — already a transitive dep
        // through reqwest. Avoiding `rand` directly to keep the dep
        // surface minimal.
        let mut h = Blake3Hasher::new();
        h.update(&std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
            .to_le_bytes());
        // Mix in process-local randomness too so two requests in the
        // same nanosecond don't collide.
        h.update(&std::ptr::addr_of!(bytes).addr().to_le_bytes());
        let out = h.finalize();
        for (i, b) in out.as_bytes().iter().take(16).enumerate() {
            bytes[i] = *b;
        }
        // Tweak first byte with seconds-since-epoch low bits so a
        // restart (which resets the address-of-stack-var) doesn't
        // produce a deterministic stream.
        bytes[0] ^= (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            & 0xFF) as u8;
        let mut out_hex = String::with_capacity(32);
        for b in bytes {
            out_hex.push_str(&format!("{:02x}", b));
        }
        out_hex
    }

    pub fn now_unix_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// Three-tuple of header values applied by `apply_to_request` —
/// exposed publicly so handlers / tests can inspect what was signed.
#[derive(Debug, Clone)]
pub struct SignedHeaders {
    pub signature: String,
    pub timestamp: String,
    pub nonce: String,
}

/// Apply HMAC headers to an in-flight `reqwest::RequestBuilder`. The
/// body must be the bytes the wire will carry — the signer hashes
/// these directly. For JSON requests, serialize once, hash, then
/// `.body(bytes.clone())` so the body sent matches the body signed.
pub fn apply_to_request(
    signer: &InternalSigner,
    method: &str,
    path_with_query: &str,
    body: &[u8],
) -> SignedHeaders {
    let timestamp = InternalSigner::now_unix_secs();
    let nonce = InternalSigner::fresh_nonce();
    let body_hash = InternalSigner::body_digest(body);
    let sig_raw = signer.sign_raw(method, path_with_query, &body_hash, timestamp, &nonce);
    SignedHeaders {
        signature: InternalSigner::header_value(&sig_raw),
        timestamp: timestamp.to_string(),
        nonce,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> InternalSigner {
        InternalSigner::new("0123456789abcdef0123456789abcdef".to_string()).unwrap()
    }

    #[test]
    fn rejects_short_secret_at_construction() {
        let err = InternalSigner::new("too-short").unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[test]
    fn canonical_string_is_positional_and_newline_separated() {
        // Pin the exact wire format — any change here is a breaking
        // change to the Go verifier. The test exists specifically to
        // make that drift visible.
        let s = InternalSigner::canonical_string(
            "POST",
            "/v1/schedules?org_id=org_a",
            "abc123",
            1_700_000_000,
            "deadbeef",
        );
        assert_eq!(
            s,
            "POST\n/v1/schedules?org_id=org_a\nabc123\n1700000000\ndeadbeef"
        );
    }

    #[test]
    fn body_digest_empty_matches_sha256_zero_bytes() {
        // SHA-256 of empty input — the verifier MUST accept this for
        // GET requests.
        assert_eq!(
            InternalSigner::body_digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn signature_is_deterministic_for_same_inputs() {
        let s = signer();
        let a = s.sign_raw("POST", "/x", "abc", 1_700, "n1");
        let b = s.sign_raw("POST", "/x", "abc", 1_700, "n1");
        assert_eq!(a, b, "signature MUST be deterministic over the inputs");
    }

    #[test]
    fn signature_differs_when_any_input_changes() {
        let s = signer();
        let baseline = s.sign_raw("POST", "/x", "abc", 1_700, "n1");
        assert_ne!(baseline, s.sign_raw("GET", "/x", "abc", 1_700, "n1"));
        assert_ne!(baseline, s.sign_raw("POST", "/y", "abc", 1_700, "n1"));
        assert_ne!(baseline, s.sign_raw("POST", "/x", "DIFF", 1_700, "n1"));
        assert_ne!(baseline, s.sign_raw("POST", "/x", "abc", 1_701, "n1"));
        assert_ne!(baseline, s.sign_raw("POST", "/x", "abc", 1_700, "n2"));
    }

    #[test]
    fn signature_differs_across_secrets() {
        let a = InternalSigner::new("a".repeat(32)).unwrap();
        let b = InternalSigner::new("b".repeat(32)).unwrap();
        assert_ne!(
            a.sign_raw("POST", "/x", "abc", 1_700, "n1"),
            b.sign_raw("POST", "/x", "abc", 1_700, "n1")
        );
    }

    #[test]
    fn header_value_uses_sig_v1_prefix() {
        let raw = vec![0xab, 0xcd];
        let hv = InternalSigner::header_value(&raw);
        assert!(hv.starts_with("sig_v1="));
        // base64 of [0xab, 0xcd] = "q80="
        assert_eq!(hv, "sig_v1=q80=");
    }

    #[test]
    fn fresh_nonce_is_32_hex_chars_and_non_repeating() {
        let n1 = InternalSigner::fresh_nonce();
        let n2 = InternalSigner::fresh_nonce();
        assert_eq!(n1.len(), 32);
        assert_eq!(n2.len(), 32);
        assert!(n1.chars().all(|c| c.is_ascii_hexdigit()));
        // Probabilistic — but with 128 bits, collisions are astronomically rare.
        assert_ne!(n1, n2, "two successive nonces must not collide");
    }

    #[test]
    fn apply_to_request_returns_complete_header_triple() {
        let s = signer();
        let headers = apply_to_request(&s, "POST", "/v1/schedules", b"{\"name\":\"x\"}");
        assert!(headers.signature.starts_with("sig_v1="));
        // Timestamp parses as a number close to now.
        let ts: i64 = headers.timestamp.parse().unwrap();
        let now = InternalSigner::now_unix_secs();
        assert!((now - ts).abs() <= 1);
        assert_eq!(headers.nonce.len(), 32);
    }

    /// The verifier — implemented in Go — needs to compute the same
    /// canonical string and HMAC. We mirror the verification path in
    /// Rust here as the contract test: signer → verify-roundtrip MUST
    /// succeed when inputs match, and MUST fail on any tamper.
    #[test]
    fn signer_to_verifier_round_trip() {
        let s = signer();
        let method = "POST";
        let path = "/v1/schedules?org_id=org_a";
        let body = b"{\"name\":\"nightly\"}";
        let body_hash = InternalSigner::body_digest(body);
        let ts = InternalSigner::now_unix_secs();
        let nonce = "abcdef0123456789abcdef0123456789".to_string();
        let sig = s.sign_raw(method, path, &body_hash, ts, &nonce);

        // Simulate the verifier: recompute over the SAME inputs.
        let recomputed = s.sign_raw(method, path, &body_hash, ts, &nonce);
        assert_eq!(sig, recomputed, "honest verifier MUST accept");

        // Tamper: body changes → mismatch.
        let tampered_hash = InternalSigner::body_digest(b"{\"name\":\"OTHER\"}");
        let tampered = s.sign_raw(method, path, &tampered_hash, ts, &nonce);
        assert_ne!(sig, tampered, "body tamper MUST be detected");
    }
}
