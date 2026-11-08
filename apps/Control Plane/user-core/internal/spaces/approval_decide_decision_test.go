package spaces

import (
	"reflect"
	"testing"
	"time"
)

// WS10-S6 / WS10-S7.ctl: the member approval-decide decision and the
// turn_origin rules. All pure.

func TestApprovalScopeAllowedForTurnOrigin(t *testing.T) {
	for _, origin := range []string{"", "  ", TurnOriginDirect, TurnOriginAmbient, TurnOriginAutomation, "something-new"} {
		if !ApprovalScopeAllowedForTurnOrigin(origin, "once") {
			t.Errorf("once must be allowed for origin %q", origin)
		}
	}
	for _, origin := range []string{"", TurnOriginDirect} {
		for _, scope := range []string{"thread", "run"} {
			if !ApprovalScopeAllowedForTurnOrigin(origin, scope) {
				t.Errorf("%s must be allowed for direct/empty origin %q", scope, origin)
			}
		}
	}
	for _, origin := range []string{TurnOriginAmbient, TurnOriginAutomation, "something-new"} {
		for _, scope := range []string{"thread", "run"} {
			if ApprovalScopeAllowedForTurnOrigin(origin, scope) {
				t.Errorf("%s must be refused for origin %q: only once is allowed", scope, origin)
			}
		}
	}
	// "always" is a standing rule, never a decide scope; unknown scopes never pass.
	for _, scope := range []string{"always", "", "forever", "ONCE"} {
		if ApprovalScopeAllowedForTurnOrigin(TurnOriginDirect, scope) {
			t.Errorf("scope %q must never be allowed", scope)
		}
	}
}

func TestDefaultApprovalAddresseeKindForTurnOrigin(t *testing.T) {
	for _, tc := range []struct{ origin, stated, want string }{
		{TurnOriginAmbient, "", ApprovalAddresseeAnyManager},
		{TurnOriginAutomation, "", ApprovalAddresseeAnyManager},
		{"something-new", "", ApprovalAddresseeAnyManager},
		{TurnOriginDirect, "", ApprovalAddresseeRequester},
		{"", "", ApprovalAddresseeRequester},
		{TurnOriginAmbient, ApprovalAddresseeNamed, ApprovalAddresseeNamed},
		{TurnOriginAutomation, ApprovalAddresseeAnyEditor, ApprovalAddresseeAnyEditor},
		{TurnOriginDirect, ApprovalAddresseeAnyManager, ApprovalAddresseeAnyManager},
		{TurnOriginAmbient, " requester ", "requester"},
	} {
		if got := DefaultApprovalAddresseeKindForTurnOrigin(tc.origin, tc.stated); got != tc.want {
			t.Errorf("origin %q stated %q = %q, want %q", tc.origin, tc.stated, got, tc.want)
		}
	}
	if TurnOriginIsDirect("something-new") || !TurnOriginIsDirect("") || !TurnOriginIsDirect(TurnOriginDirect) {
		t.Error("only an empty or direct origin is direct; an unknown origin never gains direct rights")
	}
}

func validApprovalDecideEvidence() PersonalThreadDecisionEvidence {
	evidence := validSharedReviseEvidence()
	evidence.Membership.SpaceRef = "room-1"
	evidence.Membership.Role = "editor"
	evidence.ResourceAuthorizationRef = "control:room-1:approval-decide:7"
	return evidence
}

func validApprovalDecideAuthority() ApprovalDecideAuthority {
	return ApprovalDecideAuthority{
		ApprovalID: "approval-1", OrgID: "org-1", SpaceRef: "room-1", RunID: "run-1",
		RequesterSubjectID: "user-1", AddresseeKind: ApprovalAddresseeRequester,
		TurnOrigin: TurnOriginDirect, State: "requested",
	}
}

func validApprovalDecideRequest() ApprovalDecideRequest {
	return ApprovalDecideRequest{
		DecisionRef: "decision-1", ApprovalID: "approval-1", Decision: ApprovalDecisionGranted,
		Scope: "once", IdempotencyKey: "decide-1", Nonce: "nonce-1",
	}
}

// Shared with mp-contracts tests/space_approvals_digest_golden.rs (same hex,
// same inputs, produced by an independent implementation of the byte layout).
const approvalDecideGoldenDigest = "sha256:686970f46c3e6ac25e6c62d8fbbcd5bc7bde610eac8e885a584495bd7d2ce025"

