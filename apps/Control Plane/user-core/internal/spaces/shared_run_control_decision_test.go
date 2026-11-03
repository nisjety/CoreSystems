package spaces

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// WS09-S7: the room run-control (cancel) decision.

func validRunControlEvidence() PersonalThreadDecisionEvidence {
	evidence := validSharedThreadReadEvidence()
	evidence.Membership.Role = "manager"
	evidence.ThreadReadEntitled = false
	evidence.RunControlEntitled = true
	evidence.ResourceAuthorizationRef = "control:room-1:run-control:7"
	return evidence
}

func validRunControlRequest() RunControlDecisionRequest {
	return RunControlDecisionRequest{
		DecisionRef: "control-decision-1", RunID: "run-1", Operation: RunControlOperationCancel,
		IdempotencyKey: "control-1", Nonce: "nonce-1",
	}
}

func TestIssueSharedRunControlDecisionAdmitsManagerAndOwner(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)
	for _, role := range []string{"manager", "owner"} {
		evidence := validRunControlEvidence()
		evidence.Membership.Role = role
		decision, err := IssueSharedRunControlDecision(evidence, validRunControlRequest(), now)
		if err != nil {
			t.Fatalf("%s: %v", role, err)
		}
		if decision.ActionID != "model.run.control" || decision.ActionSchemaHash != "sha256:run-control-v1" ||
			decision.ServiceAudience != "model-plane" {
			t.Fatalf("%s: target = %+v", role, decision)
		}
		if !reflect.DeepEqual(decision.Permissions, []string{"run:cancel"}) || RunControlPermission != "run:cancel" {
			t.Fatalf("%s: permissions = %#v", role, decision.Permissions)
		}
		if !decision.ExpiresAt.After(now) {
			t.Fatalf("%s: decision must expire after issue", role)
		}
	}
}

func TestIssueSharedRunControlDecisionRefusals(t *testing.T) {
	now := time.Now().UTC()
	cases := []struct {
		name   string
		mutate func(*PersonalThreadDecisionEvidence, *RunControlDecisionRequest)
		code   DenialCode
	}{
		{"editor", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) { e.Membership.Role = "editor" }, DenialRoleBelowFloor},
		{"viewer", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) { e.Membership.Role = "viewer" }, DenialRoleBelowFloor},
		{"personal Space", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) {
			e.Membership.Kind = KindPersonal
		}, DenialKindUnsupported},
		{"missing entitlement", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) {
			e.RunControlEntitled = false
		}, DenialEntitlementOff},
		{"thread_create is not run control", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) {
			e.RunControlEntitled = false
			e.ThreadCreateEntitled = true
			e.ThreadReadEntitled = true
		}, DenialEntitlementOff},
		{"not in the current audience", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) {
			e.RecipientAudienceRef, e.RecipientAudienceHash = "", ""
		}, ""},
		{"missing authorization ref", func(e *PersonalThreadDecisionEvidence, _ *RunControlDecisionRequest) {
			e.ResourceAuthorizationRef = ""
		}, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			evidence, request := validRunControlEvidence(), validRunControlRequest()
			tc.mutate(&evidence, &request)
			_, err := IssueSharedRunControlDecision(evidence, request, now)
			if err == nil {
				t.Fatal("expected a refusal")
			}
			if tc.code != "" {
				denial, ok := AsDenial(err)
				if !ok || denial.Code != tc.code {
					t.Fatalf("refusal = %v, want denial %s", err, tc.code)
				}
			}
		})
	}
}

