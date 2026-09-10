package authz

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

func validCapabilityClaims() SpaceCapabilityClaims {
	return SpaceCapabilityClaims{
		BackendID: "backend-1", ProfileDigest: "sha256:" + strings.Repeat("a", 64),
		Persistence: "ephemeral", Processes: "isolated", Backup: false,
		Egress: "disabled_by_default", CredentialMode: "credential_free",
	}
}

func signedCapabilityToken(t *testing.T, private ed25519.PrivateKey, keyID string, claims SpaceCapabilityClaims, mutate func(*spaceCapabilityDecision)) (string, string) {
	t.Helper()
	now := time.Now().UTC()
	decision := spaceCapabilityDecision{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1",
		ServiceAudience: spaceCapabilityAudience, ActionID: spaceCapabilityAction, ActionSchemaHash: spaceCapabilitySchema,
		IdempotencyKey: "capability-1", RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "resource-1", AuthorityRevision: 7,
		MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
		Permissions: []string{"space:sandbox:use"}, ExpiresAt: now.Add(time.Minute),
	}
	if mutate != nil {
		mutate(&decision)
	}
	if decision.PayloadDigest == "" {
		decision.PayloadDigest = expectedCapabilityPayloadDigest(decision, claims)
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	claimsJSON, err := json.Marshal(claims)
	if err != nil {
		t.Fatal(err)
	}
	signed := spaceCapabilityDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	token := signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
	return token, string(claimsJSON)
}

func expectedCapabilityVerifyRequest() CapabilityExpectation {
	return CapabilityExpectation{OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1"}
}

func TestVerifyAcceptsAFreshSignedBoundDecisionAndReturnsItsClaims(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "key-1"
		case "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(public)
		default:
			return ""
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	claims := validCapabilityClaims()
	token, claimsJSON := signedCapabilityToken(t, private, "key-1", claims, nil)

	got, err := verifier.Verify(token, claimsJSON, expectedCapabilityVerifyRequest())
	if err != nil {
		t.Fatalf("valid fresh signed decision denied: %v", err)
	}
	if got.BackendID != claims.BackendID {
		t.Fatalf("BackendID = %q, want %q", got.BackendID, claims.BackendID)
	}
}

func TestVerifyRejectsWhenPlaintextClaimsDoNotMatchTheSignedDigest(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "key-1"
		case "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(public)
		default:
			return ""
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	signedClaims := validCapabilityClaims()
	token, _ := signedCapabilityToken(t, private, "key-1", signedClaims, nil)

	tamperedClaims := signedClaims
	tamperedClaims.BackendID = "backend-attacker"
	tamperedJSON, err := json.Marshal(tamperedClaims)
	if err != nil {
		t.Fatal(err)
	}

	if _, err := verifier.Verify(token, string(tamperedJSON), expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("expected rejection of a decision whose signed digest does not match the presented claims")
	}
}

func TestVerifyRejectsExpiredOrMistargetedDecisions(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "key-1"
		case "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(public)
		default:
			return ""
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	claims := validCapabilityClaims()

	for _, tc := range []struct {
		name   string
		mutate func(*spaceCapabilityDecision)
	}{
		{"expired", func(d *spaceCapabilityDecision) { d.ExpiresAt = time.Now().UTC().Add(-time.Minute) }},
		{"wrong org", func(d *spaceCapabilityDecision) { d.OrgID = "org-attacker" }},
		{"wrong space", func(d *spaceCapabilityDecision) { d.SpaceRef = "space-attacker" }},
		{"wrong subject", func(d *spaceCapabilityDecision) { d.SubjectID = "user-attacker" }},
		{"missing sandbox permission", func(d *spaceCapabilityDecision) { d.Permissions = nil }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			token, claimsJSON := signedCapabilityToken(t, private, "key-1", claims, tc.mutate)
			if _, err := verifier.Verify(token, claimsJSON, expectedCapabilityVerifyRequest()); err == nil {
				t.Fatal("expected rejection")
			}
		})
	}
}

func TestVerifyRejectsEgressCapableClaimsWithoutEgressPermission(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "key-1"
		case "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(public)
		default:
			return ""
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	claims := validCapabilityClaims()
	claims.Egress = "outbound_allowlisted"
	token, claimsJSON := signedCapabilityToken(t, private, "key-1", claims, nil)

	if _, err := verifier.Verify(token, claimsJSON, expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("expected rejection of an egress-capable claim whose decision grants no space:egress permission")
	}

	tokenWithEgress, claimsJSONWithEgress := signedCapabilityToken(t, private, "key-1", claims, func(d *spaceCapabilityDecision) {
		d.Permissions = []string{"space:sandbox:use", "space:egress"}
	})
	if _, err := verifier.Verify(tokenWithEgress, claimsJSONWithEgress, expectedCapabilityVerifyRequest()); err != nil {
		t.Fatalf("egress-capable claim with space:egress permission should be accepted: %v", err)
	}
}

func TestVerifyRejectsWrongSigningKeyOrTamperedSignature(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	_, otherPrivate, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		switch name {
		case "CONTROL_SPACE_DECISION_KEY_ID":
			return "key-1"
		case "CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64":
			return base64.RawURLEncoding.EncodeToString(public)
		default:
			return ""
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	claims := validCapabilityClaims()
	tokenSignedByOtherKey, claimsJSON := signedCapabilityToken(t, otherPrivate, "key-1", claims, nil)
	if _, err := verifier.Verify(tokenSignedByOtherKey, claimsJSON, expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("expected rejection of a decision signed by an untrusted private key")
	}

	tokenWithUnknownKeyID, _ := signedCapabilityToken(t, private, "key-2", claims, nil)
	if _, err := verifier.Verify(tokenWithUnknownKeyID, claimsJSON, expectedCapabilityVerifyRequest()); err == nil {
		t.Fatal("expected rejection of a decision under an untrusted key id")
	}
}

func TestLoadSpaceCapabilityVerifierFromEnvRejectsInvalidConfiguration(t *testing.T) {
	if _, err := LoadSpaceCapabilityVerifierFromEnv(nil); err == nil {
		t.Fatal("expected error for nil environment reader")
	}
	if _, err := LoadSpaceCapabilityVerifierFromEnv(func(string) string { return "" }); err == nil {
		t.Fatal("expected error for absent configuration")
	}
	if _, err := LoadSpaceCapabilityVerifierFromEnv(func(name string) string {
		if name == "CONTROL_SPACE_DECISION_KEY_ID" {
			return "key-1"
		}
		return "not-base64url-and-wrong-length"
	}); err == nil {
		t.Fatal("expected error for malformed public key")
	}
}
