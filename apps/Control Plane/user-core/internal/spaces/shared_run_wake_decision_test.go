package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

// WS08-S1: the run-wake decision.

func validRunWakeEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.Kind = KindRoom
	evidence.Membership.SpaceRef = "room-1"
	evidence.Membership.Role = "editor"
	evidence.RecipientAudienceRef = "space:room-1:recipient-audience:4"
	evidence.RecipientAudienceHash = "sha256:shared-audience"
	evidence.Membership.Revisions.RecipientAudience = 4
	evidence.ResourceAuthorizationRef = "control:room-1:run-wake:7"
	return evidence
}

func validRunWakeRoster() []RosterMember {
	return []RosterMember{
		{SubjectType: "user", SubjectID: "user-1", Role: "editor"},
		{SubjectType: "service", SubjectID: "agent-7", Role: "editor"},
	}
}

func validRunWakeRequest() RunWakeDecisionRequest {
	return RunWakeDecisionRequest{
		DecisionRef: "wake-decision-1", ThreadID: "thread-1", TriggerMessageID: "message-9",
		ContentDigest:  "sha256:" + strings.Repeat("a", 64),
		AgentSubjectID: "agent-7", TurnOrigin: TurnOriginDirect, IdempotencyKey: "wake-1", Nonce: "nonce-1",
	}
}

func TestIssueRunWakeDecisionAdmitsEditorManagerAndOwner(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)
	for _, role := range []string{"editor", "manager", "owner"} {
		evidence := validRunWakeEvidence()
		evidence.Membership.Role = role
		decision, err := IssueRunWakeDecision(evidence, validRunWakeRoster(), validRunWakeRequest(), now)
		if err != nil {
			t.Fatalf("%s: %v", role, err)
		}
		if decision.ActionID != "model.run.wake" || decision.ActionSchemaHash != "sha256:run-wake-v1" ||
			decision.ServiceAudience != "model-plane" {
			t.Fatalf("%s: target = %+v", role, decision)
		}
		if decision.ExpiresAt.IsZero() || !decision.ExpiresAt.After(now) {
			t.Fatalf("%s: decision must expire after issue", role)
		}
	}
}

func TestIssueRunWakeDecisionRefusals(t *testing.T) {
	now := time.Now().UTC()
	cases := []struct {
		name   string
		mutate func(*PersonalThreadDecisionEvidence, *[]RosterMember, *RunWakeDecisionRequest)
	}{
		{"viewer", func(e *PersonalThreadDecisionEvidence, _ *[]RosterMember, _ *RunWakeDecisionRequest) {
			e.Membership.Role = "viewer"
		}},
		{"missing recipient audience (non-recipient)", func(e *PersonalThreadDecisionEvidence, _ *[]RosterMember, _ *RunWakeDecisionRequest) {
			e.RecipientAudienceRef, e.RecipientAudienceHash = "", ""
		}},
		{"missing thread_create entitlement", func(e *PersonalThreadDecisionEvidence, _ *[]RosterMember, _ *RunWakeDecisionRequest) {
			e.ThreadCreateEntitled = false
		}},
		{"missing authorization ref", func(e *PersonalThreadDecisionEvidence, _ *[]RosterMember, _ *RunWakeDecisionRequest) {
			e.ResourceAuthorizationRef = ""
		}},
		{"agent not on the roster", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.AgentSubjectID = "agent-other"
		}},
		{"agent is a user, not a service", func(_ *PersonalThreadDecisionEvidence, r *[]RosterMember, _ *RunWakeDecisionRequest) {
			*r = []RosterMember{{SubjectType: "user", SubjectID: "agent-7", Role: "editor"}}
		}},
		{"empty roster", func(_ *PersonalThreadDecisionEvidence, r *[]RosterMember, _ *RunWakeDecisionRequest) {
			*r = nil
		}},
		{"ambient origin", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.TurnOrigin = TurnOriginAmbient
		}},
		{"automation origin", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.TurnOrigin = TurnOriginAutomation
		}},
		{"unknown origin", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.TurnOrigin = "telepathy"
		}},
		{"short content digest", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.ContentDigest = "sha256:abc"
		}},
		{"unprefixed content digest", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.ContentDigest = strings.Repeat("a", 71)
		}},
		{"missing trigger", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.TriggerMessageID = " "
		}},
		{"missing idempotency key", func(_ *PersonalThreadDecisionEvidence, _ *[]RosterMember, r *RunWakeDecisionRequest) {
			r.IdempotencyKey = ""
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			evidence, roster, request := validRunWakeEvidence(), validRunWakeRoster(), validRunWakeRequest()
			tc.mutate(&evidence, &roster, &request)
			if _, err := IssueRunWakeDecision(evidence, roster, request, now); err == nil {
				t.Fatal("expected a refusal")
			}
		})
	}
	if _, err := IssueRunWakeDecision(validRunWakeEvidence(), validRunWakeRoster(), validRunWakeRequest(), time.Time{}); err == nil {
		t.Fatal("a zero clock must be refused")
	}
}

