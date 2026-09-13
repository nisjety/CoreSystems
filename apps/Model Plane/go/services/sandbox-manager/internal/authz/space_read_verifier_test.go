package authz

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"
	"time"
)

// A decision Control would actually sign for sandbox-manager, built the way
// user-core builds it so the digest formula is exercised rather than
// hard-coded. If Control's formula changes, these tests fail here — which is
// the point: two services computing "the same" digest differently is a hole
// neither side's own tests can see.
func validSpaceReadDecision(now time.Time) spaceReadDecision {
	decision := spaceReadDecision{
		DecisionRef:               "read-decision-1",
		OrgID:                     "org-a",
		SpaceRef:                  "space-1",
		SubjectID:                 "user-a",
		ServiceAudience:           spaceReadAudience,
		ActionID:                  spaceReadAction,
		ActionSchemaHash:          spaceReadSchema,
		IdempotencyKey:            "idem-1",
		Nonce:                     "nonce-1",
		RecipientAudienceRef:      "space:space-1:recipient-audience:4",
		RecipientAudienceHash:     "sha256:audience",
		PrivacyPolicyRef:          "policy-1",
		ResourceAuthorizationRef:  "control:space-1:thread-read:7",
		Purpose:                   "space_work_read",
		LawfulBasis:               "contract",
		PrivacyClass:              "internal",
		RetentionClass:            "standard",
		Residency:                 "eu",
		DeletionScope:             "space",
		AuthorityRevision:         3,
		RecipientAudienceRevision: 4,
		Permissions:               []string{"thread:read"},
		IssuedAt:                  now.Add(-time.Minute),
		ExpiresAt:                 now.Add(5 * time.Minute),
	}
	decision.PayloadDigest = expectedSpaceReadPayloadDigest(decision)
	return decision
}

func signSpaceRead(t *testing.T, decision spaceReadDecision) (*SpaceCapabilityVerifier, string) {
	t.Helper()
	public, private, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	const keyID = "control-space-key-1"
	signed := strings.Join([]string{
		spaceCapabilityDecisionVersion,
		base64.RawURLEncoding.EncodeToString([]byte(keyID)),
		base64.RawURLEncoding.EncodeToString(payload),
	}, ".")
	token := signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
	return &SpaceCapabilityVerifier{keyID: keyID, public: public}, token
}

func expectFor(decision spaceReadDecision, now time.Time) SpaceReadExpectation {
	return SpaceReadExpectation{
		OrgID:       decision.OrgID,
		SpaceRef:    decision.SpaceRef,
		SubjectID:   decision.SubjectID,
		DecisionRef: decision.DecisionRef,
		Now:         now,
	}
}

func TestVerifySpaceReadAcceptsAControlIssuedReadAndReturnsItsCeiling(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision := validSpaceReadDecision(now)
	verifier, token := signSpaceRead(t, decision)

	verified, err := verifier.VerifySpaceRead(token, expectFor(decision, now))
	if err != nil {
		t.Fatalf("VerifySpaceRead: %v", err)
	}
	if verified.RecipientAudienceRevision != 4 {
		t.Fatalf("ceiling = %d, want the decision's own audience revision 4", verified.RecipientAudienceRevision)
	}
}

// The audience is the ONLY thing separating this decision from the one Session
// Core verifies: same action, same schema, same digest, same permission. If
// this check ever weakened, a decision issued to read a Space's conversation
// would also read its process output, and the gateway's two upstreams would
// stop being separately authorized.
func TestVerifySpaceReadRefusesADecisionAddressedToSessionCore(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision := validSpaceReadDecision(now)
	decision.ServiceAudience = "model-plane"
	decision.PayloadDigest = expectedSpaceReadPayloadDigest(decision)
	verifier, token := signSpaceRead(t, decision)

	if _, err := verifier.VerifySpaceRead(token, expectFor(decision, now)); err == nil {
		t.Fatal("a Session Core decision was accepted here; the two audiences must not be interchangeable")
	}
}

// Every field the expectation pins comes from the caller's authenticated
// identity or the request, never from the token. A decision for another
// member, another Space, another org or another ref must not verify.
func TestVerifySpaceReadRefusesADecisionForSomeoneOrSomethingElse(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision := validSpaceReadDecision(now)
	verifier, token := signSpaceRead(t, decision)

	for name, mutate := range map[string]func(*SpaceReadExpectation){
		"another org":     func(e *SpaceReadExpectation) { e.OrgID = "org-b" },
		"another Space":   func(e *SpaceReadExpectation) { e.SpaceRef = "space-2" },
		"another member":  func(e *SpaceReadExpectation) { e.SubjectID = "user-b" },
		"another request": func(e *SpaceReadExpectation) { e.DecisionRef = "read-decision-2" },
	} {
		expect := expectFor(decision, now)
		mutate(&expect)
		if _, err := verifier.VerifySpaceRead(token, expect); err == nil {
			t.Fatalf("a decision verified for %s", name)
		}
	}
}

