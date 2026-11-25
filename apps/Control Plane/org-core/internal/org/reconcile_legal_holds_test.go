package org

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/jackc/pgx/v5"
)

// fakeHoldGateRow answers one Scan: its error, or the values written to its *int
// destinations in order.
type fakeHoldGateRow struct {
	err    error
	values []int
}

func (r fakeHoldGateRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	for i, d := range dest {
		if p, ok := d.(*int); ok && i < len(r.values) {
			*p = r.values[i]
		}
	}
	return nil
}

// fakeHoldGateTx answers the gate's two reads and records their SQL in order: the
// organisation lock first, then the active-hold count.
type fakeHoldGateTx struct {
	lockErr     error
	countErr    error
	orgHolds    int
	threadHolds int
	queries     []string
}

func (f *fakeHoldGateTx) QueryRow(_ context.Context, sql string, _ ...any) pgx.Row {
	f.queries = append(f.queries, sql)
	if strings.Contains(sql, "FOR UPDATE") {
		return fakeHoldGateRow{err: f.lockErr}
	}
	return fakeHoldGateRow{err: f.countErr, values: []int{f.orgHolds, f.threadHolds}}
}

// F-f10-06 (reconcile): the gate turns the counts it reads into the typed held refusal, and an
// unreadable lock or count is the unavailable refusal, never a held one. The SQL that filters
// released holds out is pinned by TestReconcileHoldGateLocksBeforeItCounts.
func TestReconcileHoldGateMapsCountsToTheTypedRefusal(t *testing.T) {
	tests := []struct {
		name     string
		tx       *fakeHoldGateTx
		wantHeld *ErasureHeldError
		wantErr  error
	}{
		{name: "no holds", tx: &fakeHoldGateTx{}},
		{name: "organisation hold", tx: &fakeHoldGateTx{orgHolds: 1}, wantHeld: &ErasureHeldError{OrgHolds: 1}},
		{name: "thread holds only", tx: &fakeHoldGateTx{threadHolds: 2}, wantHeld: &ErasureHeldError{ThreadHolds: 2}},
		{name: "organisation and thread holds", tx: &fakeHoldGateTx{orgHolds: 1, threadHolds: 1}, wantHeld: &ErasureHeldError{OrgHolds: 1, ThreadHolds: 1}},
		{name: "organisation row already gone", tx: &fakeHoldGateTx{lockErr: pgx.ErrNoRows}},
		{name: "lock unreadable", tx: &fakeHoldGateTx{lockErr: errors.New("database unavailable")}, wantErr: ErrErasureHoldCheckUnavailable},
		{name: "count unreadable", tx: &fakeHoldGateTx{countErr: errors.New("database unavailable")}, wantErr: ErrErasureHoldCheckUnavailable},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := refuseHeldOrganizationErasure(context.Background(), tc.tx, "org-gate")
			switch {
			case tc.wantErr != nil:
				if !errors.Is(err, tc.wantErr) || errors.Is(err, ErrErasureBlockedLegalHold) {
					t.Fatalf("err = %v, want the unavailable refusal and not a held one", err)
				}
			case tc.wantHeld != nil:
				var held *ErasureHeldError
				if !errors.As(err, &held) || *held != *tc.wantHeld {
					t.Fatalf("err = %v, want %+v", err, *tc.wantHeld)
				}
				if !errors.Is(err, ErrErasureBlockedLegalHold) || errors.Is(err, ErrErasureHoldCheckUnavailable) {
					t.Fatalf("err = %v must match the held refusal only", err)
				}
			default:
				if err != nil {
					t.Fatalf("err = %v, want nil: nothing holds the organisation", err)
				}
			}
		})
	}
}

