package attestation

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"
)

const testNowUnix = int64(1783944000)

func TestVerifierAcceptsExactlyBoundEd25519Attestation(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	keys, err := ParseTrustedKeysJSON(trustedKeysJSON(t, publicKey))
	if err != nil {
		t.Fatalf("ParseTrustedKeysJSON error: %v", err)
	}
	verifier := NewVerifier(keys, func() time.Time { return time.Unix(testNowUnix, 0).UTC() })
	binding := testBinding(t)
	claims := testClaims(binding)
	token := signCompactJWS(t, privateKey, testHeader(), claims)

	verified, err := verifier.Verify(token, binding)
	if err != nil {
		t.Fatalf("Verify error: %v", err)
	}
	if verified.Issuer != claims.Issuer || verified.KeyID != "conversation-write-2026-07" || verified.AuthorizationID != claims.AuthorizationID {
		t.Fatalf("verified attestation = %#v, want trusted identifiers", verified)
	}
}

func TestVerifierRejectsInvalidOrMismatchedAttestations(t *testing.T) {
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	otherPublicKey, otherPrivateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey(other) error: %v", err)
	}
	keys, err := ParseTrustedKeysJSON(trustedKeysJSON(t, publicKey))
	if err != nil {
		t.Fatalf("ParseTrustedKeysJSON error: %v", err)
	}
	verifier := NewVerifier(keys, func() time.Time { return time.Unix(testNowUnix, 0).UTC() })
	binding := testBinding(t)
	baseClaims := testClaims(binding)

	tests := []struct {
		name      string
		header    Header
		claims    Claims
		binding   Binding
		private   ed25519.PrivateKey
		transform func(string) string
	}{
		{name: "wrong alg", header: Header{Algorithm: "HS256", Type: AttestationType, KeyID: "conversation-write-2026-07"}, claims: baseClaims, binding: binding, private: privateKey},
		{name: "wrong type", header: Header{Algorithm: "EdDSA", Type: "JWT", KeyID: "conversation-write-2026-07"}, claims: baseClaims, binding: binding, private: privateKey},
		{name: "empty kid", header: Header{Algorithm: "EdDSA", Type: AttestationType}, claims: baseClaims, binding: binding, private: privateKey},
		{name: "unknown kid", header: Header{Algorithm: "EdDSA", Type: AttestationType, KeyID: "unknown"}, claims: baseClaims, binding: binding, private: privateKey},
		{name: "wrong signing key", header: testHeader(), claims: baseClaims, binding: binding, private: otherPrivateKey},
		{name: "wrong issuer", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.Issuer = "model-plane" }), binding: binding, private: privateKey},
		{name: "wrong audience", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.Audience = "other" }), binding: binding, private: privateKey},
		{name: "wrong presenter", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.PresenterService = "other-service" }), binding: binding, private: privateKey},
		{name: "wrong org", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.OrganizationID = "org-2" }), binding: binding, private: privateKey},
		{name: "wrong connection", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ConnectionID = "conn-2" }), binding: binding, private: privateKey},
		{name: "wrong provider", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ProviderKey = "slack" }), binding: binding, private: privateKey},
		{name: "wrong operation", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.Operation = "mail.messages" }), binding: binding, private: privateKey},
		{name: "wrong digest", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.PayloadSHA256 = strings.Repeat("0", 64) }), binding: binding, private: privateKey},
		{name: "wrong idempotency key", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.IdempotencyKey = "conversation:org-1:different" }), binding: binding, private: privateKey},
		{name: "expired", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ExpiresAt = testNowUnix }), binding: binding, private: privateKey},
		{name: "not yet valid", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.NotBefore = testNowUnix + 1 }), binding: binding, private: privateKey},
		{name: "future issued at", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.IssuedAt = testNowUnix + 1 }), binding: binding, private: privateKey},
		{name: "unsupported version", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.Version = 2 }), binding: binding, private: privateKey},
		{name: "missing action", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ActionID = "" }), binding: binding, private: privateKey},
		{name: "missing jti", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.JWTID = "" }), binding: binding, private: privateKey},
		{name: "human intent with approval", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.AuthorizationKind = AuthorizationHumanIntent; c.ApprovalID = "approval-1" }), binding: binding, private: privateKey},
		{name: "human intent action differs from authorization", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) {
			c.AuthorizationKind = AuthorizationHumanIntent
			c.ApprovalID = ""
			c.ActionID = "different-action"
		}), binding: binding, private: privateKey},
		{name: "approved ai without approval", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ApprovalID = "" }), binding: binding, private: privateKey},
		{name: "approved ai approval differs from action", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ApprovalID = "different-approval" }), binding: binding, private: privateKey},
		{name: "unknown authorization kind", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.AuthorizationKind = "automatic" }), binding: binding, private: privateKey},
		{name: "issued at differs from not before", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.NotBefore = c.IssuedAt + 1 }), binding: binding, private: privateKey},
		{name: "zero lifetime", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ExpiresAt = c.IssuedAt }), binding: binding, private: privateKey},
		{name: "lifetime exceeds sixty seconds", header: testHeader(), claims: withClaims(baseClaims, func(c *Claims) { c.ExpiresAt = c.IssuedAt + 61 }), binding: binding, private: privateKey},
		{name: "malformed compact jws", header: testHeader(), claims: baseClaims, binding: binding, private: privateKey, transform: func(string) string { return "not.a.valid.jws" }},
	}
	_ = otherPublicKey

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			token := signCompactJWS(t, tt.private, tt.header, tt.claims)
			if tt.transform != nil {
				token = tt.transform(token)
			}
			if _, err := verifier.Verify(token, tt.binding); err == nil {
				t.Fatal("Verify error = nil, want fail-closed rejection")
			}
		})
	}
}