// A read decision must never double as authority to write. Control issues
// disjoint permission sets; this keeps the disjointness a property the
// recipient enforces rather than one it takes on trust.
func TestVerifySpaceReadRefusesAReadThatAlsoCarriesWritePermissions(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	for _, write := range []string{"thread:append", "thread:create"} {
		decision := validSpaceReadDecision(now)
		decision.Permissions = []string{"thread:read", write}
		decision.PayloadDigest = expectedSpaceReadPayloadDigest(decision)
		verifier, token := signSpaceRead(t, decision)
		if _, err := verifier.VerifySpaceRead(token, expectFor(decision, now)); err == nil {
			t.Fatalf("a read decision carrying %s was accepted", write)
		}
	}
}

// A zero audience revision would become an unbounded ceiling if it reached the
// store, so it is refused at the door instead. This is why the store's ceiling
// is a POINTER: absent and zero must never collapse into the same thing.
func TestVerifySpaceReadRefusesAnIncompleteOrExpiredDecision(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	for name, mutate := range map[string]func(*spaceReadDecision){
		"no audience revision": func(d *spaceReadDecision) { d.RecipientAudienceRevision = 0 },
		"no audience ref":      func(d *spaceReadDecision) { d.RecipientAudienceRef = "" },
		"no nonce":             func(d *spaceReadDecision) { d.Nonce = "" },
		"no purpose":           func(d *spaceReadDecision) { d.Purpose = "" },
		"no lawful basis":      func(d *spaceReadDecision) { d.LawfulBasis = "" },
		"no residency":         func(d *spaceReadDecision) { d.Residency = "" },
		"expired":              func(d *spaceReadDecision) { d.ExpiresAt = now.Add(-time.Second) },
		"issued in the future": func(d *spaceReadDecision) { d.IssuedAt = now.Add(2 * time.Minute) },
	} {
		decision := validSpaceReadDecision(now)
		mutate(&decision)
		decision.PayloadDigest = expectedSpaceReadPayloadDigest(decision)
		verifier, token := signSpaceRead(t, decision)
		if _, err := verifier.VerifySpaceRead(token, expectFor(decision, now)); err == nil {
			t.Fatalf("a decision with %s was accepted", name)
		}
	}
}

// The digest binds the decision to the effect. Tampering with a plaintext the
// digest covers must fail even though the signature is over the payload as a
// whole — because a caller that can re-sign is not the threat this guards; a
// Control bug that signs a mismatched digest is.
func TestVerifySpaceReadRefusesAMismatchedPayloadDigest(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision := validSpaceReadDecision(now)
	decision.PayloadDigest = "sha256:0000000000000000000000000000000000000000000000000000000000000000"
	verifier, token := signSpaceRead(t, decision)

	if _, err := verifier.VerifySpaceRead(token, expectFor(decision, now)); err == nil {
		t.Fatal("a decision whose digest does not bind its own claims was accepted")
	}
}

// Base64URL, not std base64. A std-only decoder silently fails on every token
// containing `-` or `_`, which disables the whole decision lane rather than
// rejecting one token loudly — a failure this repo has already had once.
func TestVerifySpaceReadRefusesAnUntrustedKeyAndAMangledEnvelope(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision := validSpaceReadDecision(now)
	verifier, token := signSpaceRead(t, decision)
	expect := expectFor(decision, now)

	other, otherToken := signSpaceRead(t, decision)
	_ = other
	if _, err := verifier.VerifySpaceRead(otherToken, expect); err == nil {
		t.Fatal("a decision signed by a different key was accepted")
	}
	for name, mangled := range map[string]string{
		"empty":         "",
		"wrong version": "v1." + strings.SplitN(token, ".", 2)[1],
		"missing part":  strings.Join(strings.Split(token, ".")[:3], "."),
		"broken sig":    strings.Join(strings.Split(token, ".")[:3], ".") + ".AAAA",
		"not base64":    "v2.!!!.!!!.!!!",
	} {
		if _, err := verifier.VerifySpaceRead(mangled, expect); err == nil {
			t.Fatalf("a %s envelope was accepted", name)
		}
	}
}

// The three request-side fields are required TOGETHER. A caller that brings a
// token but no ref (or names no Space) has not presented an authority, and
// serving them would mean verifying a decision against whatever the row said.
func TestVerifySpaceReadRequiresTokenRefAndSpaceTogether(t *testing.T) {
	now := time.Date(2026, 9, 13, 12, 0, 0, 0, time.UTC)
	decision := validSpaceReadDecision(now)
	verifier, token := signSpaceRead(t, decision)

	for name, expect := range map[string]SpaceReadExpectation{
		"no decision ref": {OrgID: "org-a", SpaceRef: "space-1", SubjectID: "user-a", Now: now},
		"no space":        {OrgID: "org-a", SubjectID: "user-a", DecisionRef: "read-decision-1", Now: now},
	} {
		if _, err := verifier.VerifySpaceRead(token, expect); err == nil {
			t.Fatalf("%s was accepted", name)
		}
	}
	if _, err := verifier.VerifySpaceRead("", expectFor(decision, now)); err == nil {
		t.Fatal("an empty token was accepted")
	}
}