func TestApprovalDecidePayloadDigestGolden(t *testing.T) {
	got := approvalDecidePayloadDigest("org-1", "user-1", "room-1", validApprovalDecideRequest())
	if got != approvalDecideGoldenDigest {
		t.Fatalf("decide payload digest = %s, want golden %s", got, approvalDecideGoldenDigest)
	}
	decision, err := IssueApprovalDecideDecision(validApprovalDecideEvidence(), validApprovalDecideAuthority(), validApprovalDecideRequest(), time.Now().UTC())
	if err != nil {
		t.Fatalf("issue: %v", err)
	}
	if decision.PayloadDigest != approvalDecideGoldenDigest {
		t.Fatalf("decision digest = %s", decision.PayloadDigest)
	}
	if decision.ActionID != "model.approval.decide" || decision.ActionSchemaHash != "sha256:approval-decide-v1" || decision.ServiceAudience != "model-plane" {
		t.Fatalf("decision identity: %+v", decision)
	}
}

func TestApprovalDecidePayloadDigestBindsDecisionScopeAndApprover(t *testing.T) {
	base := approvalDecidePayloadDigest("org-1", "user-1", "room-1", validApprovalDecideRequest())
	denied := validApprovalDecideRequest()
	denied.Decision = ApprovalDecisionDenied
	thread := validApprovalDecideRequest()
	thread.Scope = "thread"
	other := validApprovalDecideRequest()
	other.ApprovalID = "approval-2"
	nonce := validApprovalDecideRequest()
	nonce.Nonce, nonce.IdempotencyKey = "other", "other"
	for name, got := range map[string]string{
		"decision": approvalDecidePayloadDigest("org-1", "user-1", "room-1", denied),
		"scope":    approvalDecidePayloadDigest("org-1", "user-1", "room-1", thread),
		"approval": approvalDecidePayloadDigest("org-1", "user-1", "room-1", other),
		"approver": approvalDecidePayloadDigest("org-1", "user-2", "room-1", validApprovalDecideRequest()),
		"org":      approvalDecidePayloadDigest("org-2", "user-1", "room-1", validApprovalDecideRequest()),
		"space":    approvalDecidePayloadDigest("org-1", "user-1", "room-2", validApprovalDecideRequest()),
	} {
		if got == base {
			t.Errorf("changing the %s must change the digest", name)
		}
	}
	if approvalDecidePayloadDigest("org-1", "user-1", "room-1", nonce) != base {
		t.Error("nonce and idempotency key are not part of the effect digest")
	}
}

func TestIssueApprovalDecideDecisionIssuesExactlyTheDecidePermission(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)
	for _, role := range []string{"viewer", "editor", "manager", "owner"} {
		evidence := validApprovalDecideEvidence()
		evidence.Membership.Role = role
		authority := validApprovalDecideAuthority()
		// viewers and editors may decide only an approval that names them.
		authority.AddresseeKind = ApprovalAddresseeRequester
		decision, err := IssueApprovalDecideDecision(evidence, authority, validApprovalDecideRequest(), now)
		if err != nil {
			t.Fatalf("role %s: %v", role, err)
		}
		if !reflect.DeepEqual(decision.Permissions, []string{"approval:decide"}) {
			t.Errorf("role %s: permissions = %v, want exactly [approval:decide]", role, decision.Permissions)
		}
	}
}