func TestVerifierSupportsMultipleIssuersWithDistinctKeys(t *testing.T) {
	conversationPublic, conversationPrivate, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey(conversation-core) error: %v", err)
	}
	executionPublic, executionPrivate, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey(model-execution) error: %v", err)
	}
	raw, err := json.Marshal([]map[string]string{
		{"issuer": "conversation-core", "kid": "conversation-write-2026-07", "public_key": base64.StdEncoding.EncodeToString(conversationPublic)},
		{"issuer": "model-execution", "kid": "model-execution-write-2026-08", "public_key": base64.StdEncoding.EncodeToString(executionPublic)},
	})
	if err != nil {
		t.Fatalf("Marshal trusted keys error: %v", err)
	}
	keys, err := ParseTrustedKeysJSON(string(raw))
	if err != nil {
		t.Fatalf("ParseTrustedKeysJSON error: %v", err)
	}
	verifier := NewVerifier(keys, func() time.Time { return time.Unix(testNowUnix, 0).UTC() })

	binding := testBinding(t)
	executionBinding := binding
	executionBinding.PresenterService = "model-execution"
	executionHeader := Header{Algorithm: "EdDSA", Type: AttestationType, KeyID: "model-execution-write-2026-08"}
	executionClaims := withClaims(testClaims(binding), func(c *Claims) {
		c.Issuer = "model-execution"
		c.PresenterService = "model-execution"
	})

	t.Run("model-execution verifies against its own registered key", func(t *testing.T) {
		token := signCompactJWS(t, executionPrivate, executionHeader, executionClaims)
		verified, err := verifier.Verify(token, executionBinding)
		if err != nil {
			t.Fatalf("Verify error: %v", err)
		}
		if verified.Issuer != "model-execution" || verified.KeyID != "model-execution-write-2026-08" {
			t.Fatalf("verified attestation = %#v, want model-execution identifiers", verified)
		}
	})

	t.Run("conversation-core still verifies against its own key unaffected by the second issuer", func(t *testing.T) {
		token := signCompactJWS(t, conversationPrivate, testHeader(), testClaims(binding))
		verified, err := verifier.Verify(token, binding)
		if err != nil {
			t.Fatalf("Verify error: %v", err)
		}
		if verified.Issuer != "conversation-core" {
			t.Fatalf("verified attestation = %#v, want conversation-core", verified)
		}
	})

	t.Run("a key registered for one issuer cannot verify claims asserting a different issuer", func(t *testing.T) {
		// Signed with conversation-core's own key under conversation-core's own
		// kid, but the claims payload asserts the model-execution issuer. The
		// signature itself is valid; only the issuer binding must reject this.
		spoofed := withClaims(testClaims(binding), func(c *Claims) { c.Issuer = "model-execution" })
		token := signCompactJWS(t, conversationPrivate, testHeader(), spoofed)
		if _, err := verifier.Verify(token, binding); err == nil {
			t.Fatal("Verify error = nil, want rejection of a kid/issuer mismatch")
		}
	})
}

