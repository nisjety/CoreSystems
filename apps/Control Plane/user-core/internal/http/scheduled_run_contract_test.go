package http

import (
	"context"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// F-f10-27 rel1: with SCHEDULE_CONTRACT_ENABLED on, the run-execution decision binds the
// stored-consent contract only from the prepared Session Core run. The contract the
// Orchestrator body claims must equal the prepared one; it is never a binding.

var runNow = time.Date(2026, 8, 16, 0, 0, 0, 0, time.UTC)

func runDigest(fill byte) string {
	return "sha256:" + strings.Repeat(string(fill), 64)
}

// requestedRun is the execution intent the Orchestrator presents for one prepared run.
func requestedRun() spaces.ScheduledRunIntent {
	return spaces.ScheduledRunIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey: "fire-1", TaskID: "run-1", TemplateDigest: runDigest('a'), IdempotencyKey: "fire-1",
	}
}

// preparedRun is what Session Core prepared for that same run, with no contract yet.
func preparedRun(intent spaces.ScheduledRunIntent) spaces.ScheduledRunAuthority {
	return spaces.ScheduledRunAuthority{
		RunID: intent.TaskID, ThreadID: "thread-1", OrgID: intent.OrgID, SubjectID: intent.SubjectID,
		SpaceRef: intent.SpaceRef, ScheduleID: intent.ScheduleID, FireKey: intent.FireKey,
		TemplateDigest: intent.TemplateDigest, IdempotencyKey: intent.IdempotencyKey,
	}
}

func TestScheduledRunBindingTakesTheContractOnlyFromPrepared(t *testing.T) {
	expiresAt := runNow.Add(time.Hour)
	intent := requestedRun()
	prepared := preparedRun(intent)
	prepared.ContractDigest = runDigest('c')
	prepared.ContractExpiresAt = expiresAt

	bound, status, message := scheduledRunBoundIntent(intent, "thread-1", runDigest('c'), expiresAt.Format(time.RFC3339), prepared, runNow)
	if status != 0 {
		t.Fatalf("a claim equal to the prepared contract was refused: %d %s", status, message)
	}
	if bound.ContractDigest != prepared.ContractDigest || !bound.ContractExpiresAt.Equal(expiresAt) {
		t.Fatalf("bound contract = %q at %s, want the prepared %q at %s",
			bound.ContractDigest, bound.ContractExpiresAt, prepared.ContractDigest, expiresAt)
	}
	if bound.SubjectID != prepared.SubjectID {
		t.Fatalf("bound subject = %q, want the prepared %q", bound.SubjectID, prepared.SubjectID)
	}
}

func TestScheduledRunBindingRefusesAContractClaimThatDiffersFromPrepared(t *testing.T) {
	expiresAt := runNow.Add(time.Hour)
	intent := requestedRun()
	contractBound := preparedRun(intent)
	contractBound.ContractDigest = runDigest('c')
	contractBound.ContractExpiresAt = expiresAt
	legacy := preparedRun(intent)

	cases := map[string]struct {
		prepared spaces.ScheduledRunAuthority
		digest   string
		expires  string
	}{
		"another digest for a contract-bound run": {contractBound, runDigest('d'), expiresAt.Format(time.RFC3339)},
		"another expiry for a contract-bound run": {contractBound, runDigest('c'), expiresAt.Add(2 * time.Hour).Format(time.RFC3339)},
		"no claim for a contract-bound run":       {contractBound, "", ""},
		"a malformed claimed expiry":              {contractBound, runDigest('c'), "not-a-time"},
		"a claim for a legacy run":                {legacy, runDigest('c'), expiresAt.Format(time.RFC3339)},
	}
	for name, tc := range cases {
		if _, status, _ := scheduledRunBoundIntent(intent, "thread-1", tc.digest, tc.expires, tc.prepared, runNow); status != http.StatusForbidden {
			t.Fatalf("%s: status=%d, want 403", name, status)
		}
	}
}

func TestScheduledRunBindingRefusesAnExpiredPreparedContractWith403(t *testing.T) {
	intent := requestedRun()
	prepared := preparedRun(intent)
	prepared.ContractDigest = runDigest('c')
	prepared.ContractExpiresAt = runNow.Add(-time.Minute)

	_, status, message := scheduledRunBoundIntent(intent, "thread-1", runDigest('c'), prepared.ContractExpiresAt.Format(time.RFC3339), prepared, runNow)
	if status != http.StatusForbidden || message != "scheduled_contract_expired" {
		t.Fatalf("expired contract: status=%d message=%q, want 403 scheduled_contract_expired", status, message)
	}
}

func TestScheduledRunBindingKeepsALegacyRunOnTheV1Binding(t *testing.T) {
	intent := requestedRun()
	prepared := preparedRun(intent)

	bound, status, message := scheduledRunBoundIntent(intent, "thread-1", "", "", prepared, runNow)
	if status != 0 {
		t.Fatalf("legacy run refused: %d %s", status, message)
	}
	if bound.ContractDigest != "" || !bound.ContractExpiresAt.IsZero() {
		t.Fatalf("legacy run gained a contract binding: %#v", bound)
	}
}

