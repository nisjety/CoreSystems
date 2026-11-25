package org

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
)

type refusalCall struct {
	orgID       string
	actor       retention.Actor
	op          string
	orgHolds    int
	threadHolds int
}

// fakeRefusalAuditor answers holds per org and records every refusal audit.
type fakeRefusalAuditor struct {
	holds    map[string][]retention.Hold
	holdsErr error
	auditErr error
	calls    []refusalCall
}

func (f *fakeRefusalAuditor) Holds(_ context.Context, orgID string) (retention.HoldList, error) {
	if f.holdsErr != nil {
		return retention.HoldList{}, f.holdsErr
	}
	return retention.HoldList{OrganizationID: orgID, Holds: f.holds[orgID]}, nil
}

func (f *fakeRefusalAuditor) AuditRefusedErasure(_ context.Context, orgID string, actor retention.Actor, op string, orgHolds, threadHolds int) error {
	f.calls = append(f.calls, refusalCall{orgID, actor, op, orgHolds, threadHolds})
	return f.auditErr
}

// F-f10-06: every organisation the purge skipped for a legal hold gets one refusal
// row, under the cron's fixed actor, with the active holds counted (released ones not).
func TestPurgeAuditsEachHeldOrgOnceWithTheCronActor(t *testing.T) {
	released := time.Now()
	auditor := &fakeRefusalAuditor{holds: map[string][]retention.Hold{
		"org-held": {
			{Scope: retention.ScopeOrg, TargetRef: "org-held"},
			{Scope: retention.ScopeThread, TargetRef: "thr-1"},
			{Scope: retention.ScopeThread, TargetRef: "thr-2", ReleasedAt: &released},
		},
		"org-thread": {{Scope: retention.ScopeThread, TargetRef: "thr-9"}},
	}}
	svc := &Service{}
	svc.SetErasureRefusalAuditor(auditor)
	svc.auditHeldPurges(context.Background(), []string{"org-held", "", "org-thread"})

	want := []refusalCall{
		{"org-held", retention.Actor{ID: gdprPurgeCronActorID, Role: "system"}, retention.ErasureOpPurge, 1, 1},
		{"org-thread", retention.Actor{ID: gdprPurgeCronActorID, Role: "system"}, retention.ErasureOpPurge, 0, 1},
	}
	if len(auditor.calls) != len(want) {
		t.Fatalf("calls = %+v, want %+v", auditor.calls, want)
	}
	for i := range want {
		if auditor.calls[i] != want[i] {
			t.Fatalf("call %d = %+v, want %+v", i, auditor.calls[i], want[i])
		}
	}
}

// An unreadable hold list still records the refusal, with the counts unknown; an audit
// failure is logged and the sweep goes on to the next organisation.
func TestPurgeAuditSurvivesUnreadableHoldsAndAuditFailures(t *testing.T) {
	auditor := &fakeRefusalAuditor{holdsErr: errors.New("db down"), auditErr: errors.New("outbox down")}
	svc := &Service{}
	svc.SetErasureRefusalAuditor(auditor)
	svc.auditHeldPurges(context.Background(), []string{"org-a", "org-b"})
	if len(auditor.calls) != 2 {
		t.Fatalf("calls = %d, want one per held org despite failures", len(auditor.calls))
	}
	for _, call := range auditor.calls {
		if call.orgHolds != retention.HoldCountUnknown || call.threadHolds != retention.HoldCountUnknown {
			t.Fatalf("unreadable holds must record unknown counts: %+v", call)
		}
	}
}

// No held organisations, or no auditor wired: nothing is called and nothing panics.
func TestPurgeAuditIsANoOpWithoutHeldOrgsOrAuditor(t *testing.T) {
	auditor := &fakeRefusalAuditor{}
	svc := &Service{}
	svc.SetErasureRefusalAuditor(auditor)
	svc.auditHeldPurges(context.Background(), nil)
	if len(auditor.calls) != 0 {
		t.Fatalf("calls = %d, want 0", len(auditor.calls))
	}
	(&Service{}).auditHeldPurges(context.Background(), []string{"org-held"})
}
