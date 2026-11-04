package spaces

import (
	"crypto/ed25519"
	"strings"
	"testing"
	"time"
)

// WS14-S2: Control verifying the import execution decision it signed itself,
// and the strict shape a room-file grant accepts.

func validFileGrantDecision(now time.Time) Decision {
	decision := validDecision(now)
	decision.ServiceAudience = personalImportExecutionAudience
	decision.ActionID = personalImportAction
	decision.ActionSchemaHash = personalImportSchema
	decision.Permissions = []string{"documents:write"}
	decision.ImportSourceType = SpaceUploadSourceType
	return decision
}

func TestVerifyIssuedDecisionAcceptsOwnTokenAndRejectsTamperedForeignAndExpired(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)
	signer, _ := testDecisionKeys(t)
	token, err := SignDecision(signer, validFileGrantDecision(now))
	if err != nil {
		t.Fatalf("SignDecision: %v", err)
	}

	decision, err := VerifyIssuedDecision(signer, token, now)
	if err != nil {
		t.Fatalf("own token must verify: %v", err)
	}
	if decision.ImportSourceType != SpaceUploadSourceType || decision.SubjectID != "user-1" {
		t.Fatalf("decoded claims = %+v", decision)
	}

	// Tampered payload: the signature no longer covers it.
	parts := strings.Split(token, ".")
	forged := validFileGrantDecision(now)
	forged.SubjectID = "user-2"
	forgedToken, err := SignDecision(signer, forged)
	if err != nil {
		t.Fatal(err)
	}
	forgedParts := strings.Split(forgedToken, ".")
	tampered := strings.Join([]string{parts[0], parts[1], forgedParts[2], parts[3]}, ".")
	if _, err := VerifyIssuedDecision(signer, tampered, now); err == nil {
		t.Fatal("a payload swapped under an existing signature must be rejected")
	}

	// Another key with the same key id.
	otherSigner, _ := testDecisionKeys(t)
	if _, err := VerifyIssuedDecision(otherSigner, token, now); err == nil {
		t.Fatal("a token signed by another key must be rejected")
	}
	// Same key, different key id.
	renamed := SigningKey{ID: "control-other", PrivateKey: signer.PrivateKey}
	if _, err := VerifyIssuedDecision(renamed, token, now); err == nil {
		t.Fatal("an unknown key id must be rejected")
	}

	// Expired and not-yet-valid.
	if _, err := VerifyIssuedDecision(signer, token, now.Add(2*time.Minute)); err == nil {
		t.Fatal("an expired decision must be rejected")
	}
	if _, err := VerifyIssuedDecision(signer, token, now.Add(-10*time.Minute)); err == nil {
		t.Fatal("a decision issued in the future must be rejected")
	}

	// Garbage and a missing key.
	for _, bad := range []string{"", "x.y.z", "a.b.c.d", strings.Repeat("a", 40)} {
		if _, err := VerifyIssuedDecision(signer, bad, now); err == nil {
			t.Errorf("token %q must be rejected", bad)
		}
	}
	if _, err := VerifyIssuedDecision(SigningKey{}, token, now); err == nil {
		t.Fatal("a missing signing key must be rejected")
	}
	if _, err := VerifyIssuedDecision(SigningKey{ID: "k", PrivateKey: ed25519.PrivateKey{1, 2}}, token, now); err == nil {
		t.Fatal("a malformed private key must be rejected")
	}
}

func TestVerifyIssuedDecisionUsesTheCurrentClockWhenNoneIsGiven(t *testing.T) {
	signer, _ := testDecisionKeys(t)
	now := time.Now().UTC()
	token, err := SignDecision(signer, validFileGrantDecision(now))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := VerifyIssuedDecision(signer, token, time.Time{}); err != nil {
		t.Fatalf("a zero clock falls back to now: %v", err)
	}
}

func TestVerifyDecisionIsUnchangedAfterTheParseWindowSplit(t *testing.T) {
	now := time.Date(2026, 8, 13, 12, 0, 0, 0, time.UTC)
	signer, verifier := testDecisionKeys(t)
	token, err := SignDecision(signer, validDecision(now))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := VerifyDecision(verifier, token, expectedDecision(now)); err != nil {
		t.Fatalf("matching expectation must verify: %v", err)
	}
	wrong := expectedDecision(now)
	wrong.SubjectID = "user-2"
	if _, err := VerifyDecision(verifier, token, wrong); err == nil {
		t.Fatal("a different subject must be rejected")
	}
	wrong = expectedDecision(now)
	wrong.PayloadDigest = "sha256:other"
	if _, err := VerifyDecision(verifier, token, wrong); err == nil {
		t.Fatal("a different payload must be rejected")
	}
	expired := expectedDecision(now.Add(5 * time.Minute))
	if _, err := VerifyDecision(verifier, token, expired); err == nil {
		t.Fatal("an expired decision must be rejected")
	}
	if _, err := VerifyDecision(VerificationKey{}, token, expectedDecision(now)); err == nil {
		t.Fatal("a missing verification key must be rejected")
	}
}

func TestValidateForFileGrantMatrix(t *testing.T) {
	now := time.Now().UTC()
	if err := validFileGrantDecision(now).ValidateForFileGrant(); err != nil {
		t.Fatalf("a room-upload import execution decision must pass: %v", err)
	}
	cases := []struct {
		name   string
		mutate func(*Decision)
		typed  DenialCode
	}{
		{"wrong action", func(d *Decision) { d.ActionID = "model.thread.create" }, ""},
		{"wrong schema", func(d *Decision) { d.ActionSchemaHash = "sha256:thread-create-v1" }, ""},
		{"initiation audience, not execution", func(d *Decision) { d.ServiceAudience = personalImportAudience }, ""},
		{"model plane audience", func(d *Decision) { d.ServiceAudience = "model-plane" }, ""},
		{"personal source type", func(d *Decision) { d.ImportSourceType = "notion" }, ""},
		{"no source type", func(d *Decision) { d.ImportSourceType = "" }, ""},
		{"zero data retention", func(d *Decision) { d.ZeroDataRetention = true }, DenialZDRForbidsEffect},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			decision := validFileGrantDecision(now)
			tc.mutate(&decision)
			err := decision.ValidateForFileGrant()
			if err == nil {
				t.Fatal("expected a refusal")
			}
			if tc.typed != "" {
				denial, ok := AsDenial(err)
				if !ok || denial.Code != tc.typed {
					t.Fatalf("refusal = %v, want denial %s", err, tc.typed)
				}
			}
		})
	}
}

func TestResolveFileGrantAuthorityFailsClosedWithoutARepository(t *testing.T) {
	var repo *Repository
	if _, err := repo.ResolveFileGrantAuthority(t.Context(), validFileGrantDecision(time.Now())); err == nil {
		t.Fatal("a nil repository must fail closed")
	}
	if _, err := (&Repository{}).ResolveFileGrantAuthority(t.Context(), validFileGrantDecision(time.Now())); err == nil {
		t.Fatal("a repository without a database must fail closed")
	}
}