func TestRunControlRequestValidation(t *testing.T) {
	now := time.Now().UTC()
	for name, mutate := range map[string]func(*RunControlDecisionRequest){
		"no run id":        func(r *RunControlDecisionRequest) { r.RunID = " " },
		"no operation":     func(r *RunControlDecisionRequest) { r.Operation = "" },
		"pause":            func(r *RunControlDecisionRequest) { r.Operation = "pause" },
		"resume":           func(r *RunControlDecisionRequest) { r.Operation = "resume" },
		"upper-case":       func(r *RunControlDecisionRequest) { r.Operation = "CANCEL" },
		"no idempotency":   func(r *RunControlDecisionRequest) { r.IdempotencyKey = "" },
		"no nonce":         func(r *RunControlDecisionRequest) { r.Nonce = "" },
		"no decision ref":  func(r *RunControlDecisionRequest) { r.DecisionRef = "" },
		"oversized run id": func(r *RunControlDecisionRequest) { r.RunID = strings.Repeat("r", maxRunControlRunIDLen+1) },
	} {
		request := validRunControlRequest()
		mutate(&request)
		if _, err := IssueSharedRunControlDecision(validRunControlEvidence(), request, now); err == nil {
			t.Errorf("%s: expected a refusal", name)
		}
	}
	if _, err := IssueSharedRunControlDecision(validRunControlEvidence(), validRunControlRequest(), time.Time{}); err == nil {
		t.Error("a zero clock must be refused")
	}
	edge := validRunControlRequest()
	edge.RunID = strings.Repeat("r", maxRunControlRunIDLen)
	if _, err := IssueSharedRunControlDecision(validRunControlEvidence(), edge, now); err != nil {
		t.Errorf("a run id at the limit must pass: %v", err)
	}
}

func TestRunControlDigestBindsTheRunAndTheOperation(t *testing.T) {
	now := time.Now().UTC()
	base, err := IssueSharedRunControlDecision(validRunControlEvidence(), validRunControlRequest(), now)
	if err != nil {
		t.Fatal(err)
	}
	other := validRunControlRequest()
	other.RunID = "run-2"
	changed, err := IssueSharedRunControlDecision(validRunControlEvidence(), other, now)
	if err != nil {
		t.Fatal(err)
	}
	if changed.PayloadDigest == base.PayloadDigest {
		t.Error("changing the run id must change the digest")
	}
	// Operation is only issuable as cancel, so bind-check the digest function.
	op := validRunControlRequest()
	op.Operation = "pause"
	if runControlPayloadDigest(validRunControlEvidence(), op) == base.PayloadDigest {
		t.Error("changing the operation must change the digest")
	}
	idem := validRunControlRequest()
	idem.IdempotencyKey = "control-2"
	if runControlPayloadDigest(validRunControlEvidence(), idem) == base.PayloadDigest {
		t.Error("changing the idempotency key must change the digest")
	}
	evidence := validRunControlEvidence()
	evidence.Membership.Revisions.Authority++
	if runControlPayloadDigest(evidence, validRunControlRequest()) == base.PayloadDigest {
		t.Error("changing the authority revision must change the digest")
	}
	// Whitespace around the run id does not mint a second digest.
	padded := validRunControlRequest()
	padded.RunID = "  run-1 "
	if runControlPayloadDigest(validRunControlEvidence(), padded) != base.PayloadDigest {
		t.Error("run id is trimmed before it is bound")
	}
}

func TestRunControlDecisionCarriesTheZDRClaimAndIsDisjointFromThreadAuthority(t *testing.T) {
	evidence := validRunControlEvidence()
	evidence.Privacy.ZeroDataRetention = true
	decision, err := IssueSharedRunControlDecision(evidence, validRunControlRequest(), time.Now().UTC())
	if err != nil {
		t.Fatalf("a run-control decision is issuable under ZDR: %v", err)
	}
	if !decision.ZeroDataRetention {
		t.Fatal("decision must carry the ZDR claim")
	}
	for _, p := range decision.Permissions {
		if strings.HasPrefix(p, "thread:") || p == RunWakePermission {
			t.Fatalf("a control token must carry no thread or wake permission, got %q", p)
		}
	}
	if decision.ActionID == threadReadAction || decision.ActionSchemaHash == threadReadSchema ||
		decision.ActionID == runWakeAction || decision.ActionSchemaHash == runWakeSchema {
		t.Fatal("a control decision must not reuse the read or wake effect")
	}
	if !strings.Contains(decision.ResourceAuthorizationRef, ":run-control:") {
		t.Fatalf("resource ref = %q", decision.ResourceAuthorizationRef)
	}
}