func TestScheduledRunBindingStillRefusesAMismatchedPreparedRun(t *testing.T) {
	intent := requestedRun()
	mismatches := map[string]func(*spaces.ScheduledRunAuthority){
		"another run":          func(a *spaces.ScheduledRunAuthority) { a.RunID = "run-2" },
		"another organization": func(a *spaces.ScheduledRunAuthority) { a.OrgID = "org-2" },
		"another space":        func(a *spaces.ScheduledRunAuthority) { a.SpaceRef = "space-2" },
		"another schedule":     func(a *spaces.ScheduledRunAuthority) { a.ScheduleID = "schedule-2" },
		"another fire":         func(a *spaces.ScheduledRunAuthority) { a.FireKey = "fire-2" },
		"another template":     func(a *spaces.ScheduledRunAuthority) { a.TemplateDigest = runDigest('e') },
		"another idempotency":  func(a *spaces.ScheduledRunAuthority) { a.IdempotencyKey = "fire-2" },
		"another subject":      func(a *spaces.ScheduledRunAuthority) { a.SubjectID = "user-2" },
	}
	for name, mutate := range mismatches {
		prepared := preparedRun(intent)
		mutate(&prepared)
		if _, status, _ := scheduledRunBoundIntent(intent, "thread-1", "", "", prepared, runNow); status != http.StatusForbidden {
			t.Fatalf("%s: status=%d, want 403", name, status)
		}
	}

	// The body names a thread other than the prepared one.
	prepared := preparedRun(intent)
	if _, status, _ := scheduledRunBoundIntent(intent, "thread-2", "", "", prepared, runNow); status != http.StatusForbidden {
		t.Fatalf("another thread: status=%d, want 403", status)
	}

	// A prepared run with no subject is refused as invalid, never bound.
	noSubject := preparedRun(intent)
	noSubject.SubjectID = ""
	intentNoSubject := intent
	intentNoSubject.SubjectID = ""
	if _, status, _ := scheduledRunBoundIntent(intentNoSubject, "thread-1", "", "", noSubject, runNow); status != http.StatusForbidden {
		t.Fatalf("a prepared run without a subject: status=%d, want 403", status)
	}
}

// fakeScheduledRunAuthority stands in for the Session Core prepared-run read.
type fakeScheduledRunAuthority struct {
	prepared spaces.ScheduledRunAuthority
	err      error
	calls    int
	threadID string
}

func (f *fakeScheduledRunAuthority) ResolveScheduledRunAuthority(_ context.Context, _ spaces.ScheduledRunIntent, threadID string) (spaces.ScheduledRunAuthority, error) {
	f.calls++
	f.threadID = threadID
	return f.prepared, f.err
}

func TestScheduledRunExecutionFailsClosedWithoutAPreparedResolver(t *testing.T) {
	s := &Server{scheduleContractsEnabled: true}
	c, w := newGinJSONCtx(`{}`)
	if _, ok := s.boundScheduledRunExecution(c, requestedRun(), "thread-1", "", ""); ok {
		t.Fatal("an execution decision bound without a prepared-run resolver")
	}
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired resolver: status=%d, want 503", w.Code)
	}
}

func TestScheduledRunExecutionBindsThroughTheResolverOnly(t *testing.T) {
	// The handler judges expiry against the wall clock, so this contract is live now. It is
	// truncated to whole seconds because the claim travels as RFC3339.
	expiresAt := time.Now().UTC().Add(time.Hour).Truncate(time.Second)
	intent := requestedRun()
	prepared := preparedRun(intent)
	prepared.ContractDigest = runDigest('c')
	prepared.ContractExpiresAt = expiresAt

	fake := &fakeScheduledRunAuthority{prepared: prepared}
	s := &Server{scheduleContractsEnabled: true}
	s.SetScheduledRunAuthorityResolver(fake)

	// A matching claim binds the prepared contract, and the resolver is asked for the body's thread.
	c, w := newGinJSONCtx(`{}`)
	bound, ok := s.boundScheduledRunExecution(c, intent, " thread-1 ", runDigest('c'), expiresAt.Format(time.RFC3339))
	if !ok {
		t.Fatalf("a matching contract claim was refused: %d %s", w.Code, w.Body.String())
	}
	if fake.calls != 1 || fake.threadID != "thread-1" {
		t.Fatalf("resolver calls=%d thread=%q, want one call for thread-1", fake.calls, fake.threadID)
	}
	if bound.ContractDigest != runDigest('c') || !bound.ContractExpiresAt.Equal(expiresAt) {
		t.Fatalf("bound contract = %q at %s, want the prepared contract", bound.ContractDigest, bound.ContractExpiresAt)
	}

	// A claim that differs from the prepared contract is refused, not bound.
	c, w = newGinJSONCtx(`{}`)
	if _, ok := s.boundScheduledRunExecution(c, intent, "thread-1", runDigest('d'), expiresAt.Format(time.RFC3339)); ok {
		t.Fatal("a contract claim that differs from the prepared one was bound")
	}
	if w.Code != http.StatusForbidden {
		t.Fatalf("differing claim: status=%d, want 403", w.Code)
	}
}

func TestScheduledRunExecutionReadFailureFailsClosed(t *testing.T) {
	fake := &fakeScheduledRunAuthority{err: context.DeadlineExceeded}
	s := &Server{scheduleContractsEnabled: true}
	s.SetScheduledRunAuthorityResolver(fake)

	c, w := newGinJSONCtx(`{}`)
	if _, ok := s.boundScheduledRunExecution(c, requestedRun(), "thread-1", "", ""); ok {
		t.Fatal("an execution decision bound while the prepared run could not be read")
	}
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("read failure: status=%d, want 503", w.Code)
	}
}
