package spaces

import (
	"context"
	"errors"
	"testing"
)

// WS20-S1 live legs for the room browser run decision. Gated on
// TEST_DATABASE_URL: newPolicyAuditFixture and newRoomLiveFixture both skip
// (t.Skip) without it, so this file compiles and reports SKIP rather than
// running against a database.

// Migration 047 adds space_effect_policies.browser_entitled with
// DEFAULT FALSE; an existing org row (seeded the way a pre-047 deployment
// would, naming none of the entitlement columns) must read back FALSE.
func TestBrowserEntitledDefaultsFalseOnExistingOrgRowsLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()
	if _, err := f.pool.Exec(ctx, `
		INSERT INTO space_effect_policies (org_id, privacy_policy_ref, purpose, lawful_basis, privacy_class,
			retention_class, residency, deletion_scope)
		VALUES ($1, $2, 'assistant_collaboration', 'contract', 'internal', 'standard', 'swedencentral', 'space')`,
		f.orgID, "privacy:"+f.orgID+":1"); err != nil {
		t.Fatalf("seed a pre-047-shaped row: %v", err)
	}
	if got := f.stored(t).BrowserEntitled; got {
		t.Fatal("migration 047 must leave browser_entitled FALSE on an existing org row")
	}
}

// UpsertEffectPolicy round-trips browser_entitled like every other bit on
// EffectPolicy.
func TestUpsertEffectPolicyRoundTripsBrowserEntitledLive(t *testing.T) {
	f := newPolicyAuditFixture(t)
	ctx := context.Background()
	for _, want := range []bool{true, false} {
		policy := f.policy()
		policy.BrowserEntitled = want
		if _, err := f.repo.UpsertEffectPolicy(ctx, policy, operatorActor); err != nil {
			t.Fatalf("UpsertEffectPolicy(browser_entitled=%v): %v", want, err)
		}
		if got := f.stored(t).BrowserEntitled; got != want {
			t.Fatalf("browser_entitled round trip = %v, want %v", got, want)
		}
	}
}

// ResolveSharedBrowserRunDecisionEvidence is a write-class resolver (a run
// writes a result turn), so it never falls back to the archived state: an
// active member of an archived room gets ErrNoCurrentMembership, same as
// every other write-class resolver.
func TestResolveSharedBrowserRunDecisionEvidenceRefusesAnArchivedRoomLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "browserarchived")
	f.exec(t, `UPDATE space_effect_policies SET browser_entitled=TRUE WHERE org_id=$1`, f.org)

	// Sanity: the active room resolves.
	if _, err := f.repo.ResolveSharedBrowserRunDecisionEvidence(ctx, s.ref, f.org, s.editor); err != nil {
		t.Fatalf("active room browser run evidence: %v", err)
	}

	if _, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner)); err != nil {
		t.Fatalf("archive: %v", err)
	}
	if _, err := f.repo.ResolveSharedBrowserRunDecisionEvidence(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("archived room resolve = %v, want ErrNoCurrentMembership", err)
	}
}
