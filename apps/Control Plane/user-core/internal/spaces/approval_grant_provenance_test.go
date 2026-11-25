package spaces

import (
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

// WS10-S2: standing-rule provenance, the grant-event audit export and the
// decision-audit registry of the approval routes. Pure logic only; the
// repository writes and their audit rows are covered by the live test.

// provenanceAuthority is Session Core's projection of a granted approval that the
// caller (user-1, a manager of space-room in org-1) requested. Each case changes
// only the field its rule is about.
func provenanceAuthority() ApprovalDecideAuthority {
	return ApprovalDecideAuthority{
		ApprovalID: "approval-1", OrgID: "org-1", SpaceRef: "space-room", RunID: "run-1",
		RequesterSubjectID: "user-1", AddresseeKind: ApprovalAddresseeRequester, State: "granted",
	}
}

func TestVerifyStandingRuleProvenanceAcceptsAGrantedApprovalTheCallerRequested(t *testing.T) {
	if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", provenanceAuthority()); err != nil {
		t.Fatalf("granted approval refused: %v", err)
	}
}

func TestVerifyStandingRuleProvenanceAcceptsTheAddresseeOfAGrantedApproval(t *testing.T) {
	named := provenanceAuthority()
	named.RequesterSubjectID, named.AddresseeKind, named.AddresseeSubjectID = "user-2", ApprovalAddresseeNamed, "user-1"
	if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", named); err != nil {
		t.Fatalf("named addressee refused: %v", err)
	}
	managers := provenanceAuthority()
	managers.RequesterSubjectID, managers.AddresseeKind = "user-2", ApprovalAddresseeAnyManager
	if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", managers); err != nil {
		t.Fatalf("any_manager addressee refused: %v", err)
	}
}

func TestVerifyStandingRuleProvenanceHidesApprovalsTheCallerCannotSee(t *testing.T) {
	// Another member's approval routed to a third member is not the caller's, so it
	// answers not-found whatever its state; a grant state is never disclosed to it.
	hidden := provenanceAuthority()
	hidden.RequesterSubjectID, hidden.AddresseeKind, hidden.AddresseeSubjectID = "user-2", ApprovalAddresseeNamed, "user-3"
	for _, state := range []string{"granted", "requested", "denied"} {
		hidden.State = state
		if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", hidden); !errors.Is(err, ErrApprovalNotFound) {
			t.Fatalf("state %s: err = %v, want ErrApprovalNotFound", state, err)
		}
	}
	// An any_editor approval is not the caller's to see as a viewer.
	editors := provenanceAuthority()
	editors.RequesterSubjectID, editors.AddresseeKind = "user-2", ApprovalAddresseeAnyEditor
	if err := VerifyStandingRuleProvenance(approvalEvidence("viewer"), "approval-1", editors); !errors.Is(err, ErrApprovalNotFound) {
		t.Fatalf("a viewer saw an any_editor approval: %v", err)
	}
}

func TestVerifyStandingRuleProvenanceGivesTheSameNotFoundAcrossSpacesAndOrgs(t *testing.T) {
	other := provenanceAuthority()
	other.SpaceRef = "space-other"
	if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", other); !errors.Is(err, ErrApprovalNotFound) {
		t.Fatalf("another Space: err = %v, want ErrApprovalNotFound", err)
	}
	otherOrg := provenanceAuthority()
	otherOrg.OrgID = "org-2"
	if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", otherOrg); !errors.Is(err, ErrApprovalNotFound) {
		t.Fatalf("another organisation: err = %v, want ErrApprovalNotFound", err)
	}
}

func TestVerifyStandingRuleProvenanceRefusesAnUngrantedApproval(t *testing.T) {
	for _, state := range []string{"requested", "denied", "timed_out"} {
		authority := provenanceAuthority()
		authority.State = state
		if err := VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", authority); !errors.Is(err, ErrApprovalNotGranted) {
			t.Fatalf("state %s: err = %v, want ErrApprovalNotGranted", state, err)
		}
	}
}

func TestVerifyStandingRuleProvenanceNeedsTheApprovalAskedAboutAndAResolvedProjection(t *testing.T) {
	evidence := approvalEvidence("manager")
	for name, mutate := range map[string]func(*ApprovalDecideAuthority, *string){
		"other approval": func(_ *ApprovalDecideAuthority, id *string) { *id = "approval-2" },
		"blank approval": func(_ *ApprovalDecideAuthority, id *string) { *id = " " },
		"no run":         func(a *ApprovalDecideAuthority, _ *string) { a.RunID = "" },
		"no state":       func(a *ApprovalDecideAuthority, _ *string) { a.State = "" },
	} {
		t.Run(name, func(t *testing.T) {
			authority := provenanceAuthority()
			id := "approval-1"
			mutate(&authority, &id)
			if err := VerifyStandingRuleProvenance(evidence, id, authority); !errors.Is(err, ErrApprovalAuthorityNotResolved) {
				t.Fatalf("err = %v, want ErrApprovalAuthorityNotResolved", err)
			}
		})
	}
}

