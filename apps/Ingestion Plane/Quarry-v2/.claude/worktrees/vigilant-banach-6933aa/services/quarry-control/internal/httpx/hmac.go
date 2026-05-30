// Package httpx — HMAC verifier middleware for cross-plane requests.
//
// D2 / cluster #14.
//
// Mirrors the Rust signer at `crates/quarry-edge/src/internal_auth.rs`
// byte-for-byte. The canonical string is:
//
//	method + "\n" + path_with_query + "\n" + body_sha256_hex + "\n" +
//	    timestamp_unix_seconds + "\n" + nonce_hex
//
// Signature: HMAC-SHA256(secret, canonical), base64-std encoded,
// prefixed with "sig_v1=" on the wire.
//
// Three required headers on every signed request:
//   - X-Quarry-Sig:        sig_v1=<base64>
//   - X-Quarry-Sig-TS:     unix seconds (rejected if > 300s skew)
//   - X-Quarry-Sig-Nonce:  hex 128-bit random (dedup window 5 min)
//
// When the secret is empty the middleware degrades to "trust the
// network" — signatures are not required but, if present, are still
// verified. This preserves the existing dev posture while wiring the
// production secret rolls forward.

package httpx

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"
)

const (
	HMACHeaderSig   = "X-Quarry-Sig"
	HMACHeaderTS    = "X-Quarry-Sig-TS"
	HMACHeaderNonce = "X-Quarry-Sig-Nonce"

	// SkewTolerance must match Rust SKEW_TOLERANCE_SECS.
	SkewToleranceSecs = 300
	// Body size cap for HMAC verification — protects the server from
	// a malicious huge body that would exhaust memory before the
	// signature check fails. 16 MB is well above any legitimate
	// request body we ship.
	maxBodyForHMAC = 16 << 20
)

// HMACVerifier holds the shared secret plus a small in-memory nonce
// cache to defeat replay inside the skew window.
type HMACVerifier struct {
	secret []byte
	// require is true when missing signatures must be rejected. When
	// false (dev / partial rollout) absent signatures are allowed but
	// present-and-bad signatures still fail.
	require bool
	mu      sync.Mutex
	// nonce -> expiry timestamp. Pruned lazily on every Add.
	seen map[string]time.Time
}

// NewHMACVerifier constructs a verifier from the configured secret.
// Empty secret → returns a verifier that accepts every request
// (logged at WARN by the caller). Use `require=true` once every
// caller is known to be signing.
func NewHMACVerifier(secret string, require bool) *HMACVerifier {
	return &HMACVerifier{
		secret:  []byte(secret),
		require: require,
		seen:    make(map[string]time.Time, 256),
	}
}

// HasSecret returns true when this verifier has a secret configured.
// Used by main() to decide whether to log "HMAC enforced" or "open".
func (v *HMACVerifier) HasSecret() bool {
	return len(v.secret) > 0
}

// Middleware returns the chi-compatible wrapper. Signatures are
// verified before the inner handler runs; failures short-circuit
// with 401 + a typed error body.
func (v *HMACVerifier) Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := v.verify(r); err != nil {
			http.Error(w, "internal auth failed: "+err.Error(), http.StatusUnauthorized)
			return
		}
		next.ServeHTTP(w, r)
	})
}

// verify is the single source-of-truth for HMAC validation. Returns
// nil iff the request is acceptable; the middleware translates errors
// into 401.
func (v *HMACVerifier) verify(r *http.Request) error {
	sigHdr := r.Header.Get(HMACHeaderSig)
	tsHdr := r.Header.Get(HMACHeaderTS)
	nonceHdr := r.Header.Get(HMACHeaderNonce)

	// No headers AND no secret AND not required → trust-the-network
	// posture. Used during the rollout window.
	if sigHdr == "" && tsHdr == "" && nonceHdr == "" {
		if v.require {
			return errors.New("missing HMAC headers")
		}
		return nil
	}
	// Any partial header set is a misconfiguration.
	if sigHdr == "" || tsHdr == "" || nonceHdr == "" {
		return errors.New("incomplete HMAC header triple")
	}
	if !v.HasSecret() {
		// Caller sent a signature but we have no secret — refuse to
		// pretend we validated.
		return errors.New("HMAC headers present but no secret configured")
	}

	// Timestamp skew check.
	ts, err := strconv.ParseInt(tsHdr, 10, 64)
	if err != nil {
		return errors.New("HMAC timestamp not an integer")
	}
	now := time.Now().Unix()
	if abs(now-ts) > SkewToleranceSecs {
		return errors.New("HMAC timestamp outside skew tolerance")
	}

	// Nonce dedup — bounded by GC pass that drops expired entries
	// every call. O(N) over the map; N is small because the skew
	// window is 5 min.
	v.mu.Lock()
	now2 := time.Now()
	for n, exp := range v.seen {
		if exp.Before(now2) {
			delete(v.seen, n)
		}
	}
	if _, dup := v.seen[nonceHdr]; dup {
		v.mu.Unlock()
		return errors.New("HMAC nonce already seen (replay)")
	}
	v.seen[nonceHdr] = now2.Add(SkewToleranceSecs * time.Second)
	v.mu.Unlock()

	// Parse signature header.
	if !strings.HasPrefix(sigHdr, "sig_v1=") {
		return errors.New("HMAC signature missing sig_v1= prefix")
	}
	sigB64 := strings.TrimPrefix(sigHdr, "sig_v1=")
	sigBytes, err := base64.StdEncoding.DecodeString(sigB64)
	if err != nil {
		return errors.New("HMAC signature base64 decode failed")
	}

	// Read + retain the body so the downstream handler still gets it.
	body, err := readAndReplaceBody(r)
	if err != nil {
		return err
	}
	bodyHash := sha256.Sum256(body)
	bodyHashHex := hex.EncodeToString(bodyHash[:])

	// Path-with-query — chi rewrites r.URL, but the raw path the
	// signer used is `r.URL.Path + "?" + r.URL.RawQuery`. We
	// reconstruct that shape exactly so the signature lines up.
	pathQ := r.URL.Path
	if r.URL.RawQuery != "" {
		pathQ += "?" + r.URL.RawQuery
	}

	canonical := r.Method + "\n" + pathQ + "\n" + bodyHashHex + "\n" +
		strconv.FormatInt(ts, 10) + "\n" + nonceHdr

	mac := hmac.New(sha256.New, v.secret)
	_, _ = mac.Write([]byte(canonical))
	expected := mac.Sum(nil)
	if !hmac.Equal(expected, sigBytes) {
		return errors.New("HMAC signature mismatch")
	}
	return nil
}

// readAndReplaceBody drains r.Body, hashes it, then restores a
// fresh io.ReadCloser on r.Body so the downstream handler sees the
// same bytes. Bodies above maxBodyForHMAC are rejected to bound
// memory.
func readAndReplaceBody(r *http.Request) ([]byte, error) {
	if r.Body == nil {
		return nil, nil
	}
	defer r.Body.Close()
	limited := io.LimitReader(r.Body, maxBodyForHMAC+1)
	body, err := io.ReadAll(limited)
	if err != nil {
		return nil, errors.New("HMAC body read failed: " + err.Error())
	}
	if len(body) > maxBodyForHMAC {
		return nil, errors.New("HMAC body exceeds 16 MB cap")
	}
	r.Body = io.NopCloser(bytes.NewReader(body))
	return body, nil
}

func abs(x int64) int64 {
	if x < 0 {
		return -x
	}
	return x
}