func TestParseTrustedKeysJSONRejectsUnsafeConfiguration(t *testing.T) {
	publicKey, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	encoded := base64.StdEncoding.EncodeToString(publicKey)
	tests := []struct {
		name string
		raw  string
	}{
		{name: "missing", raw: ""},
		{name: "not json", raw: "{"},
		{name: "empty list", raw: "[]"},
		{name: "wrong issuer", raw: `[{"issuer":"model-plane","kid":"kid-1","public_key":"` + encoded + `"}]`},
		{name: "empty kid", raw: `[{"issuer":"conversation-core","kid":"","public_key":"` + encoded + `"}]`},
		{name: "placeholder kid", raw: `[{"issuer":"conversation-core","kid":"change-me","public_key":"` + encoded + `"}]`},
		{name: "invalid base64", raw: `[{"issuer":"conversation-core","kid":"kid-1","public_key":"not-base64"}]`},
		{name: "wrong key size", raw: `[{"issuer":"conversation-core","kid":"kid-1","public_key":"` + base64.StdEncoding.EncodeToString([]byte("short")) + `"}]`},
		{name: "zero key", raw: `[{"issuer":"conversation-core","kid":"kid-1","public_key":"` + base64.StdEncoding.EncodeToString(make([]byte, ed25519.PublicKeySize)) + `"}]`},
		{name: "duplicate issuer kid", raw: `[{"issuer":"conversation-core","kid":"kid-1","public_key":"` + encoded + `"},{"issuer":"conversation-core","kid":"kid-1","public_key":"` + encoded + `"}]`},
		{name: "duplicate kid across different issuers", raw: `[{"issuer":"conversation-core","kid":"kid-1","public_key":"` + encoded + `"},{"issuer":"model-execution","kid":"kid-1","public_key":"` + encoded + `"}]`},
		{name: "invalid trailing bytes", raw: `[{"issuer":"conversation-core","kid":"kid-1","public_key":"` + encoded + `"}]garbage`},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := ParseTrustedKeysJSON(tt.raw); err == nil {
				t.Fatal("ParseTrustedKeysJSON error = nil, want rejection")
			}
		})
	}
}

