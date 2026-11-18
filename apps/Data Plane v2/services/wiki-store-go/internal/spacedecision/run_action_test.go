package spacedecision

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"
)

// runGoldenPath is the Space self-API registry golden (WS18-S1, WS19-S9), read-only here. Its digest_vectors hold the one
// shared vector that pins the agent payload digest on both the Go and the Rust side.
const runGoldenPath = "../../../../../Model Plane/api/contracts/space-self-api-registry.json"

// wellFormedRunClaims is a complete agent read decision for a room, valid at now.
func wellFormedRunClaims(now time.Time) RunClaims {
	return RunClaims{
		DecisionRef:                "decision-run-verify-1",
		RunID:                      "run-verify-1",
		ThreadID:                   "thread-verify-1",
		OrgID:                      "org-1",
		SpaceRef:                   "space-room-1",
		SubjectID:                  "user-2",
		ServiceAudience:            AudienceCanvas,
		ActionID:                   ActionRunCanvasRead,
		ActionSchemaHash:           SchemaRunCanvasRead,
		PayloadDigest:              "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey:             "space.canvas.read:verify-1",
		RecipientAudienceRef:       "audience:space-room-1:4",
		RecipientAudienceHash:      "sha256:audience-hash",
		RecipientAudienceRevision:  4,
		PrivacyPolicyRef:           "privacy:org-1:5",
		RunContextAuthorizationRef: "control:run:verify-1",
		AuthorityRevision:          7,
		Permissions:                []string{PermissionCanvasRead},
		Purpose:                    "agent canvas read",
		LawfulBasis:                "contract",
		PrivacyClass:               "room",
		RetentionClass:             "space",
		Residency:                  "eu",
		DeletionScope:              "space",
		IssuedAt:                   now.Add(-time.Minute),
		ExpiresAt:                  now.Add(time.Minute),
		Nonce:                      "nonce-run-verify-1",
	}
}

// wellFormedRunWriteClaims is wellFormedRunClaims for an agent canvas write.
func wellFormedRunWriteClaims(now time.Time) RunClaims {
	c := wellFormedRunClaims(now)
	c.ActionID = ActionRunCanvasWrite
	c.ActionSchemaHash = SchemaRunCanvasWrite
	c.Permissions = []string{PermissionCanvasWrite}
	return c
}

func TestVerifyRunActionAcceptsAWellFormedRunDecision(t *testing.T) {
	signer := newTestSigner("run-accept")
	keys := KeySet{signer.id: signer.public()}
	token := signEnvelope(t, RunEnvelopeVersion, signer, wellFormedRunClaims(verifyNow))

	claims, err := VerifyRunAction(token, keys, verifyNow)
	if err != nil {
		t.Fatalf("a well-formed run-action decision was refused: %v", err)
	}
	if claims.ActionID != ActionRunCanvasRead || claims.RunID != "run-verify-1" {
		t.Fatalf("verified run claims lost their fields: %+v", claims)
	}
}

