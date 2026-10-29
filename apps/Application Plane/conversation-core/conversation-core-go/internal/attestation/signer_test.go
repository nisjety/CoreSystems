package attestation

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func TestSignerProducesFixedEdDSAVectorAndVerifies(t *testing.T) {
	seed := []byte("0123456789abcdef0123456789abcdef")
	privateKey := ed25519.NewKeyFromSeed(seed)
	now := time.Date(2026, time.July, 13, 18, 0, 0, 0, time.UTC)
	signer, err := NewSigner(Config{
		PrivateKey: privateKey,
		KeyID:      "conversation-core-2026-07",
		Issuer:     IssuerConversationCore,
		Audience:   AudienceIntegrationCore,
		Presenter:  PresenterConversationCore,
		TTL:        30 * time.Second,
		Now:        func() time.Time { return now },
		Random:     bytes.NewReader(bytes.Repeat([]byte{0x2a}, 16)),
	})
	if err != nil {
		t.Fatal(err)
	}

	compact, err := signer.Sign(Authorization{
		AuthorizationKind: AuthorizationHumanIntent,
		AuthorizationID:   "outintent_01",
		ActionID:          "outintent_01",
		OrgID:             "org_01",
		ConnectionID:      "conn_01",
		ProviderKey:       "whatsapp",
		Operation:         "whatsapp.messages.send",
		ActorID:           "user_01",
		PayloadSHA256:     strings.Repeat("a", 64),
		IdempotencyKey:    "conversation:reply-01",
	})
	if err != nil {
		t.Fatal(err)
	}

	const fixedVector = "eyJhbGciOiJFZERTQSIsInR5cCI6InZlcmV2b24ucHJvdmlkZXItd3JpdGUtYXR0ZXN0YXRpb24rand0Iiwia2lkIjoiY29udmVyc2F0aW9uLWNvcmUtMjAyNi0wNyJ9.eyJ2IjoxLCJpc3MiOiJjb252ZXJzYXRpb24tY29yZSIsImF1ZCI6ImludGVncmF0aW9uLWNvcmV2MiIsInByZXNlbnRlcl9zZXJ2aWNlIjoiY29udmVyc2F0aW9uLWNvcmUiLCJhdXRob3JpemF0aW9uX2tpbmQiOiJodW1hbl9pbnRlbnQiLCJhdXRob3JpemF0aW9uX2lkIjoib3V0aW50ZW50XzAxIiwiYWN0aW9uX2lkIjoib3V0aW50ZW50XzAxIiwib3JnX2lkIjoib3JnXzAxIiwiY29ubmVjdGlvbl9pZCI6ImNvbm5fMDEiLCJwcm92aWRlcl9rZXkiOiJ3aGF0c2FwcCIsIm9wZXJhdGlvbiI6IndoYXRzYXBwLm1lc3NhZ2VzLnNlbmQiLCJhY3Rvcl9pZCI6InVzZXJfMDEiLCJwYXlsb2FkX3NoYTI1NiI6ImFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWEiLCJpZGVtcG90ZW5jeV9rZXkiOiJjb252ZXJzYXRpb246cmVwbHktMDEiLCJqdGkiOiJLaW9xS2lvcUtpb3FLaW9xS2lvcUtnIiwiaWF0IjoxNzgzOTY1NjAwLCJuYmYiOjE3ODM5NjU2MDAsImV4cCI6MTc4Mzk2NTYzMH0.5OQgSTSP3JnCJnVvWPV40gjrA3PPAGYj9fCEtK0lxieXAnXwSnPMwMK-o3BJ_YZgnUAzeqkNVQT-mbNtPLD0DA"
	if compact != fixedVector {
		t.Fatalf("compact JWS changed\n got: %s\nwant: %s", compact, fixedVector)
	}
	parts := strings.Split(compact, ".")
	if len(parts) != 3 {
		t.Fatalf("compact JWS parts = %d", len(parts))
	}
	decode := func(segment string, target any) {
		t.Helper()
		payload, decodeErr := base64.RawURLEncoding.DecodeString(segment)
		if decodeErr != nil {
			t.Fatal(decodeErr)
		}
		if decodeErr = json.Unmarshal(payload, target); decodeErr != nil {
			t.Fatal(decodeErr)
		}
	}
	var header map[string]any
	decode(parts[0], &header)
	if header["alg"] != "EdDSA" || header["typ"] != TypeProviderWriteAttestation || header["kid"] != "conversation-core-2026-07" || len(header) != 3 {
		t.Fatalf("header = %#v", header)
	}
	var claims Claims
	decode(parts[1], &claims)
	if claims.Version != 1 || claims.Issuer != IssuerConversationCore || claims.Audience != AudienceIntegrationCore || claims.PresenterService != PresenterConversationCore {
		t.Fatalf("authority claims = %#v", claims)
	}
	if claims.AuthorizationKind != AuthorizationHumanIntent || claims.AuthorizationID != "outintent_01" || claims.ActionID != "outintent_01" || claims.ApprovalID != "" {
		t.Fatalf("authorization claims = %#v", claims)
	}
	if claims.IssuedAt != now.Unix() || claims.NotBefore != now.Unix() || claims.ExpiresAt != now.Add(30*time.Second).Unix() {
		t.Fatalf("time claims = %#v", claims)
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatal(err)
	}
	if !ed25519.Verify(privateKey.Public().(ed25519.PublicKey), []byte(parts[0]+"."+parts[1]), signature) {
		t.Fatal("Ed25519 signature did not verify")
	}
}

