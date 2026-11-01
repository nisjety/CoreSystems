package spaces

import (
	"context"
	"errors"
	"fmt"
	"testing"
)

// WS03-S3 live proofs for the org-admin entitlement writer. Gated on
// TEST_DATABASE_URL (see newPolicyAuditFixture); the events table is
// append-only, so these leave rows behind under a unique org id.

var orgAdminActor = EffectPolicyActor{Type: "user", ID: "ws03s3-admin", OrgRole: "admin"}

func (f *policyAuditFixture) seedPolicy(t *testing.T, mutate func(*EffectPolicy)) {
	t.Helper()
	policy := f.policy()
	if mutate != nil {
		mutate(&policy)
	}
	if _, err := f.repo.UpsertEffectPolicy(context.Background(), policy, operatorActor); err != nil {
		t.Fatalf("seed policy: %v", err)
	}
}

func (f *policyAuditFixture) revisions(t *testing.T) (policyRev, entRev int64) {
	t.Helper()
	if err := f.pool.QueryRow(context.Background(),
		`SELECT policy_revision, entitlement_revision FROM space_effect_policies WHERE org_id = $1`, f.orgID).Scan(&policyRev, &entRev); err != nil {
		t.Fatalf("read revisions: %v", err)
	}
	return policyRev, entRev
}

func (f *policyAuditFixture) space(t *testing.T, ref, state string) string {
	t.Helper()
	ctx := context.Background()
	full := fmt.Sprintf("%s-%s", f.orgID, ref)
	if _, err := f.pool.Exec(ctx, `
		INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		    application_lifecycle_revision, registration_state)
		VALUES ($1,$2,'room','ws03s3-owner',1,$3)`, full, f.orgID, state); err != nil {
		t.Fatalf("seed Space: %v", err)
	}
	if _, err := f.pool.Exec(ctx, `INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, full); err != nil {
		t.Fatalf("seed Space revisions: %v", err)
	}
	t.Cleanup(func() {
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_authority_revisions WHERE space_ref = $1`, full)
		_, _ = f.pool.Exec(ctx, `DELETE FROM registered_spaces WHERE space_ref = $1`, full)
	})
	return full
}

func (f *policyAuditFixture) authorityRevision(t *testing.T, spaceRef string) (authority, entitlement int64) {
	t.Helper()
	if err := f.pool.QueryRow(context.Background(),
		`SELECT authority_revision, entitlement_revision FROM space_authority_revisions WHERE space_ref = $1`, spaceRef).Scan(&authority, &entitlement); err != nil {
		t.Fatalf("read Space revisions: %v", err)
	}
	return authority, entitlement
}

func (f *policyAuditFixture) apply(key string, expected int64, preset string, entitlements map[string]bool) (ApplyOrgEntitlementsResult, error) {
	return f.repo.ApplyOrgEntitlements(context.Background(), ApplyOrgEntitlementsRequest{
		OrgID: f.orgID, Actor: orgAdminActor, IdempotencyKey: key, ExpectedEntitlementRevision: expected,
		PresetID: preset, Entitlements: entitlements,
	})
}

func TestApplyOrgEntitlementsMissingRowInsertsNothingLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	_, err := f.apply("ws03s3-missing-1", 1, PresetConversation, nil)
	if !errors.Is(err, ErrNoOrgEffectPolicy) {
		t.Fatalf("missing row: err = %v, want ErrNoOrgEffectPolicy", err)
	}
	var rows int
	if err := f.pool.QueryRow(context.Background(), `SELECT count(*) FROM space_effect_policies WHERE org_id = $1`, f.orgID).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("the writer inserted a row: rows=%d err=%v", rows, err)
	}
	if events, outbox := f.counts(t); events != 0 || outbox != 0 {
		t.Fatalf("rows written for a missing policy: events=%d outbox=%d", events, outbox)
	}
}

