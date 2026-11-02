package spaces

import (
	"errors"
	"strings"
	"testing"
	"time"
)

// WS10-S2: authority guards for standing approval rules, request shape and the
// effect-time continuation fence (pure logic; the repository is in the live test).

func approvalEvidence(role string) PersonalThreadDecisionEvidence {
	e := validPersonalThreadEvidence()
	e.Membership.Kind = KindRoom
	e.Membership.SpaceRef = "space-room"
	e.Membership.Role = role
	e.RecipientAudienceRef = "audience:space-room:2"
	e.ResourceAuthorizationRef = "control:space-room:approval-grant:7"
	e.ApprovalGrantEntitled = true
	return e
}

func wantDenial(t *testing.T, err error, code DenialCode) *Denial {
	t.Helper()
	if err == nil {
		t.Fatalf("expected denial %s, got nil", code)
	}
	denial, ok := AsDenial(err)
	if !ok {
		t.Fatalf("expected typed denial %s, got untyped %v", code, err)
	}
	if denial.Code != code {
		t.Fatalf("denial code = %s, want %s (%v)", denial.Code, code, err)
	}
	return denial
}

func TestValidateForApprovalRuleManageRoleFloorAndGuards(t *testing.T) {
	for _, role := range []string{"manager", "owner"} {
		if err := approvalEvidence(role).ValidateForApprovalRuleManage(); err != nil {
			t.Errorf("%s refused: %v", role, err)
		}
	}
	for _, role := range []string{"editor", "viewer"} {
		denial := wantDenial(t, approvalEvidence(role).ValidateForApprovalRuleManage(), DenialRoleBelowFloor)
		if strings.Join(denial.RequiredRoles, ",") != "manager,owner" {
			t.Errorf("%s: required roles = %v, want manager and owner", role, denial.RequiredRoles)
		}
	}

	personal := approvalEvidence("owner")
	personal.Membership.Kind = KindPersonal
	wantDenial(t, personal.ValidateForApprovalRuleManage(), DenialKindUnsupported)

	off := approvalEvidence("manager")
	off.ApprovalGrantEntitled = false
	denial := wantDenial(t, off.ValidateForApprovalRuleManage(), DenialEntitlementOff)
	if denial.Entitlement != "approval_grant" {
		t.Errorf("entitlement = %q, want approval_grant", denial.Entitlement)
	}

	zdr := approvalEvidence("manager")
	zdr.Privacy.ZeroDataRetention = true
	denial = wantDenial(t, zdr.ValidateForApprovalRuleManage(), DenialZDRForbidsEffect)
	if denial.Effect != "approval_grant" {
		t.Errorf("zdr effect = %q, want approval_grant", denial.Effect)
	}

	noAudience := approvalEvidence("manager")
	noAudience.RecipientAudienceHash = ""
	wantDenial(t, noAudience.ValidateForApprovalRuleManage(), DenialAuthorityIncomplete)

	noRef := approvalEvidence("manager")
	noRef.ResourceAuthorizationRef = " "
	wantDenial(t, noRef.ValidateForApprovalRuleManage(), DenialAuthorityIncomplete)

	// Neighbouring entitlements never stand in for approval_grant.
	neighbours := approvalEvidence("manager")
	neighbours.ApprovalGrantEntitled = false
	neighbours.AgentActionEntitled, neighbours.ThreadCreateEntitled, neighbours.RunControlEntitled = true, true, true
	wantDenial(t, neighbours.ValidateForApprovalRuleManage(), DenialEntitlementOff)
}

func TestValidateForApprovalRuleUseRoleFloorAndGuards(t *testing.T) {
	for _, role := range []string{"editor", "manager", "owner"} {
		if err := approvalEvidence(role).ValidateForApprovalRuleUse(); err != nil {
			t.Errorf("%s refused: %v", role, err)
		}
	}
	denial := wantDenial(t, approvalEvidence("viewer").ValidateForApprovalRuleUse(), DenialRoleBelowFloor)
	if strings.Join(denial.RequiredRoles, ",") != "editor,manager,owner" {
		t.Errorf("required roles = %v", denial.RequiredRoles)
	}

	// The entitlement is re-read on every use: turning it off ends the rule.
	off := approvalEvidence("editor")
	off.ApprovalGrantEntitled = false
	wantDenial(t, off.ValidateForApprovalRuleUse(), DenialEntitlementOff)

	zdr := approvalEvidence("editor")
	zdr.Privacy.ZeroDataRetention = true
	wantDenial(t, zdr.ValidateForApprovalRuleUse(), DenialZDRForbidsEffect)

	personal := approvalEvidence("editor")
	personal.Membership.Kind = KindPersonal
	wantDenial(t, personal.ValidateForApprovalRuleUse(), DenialKindUnsupported)
}

