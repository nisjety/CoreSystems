package spaces

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// W3-DEC-KIND (decision D17): space_kind_evidence.go issues and signs the
// space-kind-v1 claim that lets a client restore an old note once it proves
// which kind of Space it belongs to. These are the pure unit tests for the
// issuer's input checks and the signed envelope's shape, version tag and
// signature. The HTTP-level 403/200 behaviour against a real membership read
// lives in routes_space_kind_evidence_test.go (internal/http), gated on a
// live database like the other route tests in that package.

func validSpaceKindMembership() *CurrentMembership {
	return &CurrentMembership{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", Kind: KindPersonal,
	}
}

func TestIssueSpaceKindEvidenceRefusesANilMembership(t *testing.T) {
	if _, err := IssueSpaceKindEvidence(nil, time.Now()); err == nil {
		t.Fatal("a nil membership was accepted")
	}
}

// A caller with no readable membership never reaches this function, but a
// membership that was resolved and simply never recorded a kind (the
// unknown-kind legacy case this whole slice exists for) must still be
// refused: it is exactly the case the restore has to fail closed on.
func TestIssueSpaceKindEvidenceRefusesAKindlessMembership(t *testing.T) {
	now := time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)
	for name, kind := range map[string]Kind{"empty": "", "whitespace only": "   "} {
		t.Run(name, func(t *testing.T) {
			membership := validSpaceKindMembership()
			membership.Kind = kind
			if _, err := IssueSpaceKindEvidence(membership, now); err == nil {
				t.Fatal("a membership with no recorded kind was accepted")
			}
		})
	}
}

func TestIssueSpaceKindEvidenceRefusesAZeroIssuanceTime(t *testing.T) {
	if _, err := IssueSpaceKindEvidence(validSpaceKindMembership(), time.Time{}); err == nil {
		t.Fatal("a zero issuance time was accepted")
	}
}

// The one issuer every caller funnels through carries Control's own facts
// through unchanged, trims padding around the kind the same way the older
// kind-claim assembler does (space_kind_claim_test.go), and bounds the claim
// to SpaceKindEvidenceTTL from the issuance time it was given.
func TestIssueSpaceKindEvidenceSetsEveryFieldFromTheMembership(t *testing.T) {
	now := time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)
	membership := &CurrentMembership{
		OrgID: "org-9", SpaceRef: "space-9", SubjectID: "user-9", Kind: Kind("  room  "),
	}
	evidence, err := IssueSpaceKindEvidence(membership, now)
	if err != nil {
		t.Fatalf("IssueSpaceKindEvidence: %v", err)
	}
	if evidence.OrgID != "org-9" || evidence.SpaceRef != "space-9" || evidence.SubjectID != "user-9" {
		t.Fatalf("evidence = %+v, want Control's own facts carried through unchanged", evidence)
	}
	if evidence.Kind != "room" {
		t.Fatalf("kind = %q, want the padding trimmed to room", evidence.Kind)
	}
	if !evidence.IssuedAt.Equal(now.UTC()) {
		t.Fatalf("issued_at = %v, want %v", evidence.IssuedAt, now.UTC())
	}
	if want := now.UTC().Add(SpaceKindEvidenceTTL); !evidence.ExpiresAt.Equal(want) {
		t.Fatalf("expires_at = %v, want %v", evidence.ExpiresAt, want)
	}
	if SpaceKindEvidenceTTL != 2*time.Minute {
		t.Fatalf("SpaceKindEvidenceTTL = %v, want 2 minutes (the gateway asks immediately before the restore it serves)", SpaceKindEvidenceTTL)
	}
}

func testSpaceKindSigningKey(t *testing.T) SigningKey {
	t.Helper()
	return SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
}

