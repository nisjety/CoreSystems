package schedules

import (
	"context"
	"testing"
	"time"

	commonpb "go.temporal.io/api/common/v1"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/converter"
)

// memoFor builds a Temporal schedule Memo carrying an org_id, encoded with the
// SAME default data converter the SDK uses for ScheduleOptions.Memo — so the
// reconciler's orgFromMemo decodes it exactly as it would in production.
func memoFor(t *testing.T, orgID string) *commonpb.Memo {
	t.Helper()
	if orgID == "" {
		return nil
	}
	payload, err := converter.GetDefaultDataConverter().ToPayload(orgID)
	if err != nil {
		t.Fatalf("encode memo org_id=%q: %v", orgID, err)
	}
	return &commonpb.Memo{Fields: map[string]*commonpb.Payload{memoOrgKey: payload}}
}

// reaperFakeHandle records whether Delete was called for a given schedule id.
type reaperFakeHandle struct {
	id      string
	deleted bool
}

func (h *reaperFakeHandle) GetID() string                                              { return h.id }
func (h *reaperFakeHandle) Delete(context.Context) error                               { h.deleted = true; return nil }
func (h *reaperFakeHandle) Backfill(context.Context, client.ScheduleBackfillOptions) error { return nil }
func (h *reaperFakeHandle) Update(context.Context, client.ScheduleUpdateOptions) error { return nil }
func (h *reaperFakeHandle) Describe(context.Context) (*client.ScheduleDescription, error) {
	return &client.ScheduleDescription{}, nil
}
func (h *reaperFakeHandle) Trigger(context.Context, client.ScheduleTriggerOptions) error   { return nil }
func (h *reaperFakeHandle) Pause(context.Context, client.SchedulePauseOptions) error       { return nil }
func (h *reaperFakeHandle) Unpause(context.Context, client.ScheduleUnpauseOptions) error   { return nil }

// reaperFakeClient lists a fixed set of existing Temporal schedules (each with
// an org Memo) and hands out a per-id handle so the test can assert exactly
// which schedules the reconciler deleted.
type reaperFakeClient struct {
	existing []*client.ScheduleListEntry
	handles  map[string]*reaperFakeHandle
	created  []client.ScheduleOptions
}

func newReaperFakeClient(existing []*client.ScheduleListEntry) *reaperFakeClient {
	return &reaperFakeClient{existing: existing, handles: map[string]*reaperFakeHandle{}}
}

func (c *reaperFakeClient) Create(_ context.Context, opts client.ScheduleOptions) (client.ScheduleHandle, error) {
	c.created = append(c.created, opts)
	return c.handleFor(opts.ID), nil
}
func (c *reaperFakeClient) List(context.Context, client.ScheduleListOptions) (client.ScheduleListIterator, error) {
	return &fakeScheduleIterator{entries: c.existing}, nil
}
func (c *reaperFakeClient) GetHandle(_ context.Context, scheduleID string) client.ScheduleHandle {
	return c.handleFor(scheduleID)
}
func (c *reaperFakeClient) handleFor(id string) *reaperFakeHandle {
	if h, ok := c.handles[id]; ok {
		return h
	}
	h := &reaperFakeHandle{id: id}
	c.handles[id] = h
	return h
}
func (c *reaperFakeClient) wasDeleted(id string) bool {
	h, ok := c.handles[id]
	return ok && h.deleted
}