func TestVerifyRunActionRefusesADecisionOutsideItsLife(t *testing.T) {
	signer := newTestSigner("run-life")
	keys := KeySet{signer.id: signer.public()}

	expired := wellFormedRunClaims(verifyNow)
	expired.IssuedAt = verifyNow.Add(-2 * time.Minute)
	expired.ExpiresAt = verifyNow
	_, err := VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, signer, expired), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	future := wellFormedRunClaims(verifyNow)
	future.IssuedAt = verifyNow.Add(5 * time.Minute)
	future.ExpiresAt = verifyNow.Add(7 * time.Minute)
	_, err = VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, signer, future), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestVerifyRunActionRefusesAnUntrustedKeyAndABadSignature(t *testing.T) {
	signer := newTestSigner("run-trusted")
	keys := KeySet{signer.id: signer.public()}
	claims := wellFormedRunClaims(verifyNow)

	stranger := newTestSigner("run-stranger")
	_, err := VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, stranger, claims), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	forger := testSigner{id: signer.id, priv: stranger.priv}
	_, err = VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, forger, claims), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)

	// The signature covers the payload, so a changed payload under the old signature must fail.
	parts := strings.Split(signEnvelope(t, RunEnvelopeVersion, signer, claims), ".")
	tampered := claims
	tampered.RunID = "run-other"
	tamperedPayload, err := json.Marshal(tampered)
	if err != nil {
		t.Fatalf("marshal tampered run claims: %v", err)
	}
	parts[2] = base64.RawURLEncoding.EncodeToString(tamperedPayload)
	_, err = VerifyRunAction(strings.Join(parts, "."), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestVerifyRunActionRefusesAMalformedEnvelope(t *testing.T) {
	signer := newTestSigner("run-malformed")
	keys := KeySet{signer.id: signer.public()}
	valid := signEnvelope(t, RunEnvelopeVersion, signer, wellFormedRunClaims(verifyNow))
	parts := strings.Split(valid, ".")

	// A complete run payload with one field Control's decoder does not know. It is refused by the strict decoder alone.
	var withExtra map[string]any
	complete, err := json.Marshal(wellFormedRunClaims(verifyNow))
	if err != nil {
		t.Fatalf("marshal run claims: %v", err)
	}
	if err := json.Unmarshal(complete, &withExtra); err != nil {
		t.Fatalf("decode run claims: %v", err)
	}
	withExtra["space_kind"] = "room"
	extraPayload, err := json.Marshal(withExtra)
	if err != nil {
		t.Fatalf("marshal run claims with an extra field: %v", err)
	}

	cases := map[string]string{
		"empty":              "",
		"two parts":          RunEnvelopeVersion + "." + parts[1],
		"five parts":         valid + ".extra",
		"unknown version":    "run-action-v9." + parts[1] + "." + parts[2] + "." + parts[3],
		"undecodable key id": RunEnvelopeVersion + ".!!!." + parts[2] + "." + parts[3],
		"payload not json":   signRaw(RunEnvelopeVersion, signer, []byte("not json")),
		"unknown field":      signRaw(RunEnvelopeVersion, signer, extraPayload),
	}
	for name, token := range cases {
		t.Run(name, func(t *testing.T) {
			_, err := VerifyRunAction(token, keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

// TestVerifyRunActionRefusesAHumanV2Envelope signs the run claims under the human version. The same claims verify under the
// run-action version, so the version alone decides.
func TestVerifyRunActionRefusesAHumanV2Envelope(t *testing.T) {
	signer := newTestSigner("run-v2")
	keys := KeySet{signer.id: signer.public()}
	claims := wellFormedRunClaims(verifyNow)

	if _, err := VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, signer, claims), keys, verifyNow); err != nil {
		t.Fatalf("control: the run-action envelope with these claims was refused: %v", err)
	}
	_, err := VerifyRunAction(signEnvelope(t, EnvelopeVersion, signer, claims), keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestVerifyRunActionRefusesIncompleteClaims(t *testing.T) {
	signer := newTestSigner("run-incomplete")
	keys := KeySet{signer.id: signer.public()}
	cases := map[string]func(*RunClaims){
		"run id":        func(c *RunClaims) { c.RunID = "" },
		"thread":        func(c *RunClaims) { c.ThreadID = " " },
		"run context":   func(c *RunClaims) { c.RunContextAuthorizationRef = "" },
		"no permission": func(c *RunClaims) { c.Permissions = nil },
		"two permissions": func(c *RunClaims) {
			c.Permissions = []string{PermissionCanvasRead, PermissionCanvasWrite}
		},
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			claims := wellFormedRunClaims(verifyNow)
			mutate(&claims)
			_, err := VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, signer, claims), keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

func TestVerifyRunActionRefusesNonPositiveRevisions(t *testing.T) {
	signer := newTestSigner("run-revisions")
	keys := KeySet{signer.id: signer.public()}
	cases := map[string]func(*RunClaims){
		"authority zero":    func(c *RunClaims) { c.AuthorityRevision = 0 },
		"audience negative": func(c *RunClaims) { c.RecipientAudienceRevision = -1 },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			claims := wellFormedRunClaims(verifyNow)
			mutate(&claims)
			_, err := VerifyRunAction(signEnvelope(t, RunEnvelopeVersion, signer, claims), keys, verifyNow)
			requireRefusal(t, err, CodeDecisionInvalid)
		})
	}
}

func TestVerifyRunActionRefusesAnOversizeEnvelope(t *testing.T) {
	signer := newTestSigner("run-oversize")
	keys := KeySet{signer.id: signer.public()}

	padded := wellFormedRunClaims(verifyNow)
	padded.Purpose = strings.Repeat("p", MaxEnvelopeBytes)
	token := signEnvelope(t, RunEnvelopeVersion, signer, padded)
	if len(token) <= MaxEnvelopeBytes {
		t.Fatalf("fixture is %d bytes, want more than %d", len(token), MaxEnvelopeBytes)
	}
	_, err := VerifyRunAction(token, keys, verifyNow)
	requireRefusal(t, err, CodeDecisionInvalid)
}

func TestCheckRunCanvasRefusesAnotherCallerOrAuthority(t *testing.T) {
	now := verifyNow
	if err := wellFormedRunClaims(now).CheckCanvas("org-1", "user-2", "space-room-1", false); err != nil {
		t.Fatalf("a run read for its own route was refused: %v", err)
	}
	if err := wellFormedRunWriteClaims(now).CheckCanvas("org-1", "user-2", "space-room-1", true); err != nil {
		t.Fatalf("a run write for its own route was refused: %v", err)
	}

	cases := map[string]struct {
		claims func() RunClaims
		check  func(RunClaims) error
	}{
		"another org": {
			claims: func() RunClaims { return wellFormedRunClaims(now) },
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-other", "user-2", "space-room-1", false)
			},
		},
		"another Space": {
			claims: func() RunClaims { return wellFormedRunClaims(now) },
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-other", false)
			},
		},
		"another subject": {
			claims: func() RunClaims { return wellFormedRunClaims(now) },
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-other", "space-room-1", false)
			},
		},
		"another audience": {
			claims: func() RunClaims {
				c := wellFormedRunClaims(now)
				c.ServiceAudience = "application-plane-convex-core"
				return c
			},
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
		},
		"read run on a write route": {
			claims: func() RunClaims { return wellFormedRunClaims(now) },
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", true)
			},
		},
		"another action": {
			claims: func() RunClaims {
				c := wellFormedRunClaims(now)
				c.ActionID = "space.thread.rename"
				return c
			},
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
		},
		"another schema": {
			claims: func() RunClaims {
				c := wellFormedRunClaims(now)
				c.ActionSchemaHash = SchemaRunCanvasWrite
				return c
			},
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
		},
		"missing canvas permission": {
			claims: func() RunClaims {
				c := wellFormedRunClaims(now)
				c.Permissions = []string{"space:read"}
				return c
			},
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
		},
		"zero data retention": {
			claims: func() RunClaims {
				c := wellFormedRunClaims(now)
				c.ZeroDataRetention = true
				return c
			},
			check: func(c RunClaims) error {
				return c.CheckCanvas("org-1", "user-2", "space-room-1", false)
			},
		},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			requireRefusal(t, tc.check(tc.claims()), CodeDecisionMismatch)
		})
	}
}

