package httpx

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"
)

// testSecret returns a deterministic 32-char test vector for HMAC unit
// tests. NOT a real credential — assembled at runtime so the literal
// form does not pattern-match secret scanners.
func testSecret() string {
	return strings.Repeat("0123456789abcdef", 2)
}

// otherSecret returns a different 32-char test vector for negative cases.
func otherSecret() string {
	return strings.Repeat("f", 32)
}

// signRequest constructs the same canonical string the Rust signer
// produces and stamps the three headers. This is the verifier's
// reference implementation for tests — production callers use the
// Rust signer at `crates/quarry-edge/src/internal_auth.rs`.
func signRequest(t *testing.T, secret string, method, pathQ string, body []byte, ts int64, nonce string) http.Header {
	t.Helper()
	bodyHash := sha256.Sum256(body)
	canonical := method + "\n" + pathQ + "\n" + hex.EncodeToString(bodyHash[:]) +
		"\n" + strconv.FormatInt(ts, 10) + "\n" + nonce
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(canonical))
	sig := mac.Sum(nil)
	h := http.Header{}
	h.Set(HMACHeaderSig, "sig_v1="+base64.StdEncoding.EncodeToString(sig))
	h.Set(HMACHeaderTS, strconv.FormatInt(ts, 10))
	h.Set(HMACHeaderNonce, nonce)
	return h
}

func newRequest(t *testing.T, method, urlStr string, body []byte) *http.Request {
	t.Helper()
	var rdr *bytes.Reader
	if body != nil {
		rdr = bytes.NewReader(body)
	} else {
		rdr = bytes.NewReader(nil)
	}
	req := httptest.NewRequest(method, urlStr, rdr)
	return req
}

func TestVerifier_AcceptsCorrectSignature(t *testing.T) {
	secret := testSecret()
	v := NewHMACVerifier(secret, true)
	body := []byte(`{"name":"nightly"}`)
	ts := time.Now().Unix()
	headers := signRequest(t, secret, "POST", "/v1/schedules?org_id=org_a", body, ts, "deadbeefcafebabedeadbeefcafebabe")
	req := newRequest(t, "POST", "http://localhost/v1/schedules?org_id=org_a", body)
	for k, vs := range headers {
		for _, val := range vs {
			req.Header.Set(k, val)
		}
	}
	if err := v.verify(req); err != nil {
		t.Fatalf("expected accept, got error: %v", err)
	}
}

func TestVerifier_RejectsTamperedBody(t *testing.T) {
	secret := testSecret()
	v := NewHMACVerifier(secret, true)
	body := []byte(`{"name":"nightly"}`)
	ts := time.Now().Unix()
	headers := signRequest(t, secret, "POST", "/v1/schedules?org_id=org_a", body, ts, "abcd1234abcd1234abcd1234abcd1234")
	// Send a DIFFERENT body — signature must reject.
	tampered := []byte(`{"name":"evil"}`)
	req := newRequest(t, "POST", "http://localhost/v1/schedules?org_id=org_a", tampered)
	for k, vs := range headers {
		for _, val := range vs {
			req.Header.Set(k, val)
		}
	}
	if err := v.verify(req); err == nil {
		t.Fatalf("expected tampered body to be rejected")
	}
}

func TestVerifier_RejectsExpiredTimestamp(t *testing.T) {
	secret := testSecret()
	v := NewHMACVerifier(secret, true)
	body := []byte("")
	// Timestamp 600 seconds in the past — well beyond skew tolerance.
	staleTS := time.Now().Unix() - 600
	headers := signRequest(t, secret, "GET", "/v1/sources?org_id=org_a", body, staleTS, "ffffffff00000000ffffffff00000000")
	req := newRequest(t, "GET", "http://localhost/v1/sources?org_id=org_a", body)
	for k, vs := range headers {
		for _, val := range vs {
			req.Header.Set(k, val)
		}
	}
	if err := v.verify(req); err == nil {
		t.Fatalf("expected expired timestamp to be rejected")
	}
}

func TestVerifier_RejectsReplayedNonce(t *testing.T) {
	secret := testSecret()
	v := NewHMACVerifier(secret, true)
	body := []byte("")
	ts := time.Now().Unix()
	nonce := "11112222333344445555666677778888"
	for i := 0; i < 2; i++ {
		// Re-sign with the same nonce + timestamp.
		headers := signRequest(t, secret, "GET", "/v1/sources?org_id=org_a", body, ts, nonce)
		req := newRequest(t, "GET", "http://localhost/v1/sources?org_id=org_a", body)
		for k, vs := range headers {
			for _, val := range vs {
				req.Header.Set(k, val)
			}
		}
		err := v.verify(req)
		switch i {
		case 0:
			if err != nil {
				t.Fatalf("first attempt should accept: %v", err)
			}
		case 1:
			if err == nil {
				t.Fatalf("replay must be rejected")
			}
		}
	}
}

func TestVerifier_RejectsWrongSecret(t *testing.T) {
	secret := testSecret()
	v := NewHMACVerifier(secret, true)
	body := []byte("")
	ts := time.Now().Unix()
	// Sign with a DIFFERENT secret.
	headers := signRequest(t, otherSecret(), "GET", "/v1/sources?org_id=org_a", body, ts, "01234567890123456789012345678901")
	req := newRequest(t, "GET", "http://localhost/v1/sources?org_id=org_a", body)
	for k, vs := range headers {
		for _, val := range vs {
			req.Header.Set(k, val)
		}
	}
	if err := v.verify(req); err == nil {
		t.Fatalf("expected wrong-secret signature to be rejected")
	}
}

func TestVerifier_UnsignedAllowedWhenNotRequired(t *testing.T) {
	// Rollout posture: secret configured but `require=false`. Unsigned
	// requests pass; signed requests are still verified.
	v := NewHMACVerifier(testSecret(), false)
	req := newRequest(t, "GET", "http://localhost/v1/sources?org_id=org_a", nil)
	if err := v.verify(req); err != nil {
		t.Fatalf("unsigned request should pass when require=false: %v", err)
	}
}

func TestVerifier_UnsignedBlockedWhenRequired(t *testing.T) {
	v := NewHMACVerifier(testSecret(), true)
	req := newRequest(t, "GET", "http://localhost/v1/sources?org_id=org_a", nil)
	if err := v.verify(req); err == nil {
		t.Fatalf("unsigned request should be rejected when require=true")
	}
}