func TestApplyOrgEntitlementsAdvancesRevisionsOnlyOnActiveSpacesLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil)
	active := f.space(t, "active", "active")
	deleting := f.space(t, "deleting", "deleting")
	policyRev, entRev := f.revisions(t)
	activeAuthority, activeEnt := f.authorityRevision(t, active)
	deletingAuthority, deletingEnt := f.authorityRevision(t, deleting)

	result, err := f.apply("ws03s3-fence-1", entRev, PresetConversation, nil)
	if err != nil || !result.Changed || result.Replayed {
		t.Fatalf("apply: result=%+v err=%v", result, err)
	}
	if result.SpacesFenced != 1 {
		t.Fatalf("spaces fenced = %d, want exactly the one active Space", result.SpacesFenced)
	}
	nextPolicyRev, nextEntRev := f.revisions(t)
	if nextEntRev != entRev+1 || nextPolicyRev != policyRev {
		t.Fatalf("revisions: entitlement %d->%d policy %d->%d; only the entitlement revision may advance", entRev, nextEntRev, policyRev, nextPolicyRev)
	}
	if a, e := f.authorityRevision(t, active); a != activeAuthority+1 || e != activeEnt+1 {
		t.Fatalf("active Space not fenced: authority %d->%d entitlement %d->%d", activeAuthority, a, activeEnt, e)
	}
	if a, e := f.authorityRevision(t, deleting); a != deletingAuthority || e != deletingEnt {
		t.Fatalf("a deleting Space was fenced: authority %d->%d entitlement %d->%d", deletingAuthority, a, deletingEnt, e)
	}
	stored := f.stored(t)
	if !stored.ThreadCreateEntitled || !stored.ThreadReadEntitled || stored.RetrievalReadEntitled {
		t.Fatalf("preset not applied: %+v", EntitlementValues(stored))
	}
	// Privacy columns untouched.
	if want := f.policy(); stored.PrivacyPolicyRef != want.PrivacyPolicyRef || stored.Purpose != want.Purpose ||
		stored.ZeroDataRetention != want.ZeroDataRetention || stored.Residency != want.Residency {
		t.Fatalf("privacy columns changed by an org-admin write: %+v", stored)
	}
}

func TestApplyOrgEntitlementsReplayAndIdempotencyConflictLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil)
	_, entRev := f.revisions(t)

	first, err := f.apply("ws03s3-idem-1", entRev, PresetConversation, nil)
	if err != nil || !first.Changed {
		t.Fatalf("first apply: %+v %v", first, err)
	}
	eventsAfterFirst, outboxAfterFirst := f.counts(t)

	// Same key and same request: replayed, nothing new written, even though the
	// expected revision is now stale.
	replay, err := f.apply("ws03s3-idem-1", entRev, PresetConversation, nil)
	if err != nil || !replay.Replayed {
		t.Fatalf("replay: %+v %v", replay, err)
	}
	if events, outbox := f.counts(t); events != eventsAfterFirst || outbox != outboxAfterFirst {
		t.Fatalf("a replay wrote rows: events %d->%d outbox %d->%d", eventsAfterFirst, events, outboxAfterFirst, outbox)
	}

	// Same key, different request: idempotency conflict.
	_, err = f.apply("ws03s3-idem-1", entRev, PresetConversationKnowledge, nil)
	policyErr := asOrgPolicyError(t, err)
	if policyErr.Code != OrgPolicyCodeIdempotencyConflict {
		t.Fatalf("different digest under one key: %+v", policyErr)
	}
}

func TestApplyOrgEntitlementsStaleRevisionConflictLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil)
	_, entRev := f.revisions(t)

	_, err := f.apply("ws03s3-stale-1", entRev+7, PresetConversation, nil)
	policyErr := asOrgPolicyError(t, err)
	if policyErr.Code != OrgPolicyCodeRevisionConflict || policyErr.Details["current_entitlement_revision"] != entRev {
		t.Fatalf("stale revision: %+v, want a conflict carrying revision %d", policyErr, entRev)
	}
	if _, now := f.revisions(t); now != entRev {
		t.Fatalf("a refused write moved the revision: %d -> %d", entRev, now)
	}
}

func TestApplyOrgEntitlementsUnchangedRequestWritesNothingLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil)
	_, entRev := f.revisions(t)
	before, beforeOutbox := f.counts(t)
	result, err := f.apply("ws03s3-nochange-1", entRev, PresetLocked, nil) // already all off
	if err != nil || result.Changed || result.Replayed {
		t.Fatalf("unchanged apply: %+v %v", result, err)
	}
	if events, outbox := f.counts(t); events != before || outbox != beforeOutbox {
		t.Fatalf("an unchanged request wrote rows: events %d->%d outbox %d->%d", before, events, beforeOutbox, outbox)
	}
	if _, now := f.revisions(t); now != entRev {
		t.Fatalf("an unchanged request moved the revision: %d -> %d", entRev, now)
	}
}

func TestApplyOrgEntitlementsForcedOutboxFailureRollsBackLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil)
	space := f.space(t, "active", "active")
	_, entRev := f.revisions(t)
	authority, _ := f.authorityRevision(t, space)
	events, _ := f.counts(t)

	f.outbox.fail = true
	if _, err := f.apply("ws03s3-rollback-1", entRev, PresetConversation, nil); err == nil {
		t.Fatal("an org-admin write with a failing outbox succeeded")
	}
	f.outbox.fail = false

	if stored := f.stored(t); stored.ThreadCreateEntitled || stored.ThreadReadEntitled {
		t.Fatalf("the entitlement change survived a failed audit: %+v", EntitlementValues(stored))
	}
	if _, now := f.revisions(t); now != entRev {
		t.Fatalf("the revision advanced despite the rollback: %d -> %d", entRev, now)
	}
	if got, _ := f.authorityRevision(t, space); got != authority {
		t.Fatalf("the Space fence survived a rollback: %d -> %d", authority, got)
	}
	if now, _ := f.counts(t); now != events {
		t.Fatalf("an event row survived a rollback: %d -> %d", events, now)
	}
}