func TestVerifyStandingRuleProvenanceRefusesZeroDataRetention(t *testing.T) {
	authority := provenanceAuthority()
	authority.ZeroDataRetentionRun = true
	wantDenial(t, VerifyStandingRuleProvenance(approvalEvidence("manager"), "approval-1", authority), DenialZDRForbidsEffect)
}

func TestBuildApprovalGrantEventAuditIsContentFreeAndKeyedToTheEvent(t *testing.T) {
	at := time.Date(2026, 11, 25, 10, 0, 0, 0, time.UTC)
	eventID, subject, payload, err := BuildApprovalGrantEventAudit(ApprovalGrantEventAudit{
		EventID: 42, RuleID: "apr_1", OrgID: "org-1", SpaceRef: "space-room",
		Kind: approvalEventUsed, ActorSubjectID: "user-1", RunID: "run-1", OccurredAt: at,
	})
	if err != nil {
		t.Fatalf("render grant event: %v", err)
	}
	if eventID != "space-approval-grant:42" {
		t.Fatalf("event id = %q, want space-approval-grant:42", eventID)
	}
	if subject != AuditSubjectApprovalGrantEvent || !strings.HasPrefix(subject, "verevon.audit.v2.control.user-core.") {
		t.Fatalf("subject = %q is outside user-core's audit authority", subject)
	}
	var row map[string]any
	if err := json.Unmarshal(payload, &row); err != nil {
		t.Fatalf("payload is not JSON: %v", err)
	}
	if row["event"] != "space_approval_grant_event" || row["outcome"] != "ok" ||
		row["event_id"] != eventID || row["user_id"] != "user-1" || row["subject"] != "space:space-room" {
		t.Fatalf("audit row = %v", row)
	}
	details, _ := row["details"].(map[string]any)
	if details["grant_kind"] != approvalEventUsed || details["rule_id"] != "apr_1" || details["run_id"] != "run-1" {
		t.Fatalf("details = %v", details)
	}
	for _, forbidden := range []string{"tool", "schema", "token", "nonce", "argument"} {
		if strings.Contains(string(payload), forbidden) {
			t.Fatalf("grant event audit carries %q: %s", forbidden, payload)
		}
	}
}

func TestBuildApprovalGrantEventAuditNamesTheSystemAndTheDeniedOutcome(t *testing.T) {
	at := time.Date(2026, 11, 25, 10, 0, 0, 0, time.UTC)
	_, _, payload, err := BuildApprovalGrantEventAudit(ApprovalGrantEventAudit{
		EventID: 43, RuleID: "apr_1", OrgID: "org-1", SpaceRef: "space-room",
		Kind: approvalEventRevoked, ActorSubjectID: approvalGrantSystemActor, ReasonCode: "agent_removed", OccurredAt: at,
	})
	if err != nil {
		t.Fatalf("render system revocation: %v", err)
	}
	var row map[string]any
	if err := json.Unmarshal(payload, &row); err != nil {
		t.Fatalf("payload is not JSON: %v", err)
	}
	if _, hasUser := row["user_id"]; hasUser {
		t.Fatalf("a system change names a person: %v", row)
	}
	details, _ := row["details"].(map[string]any)
	if details["actor"] != approvalGrantSystemActor || details["reason_code"] != "agent_removed" {
		t.Fatalf("details = %v", details)
	}

	_, _, payload, err = BuildApprovalGrantEventAudit(ApprovalGrantEventAudit{
		EventID: 44, RuleID: "apr_1", OrgID: "org-1", SpaceRef: "space-room",
		Kind: approvalEventUseDenied, ActorSubjectID: "user-1", OccurredAt: at,
	})
	if err != nil {
		t.Fatalf("render denied use: %v", err)
	}
	if err := json.Unmarshal(payload, &row); err != nil || row["outcome"] != "denied" {
		t.Fatalf("a denied use is not a denied outcome: %v %v", err, row["outcome"])
	}
}

func TestBuildApprovalGrantEventAuditNeedsAnEventSequenceNumber(t *testing.T) {
	if _, _, _, err := BuildApprovalGrantEventAudit(ApprovalGrantEventAudit{
		RuleID: "apr_1", OrgID: "org-1", SpaceRef: "space-room", Kind: approvalEventCreated,
	}); err == nil {
		t.Fatal("a grant event with no sequence number was rendered")
	}
}

func TestApprovalAuthorityRoutesAreRegisteredAsEffectBoundIssuers(t *testing.T) {
	want := map[string]string{
		"/api/v1/internal/spaces/approval-grants":             "approval_grant",
		"/api/v1/internal/spaces/approval-grants/use":         "approval_grant_use",
		"/api/v1/internal/spaces/approval-continuation-check": "approval_continuation",
		"/api/v1/internal/spaces/tool-gate-view":              "tool_gate_view",
	}
	for path, name := range want {
		route, row, ok := DecisionAuditRouteFor(path)
		if !ok || route == "" {
			t.Fatalf("%s is not in the decision audit registry", path)
		}
		if row.Name != name || !row.EffectBound {
			t.Fatalf("%s registers %+v, want %s effect-bound", path, row, name)
		}
	}
}