// verifySpaceKindEvidenceToken checks the signature the way Session Core's
// Rust verifier would (version.key-id.payload.signature) and returns the
// decoded key id and claim, so assertions run on what is actually signed.
func verifySpaceKindEvidenceToken(t *testing.T, token string, public ed25519.PublicKey) (string, SpaceKindEvidence) {
	t.Helper()
	parts := strings.Split(token, ".")
	if len(parts) != 4 {
		t.Fatalf("token has %d parts, want 4: %s", len(parts), token)
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil {
		t.Fatalf("decode signature: %v", err)
	}
	if !ed25519.Verify(public, []byte(strings.Join(parts[:3], ".")), signature) {
		t.Fatal("the evidence's signature does not verify against the matching public key")
	}
	keyID, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		t.Fatalf("decode key id: %v", err)
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	var evidence SpaceKindEvidence
	if err := json.Unmarshal(payload, &evidence); err != nil {
		t.Fatalf("decode evidence: %v", err)
	}
	return string(keyID), evidence
}

// The signed output verifies with the matching public key, carries the
// space-kind-v1 tag (distinct from the decision envelope's own version, by
// construction: see the comment on SpaceKindEvidenceVersion), and does not
// verify against a different key.
func TestSignSpaceKindEvidenceVerifiesAndCarriesTheVersionTag(t *testing.T) {
	now := time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)
	key := testSpaceKindSigningKey(t)
	evidence, err := IssueSpaceKindEvidence(validSpaceKindMembership(), now)
	if err != nil {
		t.Fatalf("IssueSpaceKindEvidence: %v", err)
	}
	token, err := SignSpaceKindEvidence(key, evidence)
	if err != nil {
		t.Fatalf("SignSpaceKindEvidence: %v", err)
	}

	parts := strings.Split(token, ".")
	if len(parts) != 4 {
		t.Fatalf("token has %d parts, want 4: %s", len(parts), token)
	}
	if parts[0] != SpaceKindEvidenceVersion {
		t.Fatalf("token version tag = %q, want %q", parts[0], SpaceKindEvidenceVersion)
	}
	if SpaceKindEvidenceVersion == decisionVersion {
		t.Fatalf("the evidence tag %q must stay distinct from the decision version %q", SpaceKindEvidenceVersion, decisionVersion)
	}

	public := key.PrivateKey.Public().(ed25519.PublicKey)
	keyID, decoded := verifySpaceKindEvidenceToken(t, token, public)
	if keyID != key.ID {
		t.Fatalf("key id = %q, want %q", keyID, key.ID)
	}
	if decoded.OrgID != evidence.OrgID || decoded.SpaceRef != evidence.SpaceRef ||
		decoded.SubjectID != evidence.SubjectID || decoded.Kind != evidence.Kind {
		t.Fatalf("signed evidence = %+v, want the issued evidence %+v unchanged", decoded, evidence)
	}
	if !decoded.IssuedAt.Equal(evidence.IssuedAt) || !decoded.ExpiresAt.Equal(evidence.ExpiresAt) {
		t.Fatalf("signed evidence timestamps = %+v, want %+v", decoded, evidence)
	}

	other, _, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil {
		t.Fatalf("decode signature: %v", err)
	}
	if ed25519.Verify(other, []byte(strings.Join(parts[:3], ".")), signature) {
		t.Fatal("the evidence verified against a key that never signed it")
	}
}

func TestSignSpaceKindEvidenceRefusesAnInvalidSigningKey(t *testing.T) {
	now := time.Date(2026, 11, 18, 12, 0, 0, 0, time.UTC)
	evidence, err := IssueSpaceKindEvidence(validSpaceKindMembership(), now)
	if err != nil {
		t.Fatalf("IssueSpaceKindEvidence: %v", err)
	}
	for name, key := range map[string]SigningKey{
		"no key id":        {PrivateKey: testSpaceKindSigningKey(t).PrivateKey},
		"wrong key length": {ID: "k", PrivateKey: ed25519.PrivateKey{1, 2}},
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := SignSpaceKindEvidence(key, evidence); err == nil {
				t.Fatal("an invalid signing key was accepted")
			}
		})
	}
}

func TestSignSpaceKindEvidenceRefusesInvalidEvidence(t *testing.T) {
	key := testSpaceKindSigningKey(t)
	if _, err := SignSpaceKindEvidence(key, SpaceKindEvidence{}); err == nil {
		t.Fatal("evidence that fails validation was signed")
	}
}
