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

use hmac::{Hmac, Mac};
use sha2::Sha256;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

type HmacSha256 = Hmac<Sha256>;

/// Wall-clock skew the verifier tolerates either side of the request
/// timestamp. 5 minutes matches the JWT leeway we use elsewhere.
#[allow(dead_code)] // scaffolding: wired in follow-up
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

    /// Fresh nonce, 128 bits of OS-provided CSPRNG entropy expressed
    /// as 32 hex chars. The verifier dedupes inside the skew window;
    /// a predictable nonce stream lets an on-path attacker pre-block
    /// upcoming nonces or pattern-match into the dedupe map.
    ///
    /// On any unrecoverable RNG failure (RNG broken at the OS level
    /// — process should not be issuing signed requests anyway) we
    /// panic rather than silently returning weak entropy.
    pub fn fresh_nonce() -> String {
        let mut bytes = [0u8; 16];
        getrandom::getrandom(&mut bytes).expect("OS CSPRNG must be available for fresh_nonce");
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

    /// Verify the `X-Quarry-Run-Sig` org-binding the orchestrator stamps
    /// on `/v1/internal/run_page`. The runtime bearer token authenticates
    /// the *caller*; this HMAC binds the *tenant identity* so a leaked
    /// bearer can't be replayed against an arbitrary `org_id`.
    ///
    /// Canonical string is `org_id + "\n" + url` — the body's `org_id`, a
    /// single `\n`, then the body's raw `url` (BEFORE URL-parsing). This
    /// MUST match the Go signer (`runBindingSig`) byte-for-byte. Signature
    /// is lowercase-hex of HMAC-SHA256(secret, canonical), compared in
    /// constant time so a near-miss can't be timing-oracled into a forgery.
    pub fn verify_run_binding(&self, org_id: &str, url: &str, provided_hex: &str) -> bool {
        self.verify_run_binding_with(org_id, url, &RunBindingExtras::default(), provided_hex)
    }

    /// [`InternalSigner::verify_run_binding`] over the canonical string with the page's extras
    /// appended (WS7.G 6b: the policy snapshot). With no extras this is byte-for-byte the
    /// two-field string, so an orchestrator that predates the extras still verifies.
    pub fn verify_run_binding_with(
        &self,
        org_id: &str,
        url: &str,
        extras: &RunBindingExtras,
        provided_hex: &str,
    ) -> bool {
        let Some(expected) = self.run_binding_sig(org_id, url, extras) else {
            return false;
        };
        crate::change_routes::ct_eq(expected.as_bytes(), provided_hex.as_bytes())
    }

    /// The lowercase-hex run-binding signature, or `None` when org_id or url carries a `\n`.
    ///
    /// Field-confusion guard: a `\n` inside org_id (or url) would shift the canonical
    /// boundary so (org="a", url="\nX") forges the same HMAC as (org="a\nX", url="…"). Org
    /// ids + URLs are newline-free in practice; reject any that aren't rather than hash an
    /// ambiguous string.
    pub fn run_binding_sig(
        &self,
        org_id: &str,
        url: &str,
        extras: &RunBindingExtras,
    ) -> Option<String> {
        if org_id.contains('\n') || url.contains('\n') {
            return None;
        }
        let mut canonical = format!("{org_id}\n{url}");
        extras.append_to(&mut canonical);
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
            .expect("HMAC accepts any key length");
        mac.update(canonical.as_bytes());
        // Lowercase-hex — same style as `body_digest`'s `{:x}`.
        Some(format!("{:x}", mac.finalize().into_bytes()))
    }
}

/// What the run-binding string covers beyond `org_id` and `url` (WS7.G 6b).
///
/// Each present extra adds one line `\n<tag>=<lowercase hex sha256>` after the url, in the
/// fixed order of the fields below. An absent extra adds nothing, so a request without extras
/// signs exactly the two-field string. The Go signer (`runBindingSigExtras`) mirrors this
/// byte-for-byte; the two sides are pinned by the golden constants in the tests on both.
///
/// WS14.S7a adds `robots` (the sha256 of the canonical `crawl.robots` object) as the next
/// field, after `policy`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunBindingExtras {
    /// `sha256(canonical_json(policy_snapshot))` in lowercase hex.
    pub policy_snapshot_sha256: Option<String>,
}