// The gate locks the organisation row before it counts the holds, so a hold placed later
// waits for the erasure transaction; a failed lock stops the gate before any count.
func TestReconcileHoldGateLocksBeforeItCounts(t *testing.T) {
	tx := &fakeHoldGateTx{}
	if err := refuseHeldOrganizationErasure(context.Background(), tx, "org-gate"); err != nil {
		t.Fatalf("gate: %v", err)
	}
	if len(tx.queries) != 2 {
		t.Fatalf("queries = %d, want the lock then the count", len(tx.queries))
	}
	if !strings.Contains(tx.queries[0], "FROM organizations") || !strings.Contains(tx.queries[0], "FOR UPDATE") {
		t.Fatalf("first query must lock the organisation row: %q", tx.queries[0])
	}
	if !strings.Contains(tx.queries[1], "org_legal_holds") || !strings.Contains(tx.queries[1], "released_at IS NULL") {
		t.Fatalf("second query must count active holds: %q", tx.queries[1])
	}

	unlocked := &fakeHoldGateTx{lockErr: errors.New("database unavailable")}
	if err := refuseHeldOrganizationErasure(context.Background(), unlocked, "org-gate"); !errors.Is(err, ErrErasureHoldCheckUnavailable) {
		t.Fatalf("failed lock: err = %v, want the unavailable refusal", err)
	}
	if len(unlocked.queries) != 1 {
		t.Fatalf("failed lock ran %d queries, want the lock alone", len(unlocked.queries))
	}
}

// ERASURE_HOLD_CHECK is on only for the literal "true", as the HTTP erasure routes read it.
func TestReconcileHoldGateSwitchIsLiteralTrue(t *testing.T) {
	for _, value := range []string{"", "1", "TRUE", "yes", " true"} {
		t.Setenv(envErasureHoldCheck, value)
		if erasureHoldCheckOn() {
			t.Fatalf("ERASURE_HOLD_CHECK=%q must leave the reconcile gate off", value)
		}
	}
	t.Setenv(envErasureHoldCheck, "true")
	if !erasureHoldCheckOn() {
		t.Fatal("ERASURE_HOLD_CHECK=true must switch the reconcile gate on")
	}
}

// A refused reconcile erasure is audited once, as a hard delete under the projection actor,
// with the active counts. A missing held error records nothing.
func TestReconcileHeldRefusalIsAuditedOnceUnderTheProjectionActor(t *testing.T) {
	auditor := &fakeRefusalAuditor{}
	svc := &Service{}
	svc.SetErasureRefusalAuditor(auditor)
	svc.RecordHeldReconcileRefusal(context.Background(), "org-held", &ErasureHeldError{OrgHolds: 1, ThreadHolds: 2})
	svc.RecordHeldReconcileRefusal(context.Background(), "org-held", nil)

	want := refusalCall{"org-held", retention.Actor{ID: GDPRReconcileActorID, Role: "system"}, retention.ErasureOpHardDelete, 1, 2}
	if len(auditor.calls) != 1 || auditor.calls[0] != want {
		t.Fatalf("calls = %+v, want [%+v]", auditor.calls, want)
	}
}

// An unwired auditor or a failing one never panics and never changes the refusal, which the
// caller still answers.
func TestReconcileHeldRefusalAuditSurvivesMissingAndFailingAuditors(t *testing.T) {
	(&Service{}).RecordHeldReconcileRefusal(context.Background(), "org-held", &ErasureHeldError{OrgHolds: 1})

	failing := &fakeRefusalAuditor{auditErr: errors.New("outbox down")}
	svc := &Service{}
	svc.SetErasureRefusalAuditor(failing)
	svc.RecordHeldReconcileRefusal(context.Background(), "org-held", &ErasureHeldError{OrgHolds: 1})
	if len(failing.calls) != 1 {
		t.Fatalf("calls = %d, want one attempt despite the failure", len(failing.calls))
	}
}

// The typed refusal matches through wrapping, and carries the counts for the caller.
func TestErasureHeldErrorMatchesTheTypedRefusalThroughWrapping(t *testing.T) {
	err := fmt.Errorf("reconcile: %w", &ErasureHeldError{OrgHolds: 1, ThreadHolds: 3})
	var held *ErasureHeldError
	if !errors.Is(err, ErrErasureBlockedLegalHold) || !errors.As(err, &held) {
		t.Fatalf("wrapped held error = %v, want it to match the typed refusal", err)
	}
	if held.OrgHolds != 1 || held.ThreadHolds != 3 {
		t.Fatalf("counts = %+v, want 1 org and 3 thread", *held)
	}
}
