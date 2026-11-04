package spaces

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

// WS10-S2 live proof of the standing approval rule repository. Gated on
// TEST_DATABASE_URL through newRoomLiveFixture (disposable Postgres; skipped
// without it). Every Space carries the fixture's per-run suffix, so the cleanup
// below removes exactly what the test created.

type approvalLive struct {
	*roomLiveFixture
	ref, manager, editor, agent string
}

func newApprovalLive(t *testing.T, name string, ceiling string) *approvalLive {
	t.Helper()
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	t.Cleanup(func() {
		pattern := "ws04s4-%-" + f.suffix
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_approval_grant_events WHERE space_ref LIKE $1`, pattern)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_approval_grants WHERE space_ref LIKE $1`, pattern)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_approval_policies WHERE space_ref LIKE $1`, pattern)
	})
	owner := f.user(t, name+"-owner", f.org)
	a := &approvalLive{roomLiveFixture: f}
	a.manager = f.user(t, name+"-manager", f.org)
	a.editor = f.user(t, name+"-editor", f.org)
	a.ref = f.room(t, name, owner)
	f.member(t, a.ref, a.manager, "manager", SourceMemberOps)
	f.member(t, a.ref, a.editor, "editor", SourceMemberOps)
	a.agent = "ws04s4-" + name + "-agent-" + f.suffix
	f.exec(t, `INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, source)
		VALUES ($1,'service',$2,'editor','ws04s4-test',$3)`, a.ref, a.agent, SourceApplicationSync)
	if ceiling != "" {
		if _, err := f.repo.UpsertApprovalPolicy(ctx, a.ref, f.org, a.manager, ApprovalPolicyUpdate{
			MaxGrantScope: ceiling, DefaultAddresseeKind: ApprovalAddresseeRequester, QuarantineMode: QuarantineOff,
		}); err != nil {
			t.Fatalf("set ceiling %s: %v", ceiling, err)
		}
	}
	return a
}

func (a *approvalLive) evidence(subject, role string) PersonalThreadDecisionEvidence {
	e := approvalEvidence(role)
	e.Membership.SpaceRef, e.Membership.OrgID, e.Membership.SubjectID = a.ref, a.org, subject
	e.RecipientAudienceRef = "audience:" + a.ref + ":2"
	e.ResourceAuthorizationRef = "control:" + a.ref + ":approval-grant:7"
	return e
}

func (a *approvalLive) request(key, hashChar string) ApprovalRuleRequest {
	r := validApprovalRuleRequest()
	r.SpaceRef, r.AgentSubjectID, r.IdempotencyKey = a.ref, a.agent, key
	r.ActionSchemaHash = "sha256:" + strings.Repeat(hashChar, 64)
	return r
}

func (a *approvalLive) useRequest(hashChar string) ApprovalRuleUseRequest {
	return ApprovalRuleUseRequest{
		RunID: "run-1", AgentSubjectID: a.agent, ToolKey: "mcp__github__list_issues", ActionKey: "mcp__github__list_issues",
		SchemaHash: "sha256:" + strings.Repeat(hashChar, 64),
	}
}

func (a *approvalLive) eventKinds(t *testing.T, ruleID string) []string {
	t.Helper()
	rows, err := a.pool.Query(context.Background(), `SELECT kind || ':' || reason_code FROM space_approval_grant_events WHERE rule_id=$1 ORDER BY event_id`, ruleID)
	if err != nil {
		t.Fatalf("read events: %v", err)
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var s string
		if err := rows.Scan(&s); err != nil {
			t.Fatal(err)
		}
		out = append(out, s)
	}
	return out
}

func TestApprovalRuleCreateIsIdempotentLive(t *testing.T) {
	a := newApprovalLive(t, "aprcreate", "always")
	ctx := context.Background()
	now := time.Now().UTC()
	manager := a.evidence(a.manager, "manager")

	first, created, err := a.repo.CreateApprovalRule(ctx, manager, a.request("create-key-0001", "a"), now)
	if err != nil || !created {
		t.Fatalf("first create: created=%v err=%v", created, err)
	}
	if first.Scope != "always" || first.GrantedByRole != "manager" || first.GrantedBySubjectID != a.manager ||
		!first.ReviewBy.After(now.Add(89*24*time.Hour)) {
		t.Fatalf("stored rule is wrong: %#v", first)
	}
	replay, created, err := a.repo.CreateApprovalRule(ctx, manager, a.request("create-key-0001", "a"), now.Add(time.Second))
	if err != nil || created || replay.RuleID != first.RuleID {
		t.Fatalf("replay must return the stored rule: %#v created=%v err=%v", replay, created, err)
	}
	// The same key for a different action is a 409-class conflict.
	other := a.request("create-key-0001", "a")
	other.ToolKey, other.ActionKey = "mcp__github__create_issue", "mcp__github__create_issue"
	if _, _, err := a.repo.CreateApprovalRule(ctx, manager, other, now); !errors.Is(err, ErrApprovalIdempotencyReused) {
		t.Fatalf("err = %v, want ErrApprovalIdempotencyReused", err)
	}
	// Same action again under a new key while the live rule matches: no new row.
	same, created, err := a.repo.CreateApprovalRule(ctx, manager, a.request("create-key-0002", "a"), now)
	if err != nil || created || same.RuleID != first.RuleID {
		t.Fatalf("an identical live rule must be returned as is: %#v created=%v err=%v", same, created, err)
	}
	if rules, err := a.repo.ListApprovalRules(ctx, a.ref, a.org, true); err != nil || len(rules) != 1 {
		t.Fatalf("rules = %d err=%v, want exactly one", len(rules), err)
	}
	if got := a.eventKinds(t, first.RuleID); len(got) != 1 || got[0] != "created:" {
		t.Fatalf("events = %v", got)
	}
}

func TestApprovalRuleSchemaChangeSupersedesTheLiveRuleLive(t *testing.T) {
	a := newApprovalLive(t, "aprsuper", "always")
	ctx := context.Background()
	now := time.Now().UTC()
	manager := a.evidence(a.manager, "manager")
	old, _, err := a.repo.CreateApprovalRule(ctx, manager, a.request("super-key-00001", "a"), now)
	if err != nil {
		t.Fatal(err)
	}
	next, created, err := a.repo.CreateApprovalRule(ctx, manager, a.request("super-key-00002", "b"), now.Add(time.Second))
	if err != nil || !created || next.RuleID == old.RuleID {
		t.Fatalf("schema change must create a new rule: %#v created=%v err=%v", next, created, err)
	}
	superseded, err := a.repo.GetApprovalRule(ctx, a.ref, a.org, old.RuleID)
	if err != nil || superseded.RevokedAt == nil || superseded.RevokeReason != "superseded" {
		t.Fatalf("old rule = %#v err=%v, want revoked as superseded", superseded, err)
	}
	live, err := a.repo.ListApprovalRules(ctx, a.ref, a.org, false)
	if err != nil || len(live) != 1 || live[0].RuleID != next.RuleID {
		t.Fatalf("live rules = %#v err=%v, want only the new rule", live, err)
	}
}

func TestApprovalCeilingOnceRefusesCreateAndUseLive(t *testing.T) {
	a := newApprovalLive(t, "aprceil", "")
	ctx := context.Background()
	now := time.Now().UTC()

	// No policy row: the most restrictive ceiling (once) applies.
	if _, _, err := a.repo.CreateApprovalRule(ctx, a.evidence(a.manager, "manager"), a.request("ceil-key-000001", "a"), now); !errors.Is(err, ErrApprovalScopeNotPermitted) {
		t.Fatalf("create under the default ceiling: err = %v, want ErrApprovalScopeNotPermitted", err)
	}
	// Raise to always, create, then lower to once: use must stop applying.
	policy, err := a.repo.UpsertApprovalPolicy(ctx, a.ref, a.org, a.manager, ApprovalPolicyUpdate{
		MaxGrantScope: "always", DefaultAddresseeKind: ApprovalAddresseeRequester, QuarantineMode: QuarantineOff, ExpectedRevision: 0,
	})
	if err != nil || policy.Revision != 1 {
		t.Fatalf("raise ceiling: %#v err=%v", policy, err)
	}
	if _, _, err := a.repo.CreateApprovalRule(ctx, a.evidence(a.manager, "manager"), a.request("ceil-key-000001", "a"), now); err != nil {
		t.Fatalf("create under always: %v", err)
	}
	// A stale expected_revision conflicts.
	if _, err := a.repo.UpsertApprovalPolicy(ctx, a.ref, a.org, a.manager, ApprovalPolicyUpdate{
		MaxGrantScope: "once", DefaultAddresseeKind: ApprovalAddresseeRequester, QuarantineMode: QuarantineOff, ExpectedRevision: 0,
	}); !errors.Is(err, ErrApprovalPolicyConflict) {
		t.Fatalf("stale revision: err = %v, want ErrApprovalPolicyConflict", err)
	}
	if _, err := a.repo.UpsertApprovalPolicy(ctx, a.ref, a.org, a.manager, ApprovalPolicyUpdate{
		MaxGrantScope: "once", DefaultAddresseeKind: ApprovalAddresseeRequester, QuarantineMode: QuarantineOff, ExpectedRevision: 1,
	}); err != nil {
		t.Fatalf("lower ceiling: %v", err)
	}
	if _, err := a.repo.UseApprovalRule(ctx, a.evidence(a.editor, "editor"), a.useRequest("a"), now); !errors.Is(err, ErrNoApplicableApprovalRule) {
		t.Fatalf("use under a lowered ceiling: err = %v, want ErrNoApplicableApprovalRule", err)
	}
}

func TestApprovalRuleRefusesAnAgentThatIsNotInTheSpaceLive(t *testing.T) {
	a := newApprovalLive(t, "apragent", "always")
	req := a.request("agent-key-00001", "a")
	req.AgentSubjectID = "ws04s4-stranger-" + a.suffix
	if _, _, err := a.repo.CreateApprovalRule(context.Background(), a.evidence(a.manager, "manager"), req, time.Now().UTC()); !errors.Is(err, ErrApprovalAgentNotInSpace) {
		t.Fatalf("err = %v, want ErrApprovalAgentNotInSpace", err)
	}
	// A user member is not an agent either.
	req.AgentSubjectID = a.editor
	if _, _, err := a.repo.CreateApprovalRule(context.Background(), a.evidence(a.manager, "manager"), req, time.Now().UTC()); !errors.Is(err, ErrApprovalAgentNotInSpace) {
		t.Fatalf("a human member was accepted as the agent: err = %v", err)
	}
}

func TestApprovalRuleUseCountsAndWritesAUsedEventLive(t *testing.T) {
	a := newApprovalLive(t, "apruse", "always")
	ctx := context.Background()
	now := time.Now().UTC()
	rule, _, err := a.repo.CreateApprovalRule(ctx, a.evidence(a.manager, "manager"), a.request("use-key-0000001", "a"), now)
	if err != nil {
		t.Fatal(err)
	}
	editor := a.evidence(a.editor, "editor")
	for i := int64(1); i <= 2; i++ {
		used, err := a.repo.UseApprovalRule(ctx, editor, a.useRequest("a"), now.Add(time.Duration(i)*time.Second))
		if err != nil || used.RuleID != rule.RuleID || used.UseCount != i || used.LastUsedAt == nil {
			t.Fatalf("use %d: %#v err=%v", i, used, err)
		}
	}
	events := a.eventKinds(t, rule.RuleID)
	if strings.Join(events, ",") != "created:,used:,used:" {
		t.Fatalf("events = %v, want created then two used", events)
	}
}

func TestApprovalRuleRevokedExpiredOrSchemaMismatchGiveNoRuleLive(t *testing.T) {
	a := newApprovalLive(t, "aprnone", "always")
	ctx := context.Background()
	now := time.Now().UTC()
	editor := a.evidence(a.editor, "editor")
	manager := a.evidence(a.manager, "manager")

	rule, _, err := a.repo.CreateApprovalRule(ctx, manager, a.request("none-key-0000001", "a"), now)
	if err != nil {
		t.Fatal(err)
	}
	// Schema mismatch: the rule exists but does not cover this schema.
	if _, err := a.repo.UseApprovalRule(ctx, editor, a.useRequest("b"), now); !errors.Is(err, ErrNoApplicableApprovalRule) {
		t.Fatalf("schema mismatch: err = %v", err)
	}
	// Past review_by: expired, and one expired event only however often it is tried.
	late := rule.ReviewBy.Add(time.Hour)
	for i := 0; i < 2; i++ {
		if _, err := a.repo.UseApprovalRule(ctx, editor, a.useRequest("a"), late); !errors.Is(err, ErrNoApplicableApprovalRule) {
			t.Fatalf("expired: err = %v", err)
		}
	}
	events := strings.Join(a.eventKinds(t, rule.RuleID), ",")
	if strings.Count(events, "expired:review_by") != 1 || !strings.Contains(events, "use_denied:schema_changed") || !strings.Contains(events, "use_denied:review_expired") {
		t.Fatalf("events = %s", events)
	}
	// Revoked: no rule, and revoking again is idempotent.
	revoked, err := a.repo.RevokeApprovalRule(ctx, a.ref, a.org, rule.RuleID, a.manager, "no longer needed", now)
	if err != nil || revoked.RevokedAt == nil || revoked.RevokeReason != "no longer needed" {
		t.Fatalf("revoke: %#v err=%v", revoked, err)
	}
	again, err := a.repo.RevokeApprovalRule(ctx, a.ref, a.org, rule.RuleID, a.editor, "other", now.Add(time.Minute))
	if err != nil || again.RevokeReason != "no longer needed" || again.RevokedBy != a.manager {
		t.Fatalf("a second revoke must not rewrite the first: %#v err=%v", again, err)
	}
	if _, err := a.repo.UseApprovalRule(ctx, editor, a.useRequest("a"), now); !errors.Is(err, ErrNoApplicableApprovalRule) {
		t.Fatalf("revoked: err = %v", err)
	}
	if _, err := a.repo.RevokeApprovalRule(ctx, a.ref, a.org, "apr_missing", a.manager, "", now); !errors.Is(err, ErrApprovalRuleNotFound) {
		t.Fatalf("unknown rule: err = %v, want ErrApprovalRuleNotFound", err)
	}
	// Another org's view of the same rule id finds nothing.
	if _, err := a.repo.GetApprovalRule(ctx, a.ref, a.other, rule.RuleID); !errors.Is(err, ErrApprovalRuleNotFound) {
		t.Fatalf("cross-org read: err = %v, want ErrApprovalRuleNotFound", err)
	}
}

func TestReplaceMembershipsRemovingAnAgentRevokesItsRulesLive(t *testing.T) {
	a := newApprovalLive(t, "aprgone", "always")
	ctx := context.Background()
	rule, _, err := a.repo.CreateApprovalRule(ctx, a.evidence(a.manager, "manager"), a.request("gone-key-0000001", "a"), time.Now().UTC())
	if err != nil {
		t.Fatal(err)
	}
	// The declaration speaks for services and omits the agent.
	if _, _, _, _, err := a.repo.ReplaceMemberships(ctx, MembershipReplacement{
		SpaceRef: a.ref, ManagedSubjectTypes: []string{"service"},
	}); err != nil {
		t.Fatalf("replace: %v", err)
	}
	got, err := a.repo.GetApprovalRule(ctx, a.ref, a.org, rule.RuleID)
	if err != nil || got.RevokedAt == nil || got.RevokeReason != "agent_removed" || got.RevokedBy != "system" {
		t.Fatalf("rule after agent removal = %#v err=%v, want revoked agent_removed by system", got, err)
	}
	if events := a.eventKinds(t, rule.RuleID); events[len(events)-1] != "revoked:agent_removed" {
		t.Fatalf("events = %v", events)
	}
}