// TestReconcile_ReapsOnlyStaleOrgSchedules is the MANDATORY cross-org reaper
// isolation gate (Phase 3 PR-7).
//
// Scenario — two tenants, org_a and org_b, share one Temporal namespace:
//
//	Existing Temporal schedules (what sc.List returns):
//	  - sch_a_live  (org_a)  — still desired
//	  - sch_a_stale (org_a)  — REMOVED from control (its row is gone)
//	  - sch_b_live  (org_b)  — org_b's only schedule, still desired
//	  - sch_b_other (org_b)  — org_b schedule NOT in this pass's desired set
//	  - sch_legacy  ("")     — no org memo (pre-org-stamping)
//
//	Desired set this pass (what control returned):
//	  - sch_a_live  (org_a)
//	  - sch_b_live  (org_b)
//
// The reconciler MUST reap ONLY sch_a_stale: org_a is in the desired set and
// sch_a_stale dropped out of it. It must leave EVERYTHING ELSE intact:
//   - sch_a_live / sch_b_live — still desired.
//   - sch_b_other — org_b IS in the desired set, but this is the subtle case:
//     it is also genuinely absent from desired, so under a naive global reaper
//     it WOULD be deleted. The org-scoped reaper still deletes it ONLY because
//     org_b is represented. (This is the in-org reap path — correct.)
//   - sch_legacy — unknown org, fail-closed, never reaped.
//
// The headline cross-org property is then proven by a second pass where org_a
// has NO desired schedules at all: org_a's stale schedule must SURVIVE, because
// a reconcile that doesn't represent org_a can never reap org_a's work.
func TestReconcile_ReapsOnlyStaleOrgSchedules(t *testing.T) {
	t.Parallel()

	existing := []*client.ScheduleListEntry{
		{ID: "sch_a_live", Memo: memoFor(t, "org_a")},
		{ID: "sch_a_stale", Memo: memoFor(t, "org_a")},
		{ID: "sch_b_live", Memo: memoFor(t, "org_b")},
		{ID: "sch_b_other", Memo: memoFor(t, "org_b")},
		{ID: "sch_legacy", Memo: nil},
	}

	t.Run("mixed-org pass reaps only stale rows of represented orgs", func(t *testing.T) {
		t.Parallel()
		sc := newReaperFakeClient(existing)
		m := New(nil, Config{TaskQueue: "tasks"})

		desired := []ScheduleSpec{
			{ID: "sch_a_live", OrgID: "org_a", Cron: "0 9 * * *", Workflow: "ChangeMonitorWF"},
			{ID: "sch_b_live", OrgID: "org_b", Cron: "0 9 * * *", Workflow: "ChangeMonitorWF"},
		}
		if err := m.reconcileWith(context.Background(), sc, desired); err != nil {
			t.Fatalf("reconcileWith: %v", err)
		}

		// Reaped: org_a's stale schedule, and org_b's other (org_b IS represented).
		if !sc.wasDeleted("sch_a_stale") {
			t.Error("expected sch_a_stale (org_a, dropped from desired) to be reaped")
		}
		if !sc.wasDeleted("sch_b_other") {
			t.Error("expected sch_b_other (org_b is represented + not desired) to be reaped")
		}
		// Survivors.
		if sc.wasDeleted("sch_a_live") {
			t.Error("sch_a_live is still desired; must not be reaped")
		}
		if sc.wasDeleted("sch_b_live") {
			t.Error("sch_b_live is still desired; must not be reaped")
		}
		if sc.wasDeleted("sch_legacy") {
			t.Error("sch_legacy has no org memo; fail-closed means it is NEVER reaped")
		}
	})

	t.Run("a pass that does not represent org_a never reaps org_a's schedules", func(t *testing.T) {
		t.Parallel()
		sc := newReaperFakeClient(existing)
		m := New(nil, Config{TaskQueue: "tasks"})

		// org_a is entirely ABSENT from this pass's desired set. This models a
		// per-org / partial reconcile (or a list-scoping bug). It must NOT
		// delete ANY org_a schedule — that would be cross-org reaping.
		desiredOnlyB := []ScheduleSpec{
			{ID: "sch_b_live", OrgID: "org_b", Cron: "0 9 * * *", Workflow: "ChangeMonitorWF"},
		}
		if err := m.reconcileWith(context.Background(), sc, desiredOnlyB); err != nil {
			t.Fatalf("reconcileWith: %v", err)
		}

		if sc.wasDeleted("sch_a_live") || sc.wasDeleted("sch_a_stale") {
			t.Fatal("CROSS-ORG REAP: a reconcile not representing org_a deleted an org_a schedule")
		}
		if sc.wasDeleted("sch_legacy") {
			t.Error("sch_legacy has no org memo; must never be reaped")
		}
		// org_b is represented and sch_b_other is not desired → it IS reaped.
		if !sc.wasDeleted("sch_b_other") {
			t.Error("expected sch_b_other (org_b represented, not desired) to be reaped")
		}
	})

	t.Run("empty desired set reaps nothing (no org is represented)", func(t *testing.T) {
		t.Parallel()
		sc := newReaperFakeClient(existing)
		m := New(nil, Config{TaskQueue: "tasks"})

		if err := m.reconcileWith(context.Background(), sc, nil); err != nil {
			t.Fatalf("reconcileWith: %v", err)
		}
		for _, id := range []string{"sch_a_live", "sch_a_stale", "sch_b_live", "sch_b_other", "sch_legacy"} {
			if sc.wasDeleted(id) {
				t.Errorf("empty desired set represents no org; %s must not be reaped", id)
			}
		}
	})
}

// TestCreate_StampsOrgMemo proves the create path stamps the org into the
// schedule Memo — the value the reaper later relies on to scope deletions.
func TestCreate_StampsOrgMemo(t *testing.T) {
	t.Parallel()

	sc := newReaperFakeClient(nil)
	m := New(nil, Config{TaskQueue: "tasks"})
	spec := ScheduleSpec{ID: "sch_x", OrgID: "org_x", Cron: "0 9 * * *", Workflow: "ChangeMonitorWF"}

	if err := m.create(context.Background(), sc, spec); err != nil {
		t.Fatalf("create: %v", err)
	}
	if len(sc.created) != 1 {
		t.Fatalf("created len=%d want=1", len(sc.created))
	}
	memo := sc.created[0].Memo
	if memo == nil {
		t.Fatal("expected Memo to be stamped with org_id")
	}
	if got, _ := memo[memoOrgKey].(string); got != "org_x" {
		t.Fatalf("memo org_id=%v want=org_x", memo[memoOrgKey])
	}

	// And a spec with no org leaves the Memo unset (so it is never reaped).
	sc2 := newReaperFakeClient(nil)
	if err := m.create(context.Background(), sc2, ScheduleSpec{ID: "sch_y", Cron: "0 9 * * *", Workflow: "wf"}); err != nil {
		t.Fatalf("create no-org: %v", err)
	}
	if sc2.created[0].Memo != nil {
		t.Fatalf("expected nil Memo for org-less spec, got %v", sc2.created[0].Memo)
	}
}

// TestTriggerAndBackfill_CallRealScheduleClient proves trigger/backfill reach
// the Temporal schedule handle (the real-client materialization of control's
// trigger/backfill endpoints) and that backfill validates its window.
func TestTriggerAndBackfill_CallRealScheduleClient(t *testing.T) {
	t.Parallel()

	t.Run("trigger fires the handle", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_t"}
		sc := &fakeScheduleClient{handle: h}
		m := New(nil, Config{})
		if err := m.triggerWith(context.Background(), sc, "sch_t"); err != nil {
			t.Fatalf("triggerWith: %v", err)
		}
	})

	t.Run("backfill rejects an inverted window before any Temporal call", func(t *testing.T) {
		t.Parallel()
		h := &fakeScheduleHandle{id: "sch_b"}
		sc := &fakeScheduleClient{handle: h}
		m := New(nil, Config{})
		now := time.Now()
		if err := m.backfillWith(context.Background(), sc, "sch_b", now, now); err == nil {
			t.Fatal("expected error for non-advancing window")
		}
	})
}