func TestRunWakePermissionsAreExactlyRunWakeAndDisjointFromThreadPermissions(t *testing.T) {
	decision, err := IssueRunWakeDecision(validRunWakeEvidence(), validRunWakeRoster(), validRunWakeRequest(), time.Now().UTC())
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(decision.Permissions, []string{"run:wake"}) || RunWakePermission != "run:wake" {
		t.Fatalf("permissions = %#v", decision.Permissions)
	}
	for _, p := range decision.Permissions {
		if strings.HasPrefix(p, "thread:") {
			t.Fatalf("a wake token must carry no thread permission, got %q", p)
		}
	}
	if decision.ActionID == threadAppendAction || decision.ActionSchemaHash == threadAppendSchema {
		t.Fatal("a wake must not reuse the append effect")
	}
}

func TestRunWakeDigestBindsEveryOperationField(t *testing.T) {
	now := time.Now().UTC()
	base, err := IssueRunWakeDecision(validRunWakeEvidence(), validRunWakeRoster(), validRunWakeRequest(), now)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(base.PayloadDigest, "sha256:") {
		t.Fatalf("digest = %q", base.PayloadDigest)
	}
	roster := append(validRunWakeRoster(), RosterMember{SubjectType: "service", SubjectID: "agent-8", Role: "editor"})
	for name, mutate := range map[string]func(*RunWakeDecisionRequest){
		"trigger id":      func(r *RunWakeDecisionRequest) { r.TriggerMessageID = "message-10" },
		"content digest":  func(r *RunWakeDecisionRequest) { r.ContentDigest = "sha256:" + strings.Repeat("b", 64) },
		"agent":           func(r *RunWakeDecisionRequest) { r.AgentSubjectID = "agent-8" },
		"thread id":       func(r *RunWakeDecisionRequest) { r.ThreadID = "thread-2" },
		"idempotency key": func(r *RunWakeDecisionRequest) { r.IdempotencyKey = "wake-2" },
		"decision ref":    func(r *RunWakeDecisionRequest) { r.DecisionRef = "wake-decision-2" },
	} {
		request := validRunWakeRequest()
		mutate(&request)
		changed, err := IssueRunWakeDecision(validRunWakeEvidence(), roster, request, now)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		if changed.PayloadDigest == base.PayloadDigest {
			t.Errorf("changing the %s did not change the digest", name)
		}
	}
	// Origin is bound too: it is validated as direct only, so test the digest
	// function directly with a non-direct value.
	origin := validRunWakeRequest()
	origin.TurnOrigin = TurnOriginAmbient
	if runWakePayloadDigest(validRunWakeEvidence(), origin) == base.PayloadDigest {
		t.Error("changing the origin did not change the digest")
	}
	// Revisions are bound.
	evidence := validRunWakeEvidence()
	evidence.Membership.Revisions.Authority++
	if runWakePayloadDigest(evidence, validRunWakeRequest()) == base.PayloadDigest {
		t.Error("changing the authority revision did not change the digest")
	}
	evidence = validRunWakeEvidence()
	evidence.Membership.Revisions.RecipientAudience++
	if runWakePayloadDigest(evidence, validRunWakeRequest()) == base.PayloadDigest {
		t.Error("changing the audience revision did not change the digest")
	}
}

func TestRunOriginMappingAndOnlyDirectIsIssuable(t *testing.T) {
	for turn, run := range map[string]string{"direct": "chat", "ambient": "trigger", "automation": "schedule"} {
		got, ok := RunOriginForTurnOrigin(turn)
		if !ok || got != run {
			t.Errorf("%s -> %q %v, want %q", turn, got, ok, run)
		}
		if !ValidTurnOrigin(turn) {
			t.Errorf("%s must be a valid turn origin", turn)
		}
	}
	if _, ok := RunOriginForTurnOrigin("nope"); ok || ValidTurnOrigin("") {
		t.Fatal("an unknown origin must not map")
	}
	if err := ValidateRunWakeTurnOrigin(TurnOriginDirect); err != nil {
		t.Fatal(err)
	}
	for _, origin := range []string{TurnOriginAmbient, TurnOriginAutomation, "x", ""} {
		if ValidateRunWakeTurnOrigin(origin) == nil {
			t.Errorf("%q must not be issuable", origin)
		}
	}
}