func TestIssueApprovalDecideDecisionRefusals(t *testing.T) {
	now := time.Date(2026, 11, 2, 12, 0, 0, 0, time.UTC)
	issue := func(mutateE func(*PersonalThreadDecisionEvidence), mutateA func(*ApprovalDecideAuthority), mutateR func(*ApprovalDecideRequest)) (Decision, error) {
		e, a, r := validApprovalDecideEvidence(), validApprovalDecideAuthority(), validApprovalDecideRequest()
		if mutateE != nil {
			mutateE(&e)
		}
		if mutateA != nil {
			mutateA(&a)
		}
		if mutateR != nil {
			mutateR(&r)
		}
		return IssueApprovalDecideDecision(e, a, r, now)
	}
	refused := func(name string, err error) {
		t.Helper()
		if err == nil {
			t.Errorf("%s must be refused", name)
		}
	}

	// Scope rules on a non-direct turn.
	for _, origin := range []string{TurnOriginAutomation, TurnOriginAmbient, "something-new"} {
		for _, scope := range []string{"thread", "run"} {
			_, err := issue(nil, func(a *ApprovalDecideAuthority) {
				a.TurnOrigin = origin
				a.AddresseeKind = ApprovalAddresseeNamed
				a.AddresseeSubjectID = "user-1"
			}, func(r *ApprovalDecideRequest) { r.Scope = scope })
			refused("scope "+scope+" on a "+origin+" approval", err)
		}
	}
	// Once stays possible on an automation approval for an any_manager addressee.
	manager := func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "manager" }
	if _, err := issue(manager, func(a *ApprovalDecideAuthority) { a.TurnOrigin, a.AddresseeKind = TurnOriginAutomation, "" }, nil); err != nil {
		t.Errorf("a manager deciding an automation approval once: %v", err)
	}
	// The default addressee on an automation approval is any_manager, so an editor is not one.
	_, err := issue(nil, func(a *ApprovalDecideAuthority) { a.TurnOrigin, a.AddresseeKind = TurnOriginAutomation, "" }, nil)
	refused("an editor on an automation approval with the any_manager default", err)

	// The caller must be an addressee.
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.RequesterSubjectID = "someone-else" }, nil)
	refused("a caller who is not the requester", err)
	_, err = issue(nil, func(a *ApprovalDecideAuthority) {
		a.AddresseeKind, a.AddresseeSubjectID = ApprovalAddresseeNamed, "someone-else"
	}, nil)
	refused("a caller who is not the named addressee", err)
	if _, err = issue(nil, func(a *ApprovalDecideAuthority) {
		a.AddresseeKind, a.AddresseeSubjectID = ApprovalAddresseeNamed, "user-1"
	}, nil); err != nil {
		t.Errorf("the named addressee: %v", err)
	}
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.AddresseeKind = ApprovalAddresseeAnyManager }, nil)
	refused("an editor when the addressee is any_manager", err)
	_, err = issue(func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "viewer" }, func(a *ApprovalDecideAuthority) { a.AddresseeKind = ApprovalAddresseeAnyEditor }, nil)
	refused("a viewer when the addressee is any_editor", err)
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.AddresseeKind = "bogus" }, nil)
	refused("an unknown addressee kind", err)

	// The approval must belong to the approver's Space and organization.
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.SpaceRef = "room-2" }, nil)
	refused("another Space", err)
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.OrgID = "org-2" }, nil)
	refused("another organization", err)
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.ApprovalID = "approval-9" }, nil)
	refused("another approval", err)

	// The approval must still be open.
	for _, state := range []string{"granted", "denied", "expired", "cancelled", ""} {
		_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.State = state }, nil)
		refused("an approval in state "+state, err)
	}

	// Zero Data Retention fails closed with the typed refusal.
	_, err = issue(func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true }, nil, nil)
	if denial, ok := AsDenial(err); !ok || denial.Code != DenialZDRForbidsEffect {
		t.Errorf("ZDR evidence: want %s, got %v", DenialZDRForbidsEffect, err)
	}
	_, err = issue(nil, func(a *ApprovalDecideAuthority) { a.ZeroDataRetentionRun = true }, nil)
	refused("a ZDR run", err)

	// Evidence refusals: a personal Space, an incomplete audience, a missing reference.
	_, err = issue(func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindPersonal }, nil, nil)
	if denial, ok := AsDenial(err); !ok || denial.Code == "" {
		t.Errorf("personal Space: want a typed denial, got %v", err)
	}
	_, err = issue(func(e *PersonalThreadDecisionEvidence) { e.RecipientAudienceRef = "" }, nil, nil)
	refused("a missing recipient audience", err)
	_, err = issue(func(e *PersonalThreadDecisionEvidence) { e.ResourceAuthorizationRef = "" }, nil, nil)
	refused("a missing resource reference", err)
	_, err = issue(func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = "guest" }, nil, nil)
	refused("an unknown role", err)

	// Request shape.
	for name, mutate := range map[string]func(*ApprovalDecideRequest){
		"unknown decision": func(r *ApprovalDecideRequest) { r.Decision = "maybe" },
		"always scope":     func(r *ApprovalDecideRequest) { r.Scope = "always" },
		"empty nonce":      func(r *ApprovalDecideRequest) { r.Nonce = "" },
		"empty key":        func(r *ApprovalDecideRequest) { r.IdempotencyKey = " " },
	} {
		_, err = issue(nil, nil, mutate)
		refused(name, err)
	}
	if _, err := IssueApprovalDecideDecision(validApprovalDecideEvidence(), validApprovalDecideAuthority(), validApprovalDecideRequest(), time.Time{}); err == nil {
		t.Error("a zero issuance time must be refused")
	}
}

func TestApprovalDecideAuthorityValidateRequiresAnOpenCompleteProjection(t *testing.T) {
	if err := validApprovalDecideAuthority().Validate(); err != nil {
		t.Fatalf("valid authority: %v", err)
	}
	for name, mutate := range map[string]func(*ApprovalDecideAuthority){
		"approval id": func(a *ApprovalDecideAuthority) { a.ApprovalID = "" },
		"org":         func(a *ApprovalDecideAuthority) { a.OrgID = " " },
		"space":       func(a *ApprovalDecideAuthority) { a.SpaceRef = "" },
		"run":         func(a *ApprovalDecideAuthority) { a.RunID = "" },
		"requester":   func(a *ApprovalDecideAuthority) { a.RequesterSubjectID = "" },
		"state":       func(a *ApprovalDecideAuthority) { a.State = "" },
	} {
		authority := validApprovalDecideAuthority()
		mutate(&authority)
		if authority.Validate() == nil {
			t.Errorf("missing %s must be refused", name)
		}
	}
}