func TestRunControlEntitlementCatalogueRow(t *testing.T) {
	spec, ok := EntitlementSpecByKey("run_control")
	if !ok {
		t.Fatal("the catalogue has no run_control row")
	}
	if spec.Column != "run_control_entitled" || !reflect.DeepEqual(spec.Decisions, []string{"model.run.control"}) || spec.ZDR != ZDRCompatible {
		t.Fatalf("run_control row = %+v", spec)
	}
	var policy EffectPolicy
	if spec.Get(policy) {
		t.Fatal("the default policy bit must be false (deny by default)")
	}
	spec.Set(&policy, true)
	if !policy.RunControlEntitled || !EntitlementValues(policy)["run_control"] {
		t.Fatal("Set must write RunControlEntitled")
	}
	// It is independent of the thread bits.
	if policy.ThreadCreateEntitled || policy.ThreadReadEntitled {
		t.Fatal("run_control must not imply thread_create or thread_read")
	}
}

func TestMigration038DefaultsRunControlOffAndIsReversible(t *testing.T) {
	up, err := os.ReadFile(filepath.Join("..", "..", "migrations", "038_run_control_entitled.up.sql"))
	if err != nil {
		t.Fatalf("read up: %v", err)
	}
	down, err := os.ReadFile(filepath.Join("..", "..", "migrations", "038_run_control_entitled.down.sql"))
	if err != nil {
		t.Fatalf("read down: %v", err)
	}
	flat := strings.Join(strings.Fields(string(up)), " ")
	if !strings.Contains(flat, "ADD COLUMN IF NOT EXISTS run_control_entitled BOOLEAN NOT NULL DEFAULT FALSE") {
		t.Fatalf("038 up must add run_control_entitled BOOLEAN NOT NULL DEFAULT FALSE idempotently: %s", flat)
	}
	if strings.Contains(flat, "DEFAULT TRUE") {
		t.Fatal("038 must never default the bit on")
	}
	if !strings.Contains(strings.Join(strings.Fields(string(down)), " "), "DROP COLUMN IF EXISTS run_control_entitled") {
		t.Fatalf("038 down must drop the column: %s", down)
	}
}

// runControlGolden is the shared Control/Session Core vector (WS09-S8 asserts
// the same file).
type runControlGolden struct {
	Input struct {
		OrgID                     string `json:"org_id"`
		UserID                    string `json:"user_id"`
		SpaceID                   string `json:"space_id"`
		SpaceDecisionRef          string `json:"space_decision_ref"`
		RecipientAudienceRef      string `json:"recipient_audience_ref"`
		RecipientAudienceHash     string `json:"recipient_audience_hash"`
		PrivacyPolicyRef          string `json:"privacy_policy_ref"`
		ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
		RunID                     string `json:"run_id"`
		Operation                 string `json:"operation"`
		IdempotencyKey            string `json:"idempotency_key"`
		AuthorityRevision         int64  `json:"authority_revision"`
		RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
	} `json:"input"`
	Expected struct {
		Action      string   `json:"action"`
		Schema      string   `json:"schema"`
		Audience    string   `json:"audience"`
		Permissions []string `json:"permissions"`
		Digest      string   `json:"digest"`
	} `json:"expected"`
}