impl RunBindingExtras {
    /// The extras of a page whose body carried `snapshot` (`None` or JSON null: none).
    pub fn for_snapshot(snapshot: Option<&serde_json::Value>) -> Self {
        Self {
            policy_snapshot_sha256: snapshot
                .filter(|value| !value.is_null())
                .map(snapshot_sha256_hex),
        }
    }

    fn append_to(&self, canonical: &mut String) {
        if let Some(hash) = &self.policy_snapshot_sha256 {
            canonical.push_str("\npolicy=");
            canonical.push_str(hash);
        }
    }
}

/// The canonical JSON the policy snapshot is hashed in: object keys sorted by byte order at
/// every level, no whitespace, arrays in order, strings JSON-escaped. The Go side
/// (`canonicalPolicySnapshot`) writes the same bytes. The snapshot only ever holds integers,
/// ASCII host names and closed labels, so no number or escape form can differ between the two.
pub fn canonical_json(value: &serde_json::Value) -> String {
    use serde_json::Value;
    let mut out = String::new();
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).expect("a string key serialises"));
                    out.push(':');
                    write(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).expect("a scalar serialises")),
        }
    }
    write(value, &mut out);
    out
}

/// Lowercase-hex sha256 of [`canonical_json`].
pub fn snapshot_sha256_hex(value: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(canonical_json(value).as_bytes()))
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

    /// run_page org-binding round-trip. A correct sig over
    /// `org_id\nurl` verifies; tampering with org, url, or the sig
    /// itself fails. This is the contract the Go `runBindingSig` signer
    /// must mirror byte-for-byte.
    #[test]
    fn verify_run_binding_accepts_correct_and_rejects_tampered() {
        let s = signer();
        let org = "org_a";
        let url = "https://example.com/pricing";

        // Compute the honest signature the way the Go signer does:
        // lowercase-hex HMAC-SHA256 over `org_id\nurl`.
        let good = {
            let mut mac = HmacSha256::new_from_slice(s.secret.as_bytes()).unwrap();
            mac.update(format!("{org}\n{url}").as_bytes());
            format!("{:x}", mac.finalize().into_bytes())
        };

        assert!(
            s.verify_run_binding(org, url, &good),
            "honest org-binding signature MUST verify"
        );

        // Tampered org → mismatch.
        assert!(
            !s.verify_run_binding("org_b", url, &good),
            "swapping org_id MUST fail (the whole point of the binding)"
        );
        // Tampered url → mismatch.
        assert!(
            !s.verify_run_binding(org, "https://evil.example.com", &good),
            "swapping url MUST fail"
        );
        // Tampered signature → mismatch.
        assert!(
            !s.verify_run_binding(org, url, "deadbeef"),
            "a forged signature MUST fail"
        );
        // Empty signature (header absent) → mismatch.
        assert!(
            !s.verify_run_binding(org, url, ""),
            "a missing signature MUST fail"
        );
    }

    /// The golden snapshot both sides pin (the Go test `TestPolicySnapshotGolden` holds the same
    /// document and the same two constants). Key order and whitespace are deliberately not the
    /// canonical ones.
    const GOLDEN_SNAPSHOT: &str = r#"{
        "v": 1,
        "proxy_max": "none",
        "robots_max": "respect",
        "modes": {"search.paid_provider": "disabled", "browser.act.input": "disabled"},
        "deny": ["bad.example", "evil.test"],
        "allow": [["docs.example.org", "example.com"], ["example.com"]],
        "org_rev": 7
    }"#;
    const GOLDEN_CANONICAL: &str = r#"{"allow":[["docs.example.org","example.com"],["example.com"]],"deny":["bad.example","evil.test"],"modes":{"browser.act.input":"disabled","search.paid_provider":"disabled"},"org_rev":7,"proxy_max":"none","robots_max":"respect","v":1}"#;
    const GOLDEN_SHA256: &str = "eac471399cd4fdd009a071ed0ce1605c547b4c26b1a77adb140b1a913b78782c";
    const GOLDEN_SIG: &str = "e85e8d3b76888dbce0f63b66035a3fb7d79280d2ec04f86017852256295e888a";

    #[test]
    fn canonical_json_sorts_keys_and_drops_whitespace() {
        let value: serde_json::Value = serde_json::from_str(GOLDEN_SNAPSHOT).unwrap();
        assert_eq!(canonical_json(&value), GOLDEN_CANONICAL);
        // Arrays keep their order; only object keys are sorted.
        let swapped: serde_json::Value =
            serde_json::from_str(r#"{"b":[2,1],"a":{"d":null,"c":"x\"y"}}"#).unwrap();
        assert_eq!(canonical_json(&swapped), r#"{"a":{"c":"x\"y","d":null},"b":[2,1]}"#);
    }

    #[test]
    fn policy_snapshot_golden_matches_the_go_signer() {
        let value: serde_json::Value = serde_json::from_str(GOLDEN_SNAPSHOT).unwrap();
        assert_eq!(snapshot_sha256_hex(&value), GOLDEN_SHA256);
        let s = signer();
        let extras = RunBindingExtras::for_snapshot(Some(&value));
        let sig = s
            .run_binding_sig("org_a", "https://example.com/pricing", &extras)
            .expect("no newline in the fields");
        assert_eq!(sig, GOLDEN_SIG);
        assert!(s.verify_run_binding_with("org_a", "https://example.com/pricing", &extras, GOLDEN_SIG));
    }

    #[test]
    fn run_binding_without_extras_is_the_two_field_string() {
        let s = signer();
        let with_none = s
            .run_binding_sig("org_a", "https://example.com/", &RunBindingExtras::default())
            .unwrap();
        let mut mac = HmacSha256::new_from_slice(s.secret.as_bytes()).unwrap();
        mac.update(b"org_a\nhttps://example.com/");
        assert_eq!(with_none, format!("{:x}", mac.finalize().into_bytes()));
        // JSON null counts as no snapshot.
        assert_eq!(
            RunBindingExtras::for_snapshot(Some(&serde_json::Value::Null)),
            RunBindingExtras::default()
        );
        assert_eq!(RunBindingExtras::for_snapshot(None), RunBindingExtras::default());
    }

    #[test]
    fn a_snapshot_cannot_be_added_swapped_or_stripped_without_the_secret() {
        let s = signer();
        let (org, url) = ("org_a", "https://example.com/p");
        let snapshot: serde_json::Value = serde_json::from_str(GOLDEN_SNAPSHOT).unwrap();
        let with = RunBindingExtras::for_snapshot(Some(&snapshot));
        let none = RunBindingExtras::default();
        let signed_with = s.run_binding_sig(org, url, &with).unwrap();
        let signed_without = s.run_binding_sig(org, url, &none).unwrap();
        assert_ne!(signed_with, signed_without);
        // Stripped: the body no longer carries the snapshot the signature covered.
        assert!(!s.verify_run_binding_with(org, url, &none, &signed_with));
        // Added: a body that gained a snapshot under a signature made without one.
        assert!(!s.verify_run_binding_with(org, url, &with, &signed_without));
        // Swapped: one deny host removed.
        let mut loosened = snapshot.clone();
        loosened["deny"] = serde_json::json!(["bad.example"]);
        let swapped = RunBindingExtras::for_snapshot(Some(&loosened));
        assert!(!s.verify_run_binding_with(org, url, &swapped, &signed_with));
        // The plain two-field check does not accept the extended signature either.
        assert!(!s.verify_run_binding(org, url, &signed_with));
    }
}