func validApprovalRuleRequest() ApprovalRuleRequest {
	return ApprovalRuleRequest{
		SpaceRef: "space-room", AgentSubjectID: "agent-1",
		ToolKey: "mcp__github__list_issues", ActionKey: "mcp__github__list_issues",
		ActionSchemaHash: "sha256:" + strings.Repeat("c", 64),
		SourceApprovalID: "approval-1", IdempotencyKey: "rule-create-0001",
	}
}

func TestApprovalRuleRequestValidate(t *testing.T) {
	if err := validApprovalRuleRequest().Validate(); err != nil {
		t.Fatalf("valid request refused: %v", err)
	}
	// source_approval_id is optional.
	noSource := validApprovalRuleRequest()
	noSource.SourceApprovalID = ""
	if err := noSource.Validate(); err != nil {
		t.Fatalf("request without source approval refused: %v", err)
	}
	for name, mutate := range map[string]func(*ApprovalRuleRequest){
		"space_ref":       func(r *ApprovalRuleRequest) { r.SpaceRef = "" },
		"agent":           func(r *ApprovalRuleRequest) { r.AgentSubjectID = " " },
		"tool":            func(r *ApprovalRuleRequest) { r.ToolKey = "" },
		"action":          func(r *ApprovalRuleRequest) { r.ActionKey = "" },
		"schema hash":     func(r *ApprovalRuleRequest) { r.ActionSchemaHash = "" },
		"bad schema hash": func(r *ApprovalRuleRequest) { r.ActionSchemaHash = "sha256:ABC" },
		"short key":       func(r *ApprovalRuleRequest) { r.IdempotencyKey = "short" },
		"long key":        func(r *ApprovalRuleRequest) { r.IdempotencyKey = strings.Repeat("k", approvalIdempotencyHi+1) },
		"oversized agent": func(r *ApprovalRuleRequest) { r.AgentSubjectID = strings.Repeat("a", maxApprovalIDLen+1) },
		"oversized space": func(r *ApprovalRuleRequest) { r.SpaceRef = strings.Repeat("s", maxApprovalIDLen+1) },
		"oversized src":   func(r *ApprovalRuleRequest) { r.SourceApprovalID = strings.Repeat("p", maxApprovalIDLen+1) },
	} {
		t.Run(name, func(t *testing.T) {
			r := validApprovalRuleRequest()
			mutate(&r)
			if err := r.Validate(); err == nil {
				t.Fatalf("%s accepted", name)
			}
		})
	}
}

// A caller cannot obtain a rule for anything that is not an MCP tool, whatever
// else in the request is well formed.
func TestApprovalRuleRequestRefusesIneligibleToolsWithTheScopeSentinel(t *testing.T) {
	for _, tool := range []string{"execute_provider_action:gmail.send", "tickets.create", "shell", "browser.click"} {
		r := validApprovalRuleRequest()
		r.ToolKey, r.ActionKey = tool, tool
		if err := r.Validate(); !errors.Is(err, ErrApprovalScopeNotPermitted) {
			t.Errorf("%s: err = %v, want ErrApprovalScopeNotPermitted", tool, err)
		}
	}
	mismatch := validApprovalRuleRequest()
	mismatch.ActionKey = "mcp__github__create_issue"
	if err := mismatch.Validate(); !errors.Is(err, ErrApprovalScopeNotPermitted) {
		t.Errorf("tool/action mismatch: err = %v, want ErrApprovalScopeNotPermitted", err)
	}
}