func TestProviderWriteAttestationFixedVector(t *testing.T) {
	fixtureJSON, err := os.ReadFile("../../testdata/provider_write_attestation_v1.json")
	if err != nil {
		t.Fatalf("ReadFile fixed vector error: %v", err)
	}
	var fixture struct {
		PrivateKeySeed string `json:"private_key_seed_base64"`
		PublicKey      string `json:"public_key_base64"`
		JTIRandomBytes string `json:"jti_random_bytes_base64"`
		TTLSeconds     int64  `json:"ttl_seconds"`
		Header         Header `json:"header"`
		Claims         Claims `json:"claims"`
		SendRequest    struct {
			OrganizationID    string   `json:"org_id"`
			ConnectionID      string   `json:"connection_id"`
			Provider          string   `json:"provider"`
			ProviderThreadID  string   `json:"provider_thread_id"`
			BodyText          string   `json:"body_text"`
			BodyHTML          string   `json:"body_html"`
			Subject           string   `json:"subject"`
			To                []string `json:"to"`
			ActorUserID       string   `json:"actor_user_id"`
			AuthorizationKind string   `json:"authorization_kind"`
			AuthorizationID   string   `json:"authorization_id"`
			ApprovalID        string   `json:"approval_id"`
			ActionID          string   `json:"action_id"`
			IdempotencyKey    string   `json:"idempotency_key"`
		} `json:"send_request"`
		Canonical     string `json:"canonical_payload_json"`
		PayloadSHA256 string `json:"payload_sha256"`
		CompactJWS    string `json:"compact_jws"`
	}
	if err := json.Unmarshal(fixtureJSON, &fixture); err != nil {
		t.Fatalf("Unmarshal fixed vector error: %v", err)
	}
	seed, err := base64.StdEncoding.Strict().DecodeString(fixture.PrivateKeySeed)
	if err != nil || len(seed) != ed25519.SeedSize {
		t.Fatalf("fixture seed = %d bytes, %v", len(seed), err)
	}
	privateKey := ed25519.NewKeyFromSeed(seed)
	publicKey := privateKey.Public().(ed25519.PublicKey)
	if base64.StdEncoding.EncodeToString(publicKey) != fixture.PublicKey {
		t.Fatal("fixture public key does not derive from its documented seed")
	}
	threadParts := strings.SplitN(fixture.SendRequest.ProviderThreadID, ":", 2)
	if fixture.SendRequest.Provider != "whatsapp" || len(threadParts) != 2 || len(fixture.SendRequest.To) != 0 {
		t.Fatalf("fixture send_request is not the no-override WhatsApp mapping: %#v", fixture.SendRequest)
	}
	if fixture.Claims.AuthorizationKind != fixture.SendRequest.AuthorizationKind ||
		fixture.Claims.AuthorizationID != fixture.SendRequest.AuthorizationID ||
		fixture.Claims.ApprovalID != fixture.SendRequest.ApprovalID || fixture.Claims.ActionID != fixture.SendRequest.ActionID ||
		fixture.Claims.ActorID != fixture.SendRequest.ActorUserID || fixture.Claims.IdempotencyKey != fixture.SendRequest.IdempotencyKey {
		t.Fatalf("fixture claims do not reproduce send_request authorization: %#v / %#v", fixture.Claims, fixture.SendRequest)
	}
	binding := Binding{
		PresenterService: "conversation-core",
		OrganizationID:   strings.TrimSpace(fixture.SendRequest.OrganizationID),
		ConnectionID:     strings.TrimSpace(fixture.SendRequest.ConnectionID),
		ProviderKey:      strings.TrimSpace(fixture.SendRequest.Provider),
		Operation:        "whatsapp.messages.send",
		Params:           map[string]any{"phoneNumberId": threadParts[0]},
		Body: map[string]any{
			"to": threadParts[1], "type": "text", "text": map[string]any{"body": fixture.SendRequest.BodyText},
		},
		IdempotencyKey: fixture.Claims.IdempotencyKey,
	}
	digest, err := PayloadSHA256(binding)
	if err != nil {
		t.Fatalf("PayloadSHA256 fixed vector error: %v", err)
	}
	canonical, err := json.Marshal(struct {
		OrganizationID string         `json:"org_id"`
		ConnectionID   string         `json:"connection_id"`
		ProviderKey    string         `json:"provider_key"`
		Operation      string         `json:"operation"`
		Params         map[string]any `json:"params"`
		Body           map[string]any `json:"body"`
	}{binding.OrganizationID, binding.ConnectionID, binding.ProviderKey, binding.Operation, binding.Params, binding.Body})
	if err != nil {
		t.Fatalf("Marshal canonical payload error: %v", err)
	}
	jtiSource, err := base64.StdEncoding.Strict().DecodeString(fixture.JTIRandomBytes)
	if err != nil || len(jtiSource) != 16 || base64.RawURLEncoding.EncodeToString(jtiSource) != fixture.Claims.JWTID {
		t.Fatalf("fixture JTI is not base64url of its 16 documented random bytes")
	}
	if fixture.TTLSeconds != 30 || fixture.Claims.IssuedAt != fixture.Claims.NotBefore ||
		fixture.Claims.ExpiresAt-fixture.Claims.IssuedAt != fixture.TTLSeconds {
		t.Fatalf("fixture timestamps/TTL are inconsistent: %#v", fixture.Claims)
	}
	if string(canonical) != fixture.Canonical || digest != fixture.PayloadSHA256 || fixture.Claims.PayloadSHA256 != digest {
		t.Fatalf("fixture canonical payload/digest drifted\ncanonical=%s\ndigest=%s", canonical, digest)
	}
	generatedJWS := signCompactJWS(t, privateKey, fixture.Header, fixture.Claims)
	if generatedJWS != fixture.CompactJWS {
		t.Fatalf("fixture compact JWS drifted\ngenerated=%s", generatedJWS)
	}
	keys, err := ParseTrustedKeysJSON(`[{"issuer":"conversation-core","kid":"` + fixture.Header.KeyID + `","public_key":"` + fixture.PublicKey + `"}]`)
	if err != nil {
		t.Fatalf("ParseTrustedKeysJSON fixture error: %v", err)
	}
	verifier := NewVerifier(keys, func() time.Time { return time.Unix(fixture.Claims.IssuedAt, 0).UTC() })
	binding.PayloadSHA256 = digest
	if _, err := verifier.Verify(fixture.CompactJWS, binding); err != nil {
		t.Fatalf("fixture did not verify: %v", err)
	}
}