func TestApplyOrgEntitlementsLeavesResourceGrantsAloneLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil)
	ctx := context.Background()
	if _, err := f.pool.Exec(ctx, `
		INSERT INTO resource_grants (org_id, resource_type, resource_id, subject_type, subject_id, role)
		VALUES ($1, 'document', 'doc-ws03s3', 'user', 'user-ws03s3', 'view')`, f.orgID); err != nil {
		t.Fatalf("seed grant: %v", err)
	}
	t.Cleanup(func() { _, _ = f.pool.Exec(ctx, `DELETE FROM resource_grants WHERE org_id = $1`, f.orgID) })
	_, entRev := f.revisions(t)
	if _, err := f.apply("ws03s3-grants-1", entRev, PresetConversation, nil); err != nil {
		t.Fatalf("apply: %v", err)
	}
	var grants int
	if err := f.pool.QueryRow(ctx, `SELECT count(*) FROM resource_grants WHERE org_id = $1`, f.orgID).Scan(&grants); err != nil || grants != 1 {
		t.Fatalf("resource grants touched by a policy write: count=%d err=%v", grants, err)
	}
}

func TestListOrgPolicyEventsNewestFirstWithCursorLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	f.seedPolicy(t, nil) // event 1: operator_created
	ctx := context.Background()
	steps := []string{PresetConversation, PresetConversationKnowledge, PresetConversation}
	for i, preset := range steps {
		_, entRev := f.revisions(t)
		if _, err := f.apply(fmt.Sprintf("ws03s3-hist-%d", i), entRev, preset, nil); err != nil {
			t.Fatalf("apply %d: %v", i, err)
		}
	}

	page1, next, err := f.repo.ListOrgPolicyEvents(ctx, f.orgID, 2, 0)
	if err != nil || len(page1) != 2 || next == 0 {
		t.Fatalf("page 1: %d events next=%d err=%v", len(page1), next, err)
	}
	page2, next2, err := f.repo.ListOrgPolicyEvents(ctx, f.orgID, 2, next)
	if err != nil || len(page2) != 2 || next2 != 0 {
		t.Fatalf("page 2: %d events next=%d err=%v", len(page2), next2, err)
	}
	all := append(append([]OrgPolicyEvent{}, page1...), page2...)
	for i := 1; i < len(all); i++ {
		if all[i-1].EventID <= all[i].EventID {
			t.Fatalf("history is not newest first: %d then %d", all[i-1].EventID, all[i].EventID)
		}
	}
	if all[0].ChangeKind != PolicyChangePresetApplied || all[0].PresetID != PresetConversation || all[0].Source != PolicyEventSourceOrgAdmin ||
		all[0].Actor.ID != "ws03s3-admin" || all[0].Actor.Type != "user" {
		t.Fatalf("newest event = %+v", all[0])
	}
	if last := all[len(all)-1]; last.ChangeKind != PolicyChangeOperatorCreated || last.Source != PolicyEventSourceOperator {
		t.Fatalf("oldest event = %+v", last)
	}

	view, err := f.repo.GetOrgPolicy(ctx, f.orgID)
	if err != nil {
		t.Fatalf("GetOrgPolicy: %v", err)
	}
	if view.LastChange == nil || view.LastChange.EventID != all[0].EventID {
		t.Fatalf("last_change = %+v, want the newest event", view.LastChange)
	}
	if view.MatchesPreset == nil || *view.MatchesPreset != PresetConversation {
		t.Fatalf("matches_preset = %v", view.MatchesPreset)
	}
}

func TestGetOrgPolicyNotProvisionedLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	view, err := f.repo.GetOrgPolicy(context.Background(), f.orgID)
	if err != nil {
		t.Fatalf("GetOrgPolicy: %v", err)
	}
	if view.State != OrgPolicyStateNotProvisioned || view.Attestation != nil {
		t.Fatalf("view = %+v", view)
	}
	var rows int
	if err := f.pool.QueryRow(context.Background(), `SELECT count(*) FROM space_effect_policies WHERE org_id = $1`, f.orgID).Scan(&rows); err != nil || rows != 0 {
		t.Fatalf("a read inserted a policy row: rows=%d err=%v", rows, err)
	}
}