func TestSignerRejectsInvalidAuthorityAndAuthorizationShapes(t *testing.T) {
	validPrivateKey := ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))
	validConfig := Config{
		PrivateKey: validPrivateKey, KeyID: "key-1", Issuer: IssuerConversationCore,
		Audience: AudienceIntegrationCore, Presenter: PresenterConversationCore,
	}
	for name, mutate := range map[string]func(*Config){
		"wrong key size": func(c *Config) { c.PrivateKey = ed25519.PrivateKey("short") },
		"inconsistent private key": func(c *Config) {
			c.PrivateKey = append(ed25519.PrivateKey(nil), validPrivateKey...)
			c.PrivateKey[len(c.PrivateKey)-1] ^= 0xff
		},
		"missing key id": func(c *Config) { c.KeyID = "" },
		"wrong issuer":   func(c *Config) { c.Issuer = "forged" },
		"wrong audience": func(c *Config) { c.Audience = "forged" },
		"wrong presenter": func(c *Config) {
			c.Presenter = "forged"
		},
	} {
		t.Run(name, func(t *testing.T) {
			cfg := validConfig
			mutate(&cfg)
			if _, err := NewSigner(cfg); err == nil {
				t.Fatal("NewSigner() error = nil")
			}
		})
	}

	signer, err := NewSigner(validConfig)
	if err != nil {
		t.Fatal(err)
	}
	valid := Authorization{
		AuthorizationKind: AuthorizationHumanIntent, AuthorizationID: "intent-1",
		ActionID: "intent-1", OrgID: "org-1", ConnectionID: "conn-1",
		ProviderKey: "slack", Operation: "message.send", ActorID: "user-1",
		PayloadSHA256: strings.Repeat("a", 64), IdempotencyKey: "conversation:reply-1",
	}
	for name, mutate := range map[string]func(*Authorization){
		"missing actor": func(a *Authorization) { a.ActorID = "" },
		"bad digest":    func(a *Authorization) { a.PayloadSHA256 = "abc" },
		"manual approval present": func(a *Authorization) {
			a.ApprovalID = "not-allowed"
		},
		"manual action mismatch": func(a *Authorization) { a.ActionID = "other" },
		"ai approval absent": func(a *Authorization) {
			a.AuthorizationKind = AuthorizationHumanApprovedAIAction
			a.ApprovalID = ""
		},
	} {
		t.Run(name, func(t *testing.T) {
			authorization := valid
			mutate(&authorization)
			if _, err := signer.Sign(authorization); err == nil {
				t.Fatal("Sign() error = nil")
			}
		})
	}
}

func TestPrivateKeyDecoderDigestValidatorAndSignerFailures(t *testing.T) {
	privateKey := ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))
	encoded := base64.StdEncoding.EncodeToString(privateKey)
	decoded, err := DecodePrivateKey(encoded)
	if err != nil || !bytes.Equal(decoded, privateKey) {
		t.Fatalf("DecodePrivateKey() = %d bytes, %v", len(decoded), err)
	}
	for _, invalid := range []string{"not-base64", base64.StdEncoding.EncodeToString(privateKey[:ed25519.SeedSize])} {
		if _, err := DecodePrivateKey(invalid); err == nil {
			t.Fatalf("DecodePrivateKey(%q) error = nil", invalid)
		}
	}
	if !IsLowerHexSHA256(strings.Repeat("a", 64)) || IsLowerHexSHA256(strings.Repeat("A", 64)) || IsLowerHexSHA256("not-a-digest") {
		t.Fatal("IsLowerHexSHA256 accepted a non-canonical digest")
	}

	var nilSigner *Signer
	if _, err := nilSigner.Sign(Authorization{}); err == nil {
		t.Fatal("nil Signer.Sign() error = nil")
	}
	signer, err := NewSigner(Config{
		PrivateKey: privateKey, KeyID: "key-1", Issuer: IssuerConversationCore,
		Audience: AudienceIntegrationCore, Presenter: PresenterConversationCore,
		Random: strings.NewReader("short"),
	})
	if err != nil {
		t.Fatal(err)
	}
	_, err = signer.Sign(Authorization{
		AuthorizationKind: AuthorizationHumanIntent, AuthorizationID: "intent-1", ActionID: "intent-1",
		OrgID: "org-1", ConnectionID: "conn-1", ProviderKey: "slack", Operation: "message.send",
		ActorID: "user-1", PayloadSHA256: strings.Repeat("a", 64), IdempotencyKey: "conversation:test",
	})
	if err == nil {
		t.Fatal("short randomness Sign() error = nil")
	}
}