// referenceRunControlDigest is the encoding written out by hand, independent of
// runControlPayloadDigest, so the golden cannot just echo the implementation.
func referenceRunControlDigest(g runControlGolden) string {
	h := sha256.New()
	h.Write([]byte("model.run.control\x00v1\x00"))
	in := g.Input
	for _, f := range [][2]string{
		{"org_id", in.OrgID}, {"user_id", in.UserID}, {"space_id", in.SpaceID},
		{"space_decision_ref", in.SpaceDecisionRef}, {"recipient_audience_ref", in.RecipientAudienceRef},
		{"recipient_audience_hash", in.RecipientAudienceHash}, {"privacy_policy_ref", in.PrivacyPolicyRef},
		{"resource_authorization_ref", in.ResourceAuthorizationRef}, {"run_id", in.RunID},
		{"operation", in.Operation}, {"action_schema_hash", g.Expected.Schema}, {"idempotency_key", in.IdempotencyKey},
	} {
		h.Write([]byte(f[0]))
		h.Write([]byte{0})
		var n [8]byte
		binary.BigEndian.PutUint64(n[:], uint64(len(f[1])))
		h.Write(n[:])
		h.Write([]byte(f[1]))
	}
	for _, r := range []struct {
		name  string
		value int64
	}{{"authority_revision", in.AuthorityRevision}, {"recipient_audience_revision", in.RecipientAudienceRevision}} {
		h.Write([]byte(r.name))
		h.Write([]byte{0})
		var n [8]byte
		binary.BigEndian.PutUint64(n[:], uint64(r.value))
		h.Write(n[:])
	}
	return "sha256:" + hex.EncodeToString(h.Sum(nil))
}

func TestRunControlDecisionMatchesTheSharedGolden(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "run-control-v1.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden runControlGolden
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	in := golden.Input

	if got := referenceRunControlDigest(golden); got != golden.Expected.Digest {
		t.Fatalf("golden digest %s does not match the reference encoding %s", golden.Expected.Digest, got)
	}

	evidence := PersonalThreadDecisionEvidence{
		Membership: CurrentMembership{
			SpaceRef: in.SpaceID, OrgID: in.OrgID, SubjectID: in.UserID, Kind: KindRoom, Role: "manager",
			Revisions: AuthorityRevision{
				Authority: in.AuthorityRevision, Membership: 1, Privacy: 5,
				RecipientAudience: in.RecipientAudienceRevision, Entitlement: 1,
			},
		},
		RecipientAudienceRef: in.RecipientAudienceRef, RecipientAudienceHash: in.RecipientAudienceHash,
		RunControlEntitled: true, ResourceAuthorizationRef: in.ResourceAuthorizationRef,
		Privacy: PrivacyPolicySnapshot{
			PolicyRef: in.PrivacyPolicyRef, Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
	decision, err := IssueSharedRunControlDecision(evidence, RunControlDecisionRequest{
		DecisionRef: in.SpaceDecisionRef, RunID: in.RunID, Operation: in.Operation,
		IdempotencyKey: in.IdempotencyKey, Nonce: "nonce-1",
	}, time.Now().UTC())
	if err != nil {
		t.Fatalf("IssueSharedRunControlDecision: %v", err)
	}
	if decision.PayloadDigest != golden.Expected.Digest {
		t.Fatalf("digest = %s, golden %s (change only on an intended wire change, on both sides)", decision.PayloadDigest, golden.Expected.Digest)
	}
	if decision.ActionID != golden.Expected.Action || decision.ActionSchemaHash != golden.Expected.Schema ||
		decision.ServiceAudience != golden.Expected.Audience || !reflect.DeepEqual(decision.Permissions, golden.Expected.Permissions) {
		t.Fatalf("decision target = %+v, golden %+v", decision, golden.Expected)
	}

	// Session Core's copy, when this checkout has it, must be the same bytes
	// (line endings normalized: autocrlf rewrites checkouts on Windows).
	rustCopy := filepath.Join("..", "..", "..", "..", "Model Plane", "rust", "services", "session-core", "tests", "fixtures", "run-control-v1.golden.json")
	if rust, err := os.ReadFile(rustCopy); err == nil {
		if !bytes.Equal(normalizeEOL(rust), normalizeEOL(raw)) {
			t.Fatal("Session Core's copy of run-control-v1.golden.json differs from Control's")
		}
	}
}