// The golden is shared with Session Core. The digest and the mapping table must
// equal the committed vector.
func TestRunWakeDecisionMatchesTheSharedGolden(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("testdata", "run-wake-v1.golden.json"))
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden struct {
		Input struct {
			OrgID                     string `json:"org_id"`
			UserID                    string `json:"user_id"`
			ThreadID                  string `json:"thread_id"`
			SpaceID                   string `json:"space_id"`
			SpaceDecisionRef          string `json:"space_decision_ref"`
			RecipientAudienceRef      string `json:"recipient_audience_ref"`
			RecipientAudienceHash     string `json:"recipient_audience_hash"`
			PrivacyPolicyRef          string `json:"privacy_policy_ref"`
			ResourceAuthorizationRef  string `json:"resource_authorization_ref"`
			TriggerMessageID          string `json:"trigger_message_id"`
			Content                   string `json:"content"`
			AgentSubjectID            string `json:"agent_subject_id"`
			TurnOrigin                string `json:"turn_origin"`
			IdempotencyKey            string `json:"idempotency_key"`
			AuthorityRevision         int64  `json:"authority_revision"`
			RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
		} `json:"input"`
		Expected struct {
			Action        string   `json:"action"`
			Schema        string   `json:"schema"`
			Audience      string   `json:"audience"`
			Permissions   []string `json:"permissions"`
			ContentDigest string   `json:"content_digest"`
			Digest        string   `json:"digest"`
		} `json:"expected"`
		Mapping []struct {
			TurnOrigin string `json:"turn_origin"`
			RunOrigin  string `json:"run_origin"`
			Issued     bool   `json:"issued"`
		} `json:"turn_origin_mapping"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	in := golden.Input

	sum := sha256.Sum256([]byte(in.Content))
	contentDigest := "sha256:" + hex.EncodeToString(sum[:])
	if contentDigest != golden.Expected.ContentDigest {
		t.Fatalf("content digest = %s, golden %s", contentDigest, golden.Expected.ContentDigest)
	}

	evidence := PersonalThreadDecisionEvidence{
		Membership: CurrentMembership{
			SpaceRef: in.SpaceID, OrgID: in.OrgID, SubjectID: in.UserID, Kind: KindRoom, Role: "editor",
			Revisions: AuthorityRevision{
				Authority: in.AuthorityRevision, Membership: 1, Privacy: 5,
				RecipientAudience: in.RecipientAudienceRevision, Entitlement: 1,
			},
		},
		RecipientAudienceRef: in.RecipientAudienceRef, RecipientAudienceHash: in.RecipientAudienceHash,
		ThreadCreateEntitled: true, ResourceAuthorizationRef: in.ResourceAuthorizationRef,
		Privacy: PrivacyPolicySnapshot{
			PolicyRef: in.PrivacyPolicyRef, Purpose: "assistant_collaboration", LawfulBasis: "contract",
			PrivacyClass: "internal", RetentionClass: "standard", Residency: "swedencentral", DeletionScope: "space",
		},
	}
	request := RunWakeDecisionRequest{
		DecisionRef: in.SpaceDecisionRef, ThreadID: in.ThreadID, TriggerMessageID: in.TriggerMessageID,
		ContentDigest: contentDigest, AgentSubjectID: in.AgentSubjectID, TurnOrigin: in.TurnOrigin,
		IdempotencyKey: in.IdempotencyKey, Nonce: "nonce-1",
	}
	roster := []RosterMember{{SubjectType: "service", SubjectID: in.AgentSubjectID, Role: "editor"}}
	decision, err := IssueRunWakeDecision(evidence, roster, request, time.Now().UTC())
	if err != nil {
		t.Fatalf("IssueRunWakeDecision: %v", err)
	}
	if decision.PayloadDigest != golden.Expected.Digest {
		t.Fatalf("digest = %s, golden %s (change only on an intended wire change, on both sides)", decision.PayloadDigest, golden.Expected.Digest)
	}
	if decision.ActionID != golden.Expected.Action || decision.ActionSchemaHash != golden.Expected.Schema ||
		decision.ServiceAudience != golden.Expected.Audience || !reflect.DeepEqual(decision.Permissions, golden.Expected.Permissions) {
		t.Fatalf("decision target = %+v, golden %+v", decision, golden.Expected)
	}

	if len(golden.Mapping) != len(turnOriginRunOrigin) {
		t.Fatalf("mapping rows = %d, golden %d", len(turnOriginRunOrigin), len(golden.Mapping))
	}
	for _, row := range golden.Mapping {
		got, ok := RunOriginForTurnOrigin(row.TurnOrigin)
		if !ok || got != row.RunOrigin {
			t.Errorf("%s -> %q, golden %q", row.TurnOrigin, got, row.RunOrigin)
		}
		if issued := ValidateRunWakeTurnOrigin(row.TurnOrigin) == nil; issued != row.Issued {
			t.Errorf("%s issued = %v, golden %v", row.TurnOrigin, issued, row.Issued)
		}
	}
}