func testBinding(t *testing.T) Binding {
	t.Helper()
	binding := Binding{
		PresenterService: "conversation-core",
		OrganizationID:   "org-1",
		ConnectionID:     "conn-1",
		ProviderKey:      "whatsapp",
		Operation:        "whatsapp.messages.send",
		Params:           map[string]any{"phoneNumberId": "phone-1"},
		Body: map[string]any{
			"to": "15550001", "type": "text", "text": map[string]any{"body": "Approved reply"},
		},
		IdempotencyKey: "conversation:org-1:action-1",
	}
	digest, err := PayloadSHA256(binding)
	if err != nil {
		t.Fatalf("PayloadSHA256 error: %v", err)
	}
	binding.PayloadSHA256 = digest
	return binding
}

func testClaims(binding Binding) Claims {
	return Claims{
		Version:           1,
		Issuer:            "conversation-core",
		Audience:          "integration-corev2",
		PresenterService:  binding.PresenterService,
		AuthorizationKind: AuthorizationHumanApprovedAIAction,
		AuthorizationID:   "authorization-1",
		ApprovalID:        "action-1",
		ActionID:          "action-1",
		OrganizationID:    binding.OrganizationID,
		ConnectionID:      binding.ConnectionID,
		ProviderKey:       binding.ProviderKey,
		Operation:         binding.Operation,
		ActorID:           "user-1",
		PayloadSHA256:     binding.PayloadSHA256,
		IdempotencyKey:    binding.IdempotencyKey,
		JWTID:             "AAECAwQFBgcICQoLDA0ODw",
		IssuedAt:          testNowUnix,
		NotBefore:         testNowUnix,
		ExpiresAt:         testNowUnix + 30,
	}
}

func testHeader() Header {
	return Header{Algorithm: "EdDSA", Type: AttestationType, KeyID: "conversation-write-2026-07"}
}

func trustedKeysJSON(t *testing.T, publicKey ed25519.PublicKey) string {
	t.Helper()
	raw, err := json.Marshal([]map[string]string{{
		"issuer": "conversation-core", "kid": "conversation-write-2026-07", "public_key": base64.StdEncoding.EncodeToString(publicKey),
	}})
	if err != nil {
		t.Fatalf("Marshal trusted keys error: %v", err)
	}
	return string(raw)
}

func signCompactJWS(t *testing.T, privateKey ed25519.PrivateKey, header Header, claims Claims) string {
	t.Helper()
	headerJSON, err := json.Marshal(header)
	if err != nil {
		t.Fatalf("Marshal header error: %v", err)
	}
	claimsJSON, err := json.Marshal(claims)
	if err != nil {
		t.Fatalf("Marshal claims error: %v", err)
	}
	encodedHeader := base64.RawURLEncoding.EncodeToString(headerJSON)
	encodedClaims := base64.RawURLEncoding.EncodeToString(claimsJSON)
	signingInput := encodedHeader + "." + encodedClaims
	signature := ed25519.Sign(privateKey, []byte(signingInput))
	return signingInput + "." + base64.RawURLEncoding.EncodeToString(signature)
}

func withClaims(base Claims, update func(*Claims)) Claims {
	out := base
	update(&out)
	return out
}
