package delegation

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"testing"
)

// The same vector is asserted by convex-core/test/space-attention-courier.test.cjs
// (WS11-S9): the Convex signer must produce what this verifier computes.
func TestSpaceAttentionCourierSignatureVector(t *testing.T) {
	secret := []byte("sssssssssssssssssssssssssssssssssssssssssss")
	body := []byte(`{"organization_id":"org_ext","type":"space.mention"}`)
	nonceBytes := make([]byte, 18)
	for i := range nonceBytes {
		nonceBytes[i] = byte(i)
	}
	nonce := base64.RawURLEncoding.EncodeToString(nonceBytes)
	digestBytes := sha256.Sum256(body)
	digest := base64.RawURLEncoding.EncodeToString(digestBytes[:])
	canonical := Canonical(CanonicalFields{
		ServiceID:      "space-attention",
		Audience:       "notification-core",
		Timestamp:      "2026-11-02T10:00:00Z",
		Nonce:          nonce,
		Method:         "POST",
		URI:            "/api/v1/notification-requests",
		OrganizationID: "org_ext",
		BodySHA256:     digest,
	})
	mac := hmac.New(sha256.New, secret)
	_, _ = mac.Write([]byte(canonical))
	signature := base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
	if nonce != "AAECAwQFBgcICQoLDA0ODxAR" {
		t.Fatalf("nonce = %s", nonce)
	}
	if digest != "hvOdQlMVNi0BY1TkhzQeR_EBUrdga-YnlQsmeIty8RU" {
		t.Fatalf("digest = %s", digest)
	}
	if want := "0EZpEyYo_37rN8-kL155WG9K1aJOtk7MtfszOF-YCro"; signature != want {
		t.Fatalf("signature = %s, want %s", signature, want)
	}
}
