package spaces

import (
	"context"
	"errors"
	"strings"
	"testing"
)

// WS14-S1 live proof of ResolveSharedImportDecisionEvidence. Gated on
// TEST_DATABASE_URL through newRoomLiveFixture (skipped without it).

func TestResolveSharedImportDecisionEvidenceLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "shimport")

	// A current member in the audience resolves, with the import resource ref
	// and not the retrieval or thread one.
	evidence, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if err != nil {
		t.Fatalf("member: %v", err)
	}
	if evidence.Membership.Role != "editor" || evidence.Membership.Kind != KindRoom || !evidence.ImportWriteEntitled ||
		!strings.Contains(evidence.ResourceAuthorizationRef, ":ingestion-import:") {
		t.Fatalf("evidence = %+v", evidence)
	}
	if _, err := IssueSharedImportDecision(evidence, sharedImportRequest(SpaceUploadSourceType), sharedImportNow); err != nil {
		t.Fatalf("an editor in the audience must be issuable: %v", err)
	}
	// A viewer resolves too (the role floor is the issuer's), but cannot be issued.
	viewer, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.viewer)
	if err != nil {
		t.Fatalf("viewer: %v", err)
	}
	_ = viewer

	// Another organization's id finds nothing; so does an unknown Space.
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.other, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("other org: err = %v, want ErrNoCurrentMembership", err)
	}
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, "ws04s4-nope-"+f.suffix, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("unknown Space: err = %v", err)
	}

	// A member outside the published audience (added after it) resolves nothing.
	late := f.user(t, "shimport-late", f.org)
	f.member(t, s.ref, late, "editor", SourceMemberOps)
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, late); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("stale audience: err = %v, want ErrNoCurrentMembership", err)
	}

	// An archived room is not writable.
	if _, err := f.repo.ArchiveSpace(ctx, f.archiveReq(s.ref, s.owner)); err != nil {
		t.Fatalf("archive: %v", err)
	}
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("archived room: err = %v, want ErrNoCurrentMembership", err)
	}
	if _, err := f.repo.UnarchiveSpace(ctx, f.archiveReq(s.ref, s.owner)); err != nil {
		t.Fatalf("unarchive: %v", err)
	}
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.editor); err != nil {
		t.Fatalf("restored room: %v", err)
	}

	// A removed member stops resolving at once.
	f.exec(t, `UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, s.ref, s.editor)
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("removed member: err = %v, want ErrNoCurrentMembership", err)
	}

	// The entitlement off makes the resolver fail with the typed refusal.
	f.exec(t, `UPDATE space_effect_policies SET import_write_entitled=FALSE WHERE org_id=$1`, f.org)
	_, err = f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.manager)
	wantDenial(t, err, DenialEntitlementOff)
}

func TestResolveSharedImportDecisionEvidenceRefusesAPersonalSpaceLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.cleanupPolicy(t)
	ctx := context.Background()
	s := f.archiveFixtureRoom(t, "shimportp")
	// Turn the room into a personal Space: the shared resolver must not serve it.
	f.exec(t, `UPDATE registered_spaces SET space_kind='personal' WHERE space_ref=$1`, s.ref)
	if _, err := f.repo.ResolveSharedImportDecisionEvidence(ctx, s.ref, f.org, s.owner); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("personal Space: err = %v, want ErrNoCurrentMembership", err)
	}
}