func TestApprovalRuleUseRequestValidate(t *testing.T) {
	valid := ApprovalRuleUseRequest{
		RunID: "run-1", AgentSubjectID: "agent-1", ToolKey: "mcp__a__b", ActionKey: "mcp__a__b",
		SchemaHash: "sha256:" + strings.Repeat("d", 64),
	}
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid use request refused: %v", err)
	}
	for name, mutate := range map[string]func(*ApprovalRuleUseRequest){
		"run":         func(r *ApprovalRuleUseRequest) { r.RunID = "" },
		"agent":       func(r *ApprovalRuleUseRequest) { r.AgentSubjectID = "" },
		"tool":        func(r *ApprovalRuleUseRequest) { r.ToolKey = "" },
		"action":      func(r *ApprovalRuleUseRequest) { r.ActionKey = "" },
		"hash":        func(r *ApprovalRuleUseRequest) { r.SchemaHash = "nope" },
		"oversized":   func(r *ApprovalRuleUseRequest) { r.RunID = strings.Repeat("r", maxApprovalIDLen+1) },
		"oversizedAg": func(r *ApprovalRuleUseRequest) { r.AgentSubjectID = strings.Repeat("a", maxApprovalIDLen+1) },
	} {
		t.Run(name, func(t *testing.T) {
			r := valid
			mutate(&r)
			if err := r.Validate(); err == nil {
				t.Fatalf("%s accepted", name)
			}
		})
	}
}

func TestApprovalRuleLiveRequiresNoRevocationAndAnOpenReviewWindow(t *testing.T) {
	now := time.Date(2026, 11, 2, 9, 0, 0, 0, time.UTC)
	rule := ApprovalRule{ReviewBy: now.Add(ApprovalRuleReviewWindow)}
	if !rule.Live(now) {
		t.Fatal("a fresh rule must be live")
	}
	if rule.Live(rule.ReviewBy) || rule.Live(rule.ReviewBy.Add(time.Hour)) {
		t.Fatal("a rule at or past review_by must not be live")
	}
	revoked := now.Add(-time.Hour)
	rule.RevokedAt = &revoked
	if rule.Live(now) {
		t.Fatal("a revoked rule must not be live")
	}
	if ApprovalRuleReviewWindow != 90*24*time.Hour {
		t.Errorf("review window = %v, want 90 days", ApprovalRuleReviewWindow)
	}
}

func approvalContinuationAuthority() RunActionAuthority {
	a := validRunActionAuthority()
	a.SpaceRef = "space-room"
	a.RecipientAudienceRef = "audience:space-room:2"
	a.RunContextAuthorizationRef = "control:space-room:thread-create:7"
	return a
}

func approvalContinuationEvidence() PersonalThreadDecisionEvidence {
	e := approvalEvidence("editor")
	e.AgentActionEntitled = true
	return e
}

func TestValidateCurrentApprovalContinuation(t *testing.T) {
	if err := ValidateCurrentApprovalContinuation(approvalContinuationEvidence(), approvalContinuationAuthority()); err != nil {
		t.Fatalf("a matching continuation was refused: %v", err)
	}
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence, *RunActionAuthority){
		"authority revision changed": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) { a.AuthorityRevision++ },
		"evidence authority moved": func(e *PersonalThreadDecisionEvidence, _ *RunActionAuthority) {
			e.Membership.Revisions.Authority++
		},
		"audience hash changed": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) {
			a.RecipientAudienceHash = "sha256:other"
		},
		"audience ref changed": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) {
			a.RecipientAudienceRef = "audience:space-room:9"
		},
		"audience revision changed": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) { a.RecipientAudienceRevision++ },
		"privacy policy changed":    func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) { a.PrivacyPolicyRef = "privacy:org-1:6" },
		"different requester":       func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) { a.SubjectID = "user-2" },
		"different space":           func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) { a.SpaceRef = "space-other" },
		"different org":             func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) { a.OrgID = "org-2" },
		"run source ref changed": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority) {
			a.RunContextAuthorizationRef = "control:space-room:thread-create:6"
		},
	} {
		t.Run(name, func(t *testing.T) {
			e, a := approvalContinuationEvidence(), approvalContinuationAuthority()
			mutate(&e, &a)
			if err := ValidateCurrentApprovalContinuation(e, a); err == nil {
				t.Fatalf("%s was accepted", name)
			}
		})
	}
	// A requester who lost the agent-action entitlement or went ZDR cannot continue.
	e := approvalContinuationEvidence()
	e.AgentActionEntitled = false
	wantDenial(t, ValidateCurrentApprovalContinuation(e, approvalContinuationAuthority()), DenialEntitlementOff)
	e = approvalContinuationEvidence()
	e.Privacy.ZeroDataRetention = true
	wantDenial(t, ValidateCurrentApprovalContinuation(e, approvalContinuationAuthority()), DenialZDRForbidsEffect)
	e = approvalContinuationEvidence()
	e.Membership.Role = "viewer"
	wantDenial(t, ValidateCurrentApprovalContinuation(e, approvalContinuationAuthority()), DenialRoleBelowFloor)
}