// TestRunPayloadDigestMatchesTheRustPayloadDigest recomputes each golden digest vector. Its arguments and digest come from
// the registry, which the Rust payload_digest reproduces in space_self_api.rs. The registry holds one vector today, for
// space.thread.rename; no vector covers space.canvas.write yet.
func TestRunPayloadDigestMatchesTheRustPayloadDigest(t *testing.T) {
	raw, err := os.ReadFile(runGoldenPath)
	if err != nil {
		t.Fatalf("read the Space self-API registry golden: %v", err)
	}
	var golden struct {
		DigestVectors []struct {
			Name           string     `json:"name"`
			ActionID       string     `json:"action_id"`
			RunID          string     `json:"run_id"`
			OrgID          string     `json:"org_id"`
			IdempotencyKey string     `json:"idempotency_key"`
			ArgumentPairs  [][]string `json:"argument_pairs"`
			PayloadDigest  string     `json:"payload_digest"`
		} `json:"digest_vectors"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode the Space self-API registry golden: %v", err)
	}
	if len(golden.DigestVectors) == 0 {
		t.Fatal("the registry golden carries no digest vector to check the run payload digest against")
	}
	for _, vector := range golden.DigestVectors {
		t.Run(vector.Name, func(t *testing.T) {
			args := make([]RunArgument, 0, len(vector.ArgumentPairs))
			for _, pair := range vector.ArgumentPairs {
				if len(pair) != 2 {
					t.Fatalf("argument pair %v is not name and value", pair)
				}
				args = append(args, RunArgument{Name: pair[0], Value: pair[1]})
			}
			if got := RunPayloadDigest(vector.ActionID, vector.RunID, vector.OrgID, vector.IdempotencyKey, args); got != vector.PayloadDigest {
				t.Fatalf("run payload digest = %s, golden %s", got, vector.PayloadDigest)
			}
		})
	}
}
