package spaces

import (
	"context"
	"errors"
	"testing"
)

// WS12-S5 live legs: the shared revise resolver, the operator policy writer and
// the member hints. Gated on TEST_DATABASE_URL through roomLiveFixture.

func (f *roomLiveFixture) lifecycleRoom(t *testing.T, name string) roomStaff {
	t.Helper()
	s := f.archiveFixtureRoom(t, name)
	f.exec(t, `UPDATE space_effect_policies SET message_revise_entitled=TRUE, message_moderate_entitled=TRUE WHERE org_id=$1`, f.org)
	t.Cleanup(func() {
		ctx := context.Background()
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_message_policy_events WHERE org_id=$1`, f.org)
		_, _ = f.pool.Exec(ctx, `DELETE FROM space_message_policies WHERE org_id=$1`, f.org)
	})
	return s
}

func (f *roomLiveFixture) policyEventCount(t *testing.T) int {
	t.Helper()
	var n int
	if err := f.pool.QueryRow(context.Background(), `SELECT count(*) FROM space_message_policy_events WHERE org_id=$1`, f.org).Scan(&n); err != nil {
		t.Fatalf("count policy events: %v", err)
	}
	return n
}

func TestSharedReviseResolverRefusesArchivedRoomsAndRemovedMembersLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.lifecycleRoom(t, "rvres")

	evidence, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if err != nil {
		t.Fatalf("active editor: %v", err)
	}
	if !evidence.MessageReviseEntitled || !evidence.MessageModerateEntitled || evidence.OriginalRetention != OriginalRetentionPurge {
		t.Fatalf("evidence lifecycle bits: %+v", evidence)
	}
	if evidence.Membership.Role != "editor" || evidence.ResourceAuthorizationRef == "" {
		t.Fatalf("evidence: %+v", evidence)
	}
	// A viewer resolves the facts but the role floor refuses the evidence.
	if _, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.viewer); err == nil {
		t.Fatal("a viewer must not resolve revise evidence")
	} else if denial, ok := AsDenial(err); !ok || denial.Code != DenialRoleBelowFloor {
		t.Fatalf("viewer refusal = %v", err)
	}
	// The entitlement is not borrowed from thread_create.
	f.exec(t, `UPDATE space_effect_policies SET message_revise_entitled=FALSE WHERE org_id=$1`, f.org)
	if _, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.editor); err == nil {
		t.Fatal("revise must not fall back to thread_create")
	} else if denial, ok := AsDenial(err); !ok || denial.Code != DenialEntitlementOff {
		t.Fatalf("entitlement-off refusal = %v", err)
	}
	f.exec(t, `UPDATE space_effect_policies SET message_revise_entitled=TRUE WHERE org_id=$1`, f.org)

	// A member removed from the current audience stops resolving.
	f.exec(t, `DELETE FROM space_recipient_audience_members WHERE space_ref=$1 AND subject_id=$2`, s.ref, s.editor)
	if _, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.editor); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("removed audience member: %v", err)
	}
	// An archived room cannot be edited, even for a current audience member.
	f.exec(t, `UPDATE registered_spaces SET registration_state='archived' WHERE space_ref=$1`, s.ref)
	if _, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.manager); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("archived room: %v", err)
	}
	// Another organization sees nothing.
	if _, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.other, s.manager); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("wrong org: %v", err)
	}
}

func TestUpsertMessagePolicyWritesOneEventPerAttemptAndDrivesTheResolverLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.lifecycleRoom(t, "rvpol")

	// No row: the safe default is purge.
	evidence, err := f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if err != nil || evidence.OriginalRetention != OriginalRetentionPurge {
		t.Fatalf("default: %+v %v", evidence, err)
	}
	if got := f.policyEventCount(t); got != 0 {
		t.Fatalf("events before any write = %d", got)
	}

	policy := MessagePolicy{OrgID: f.org, OriginalRetention: OriginalRetentionRetain}
	for attempt := 1; attempt <= 2; attempt++ {
		if err := f.repo.UpsertMessagePolicy(ctx, policy, "control-space-policy"); err != nil {
			t.Fatalf("attempt %d: %v", attempt, err)
		}
		// An unchanged value still records its attempt.
		if got := f.policyEventCount(t); got != attempt {
			t.Fatalf("after attempt %d events = %d", attempt, got)
		}
	}
	evidence, err = f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if err != nil || evidence.OriginalRetention != OriginalRetentionRetain {
		t.Fatalf("after retain: %+v %v", evidence, err)
	}
	if err := f.repo.UpsertMessagePolicy(ctx, MessagePolicy{OrgID: f.org, OriginalRetention: OriginalRetentionPurge}, "control-space-policy"); err != nil {
		t.Fatalf("back to purge: %v", err)
	}
	evidence, _ = f.repo.ResolveSharedThreadReviseDecisionEvidence(ctx, s.ref, f.org, s.editor)
	if evidence.OriginalRetention != OriginalRetentionPurge || f.policyEventCount(t) != 3 {
		t.Fatalf("purge again: %q events=%d", evidence.OriginalRetention, f.policyEventCount(t))
	}

	// Invalid input writes nothing, not even an event.
	if err := f.repo.UpsertMessagePolicy(ctx, MessagePolicy{OrgID: f.org, OriginalRetention: "keep"}, "x"); err == nil {
		t.Fatal("an unknown rule must be refused")
	}
	if err := f.repo.UpsertMessagePolicy(ctx, policy, "  "); err == nil {
		t.Fatal("a blank actor must be refused")
	}
	if got := f.policyEventCount(t); got != 3 {
		t.Fatalf("refused writes recorded events: %d", got)
	}
}

func TestMessagePolicyHintsMaskUnderZDRAndForDMModerateLive(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.lifecycleRoom(t, "rvhint")

	hints := func(subject string) MessagePolicyHints {
		t.Helper()
		h, err := f.repo.MessagePolicyHints(ctx, s.ref, f.org, subject)
		if err != nil {
			t.Fatalf("hints for %s: %v", subject, err)
		}
		return h
	}
	if h := hints(s.viewer); h.ReviseEntitled || h.ModerateEntitled || h.OriginalRetention != OriginalRetentionPurge {
		t.Fatalf("viewer hints = %+v", h)
	}
	if h := hints(s.editor); !h.ReviseEntitled || h.ModerateEntitled {
		t.Fatalf("editor hints = %+v", h)
	}
	for _, subject := range []string{s.manager, s.owner} {
		if h := hints(subject); !h.ReviseEntitled || !h.ModerateEntitled {
			t.Fatalf("%s hints = %+v", subject, h)
		}
	}
	// The retention hint follows the organization rule.
	if err := f.repo.UpsertMessagePolicy(ctx, MessagePolicy{OrgID: f.org, OriginalRetention: OriginalRetentionRetain}, "control-space-policy"); err != nil {
		t.Fatalf("policy: %v", err)
	}
	if h := hints(s.editor); h.OriginalRetention != OriginalRetentionRetain {
		t.Fatalf("retention hint = %+v", h)
	}
	// A non-member learns nothing.
	if _, err := f.repo.MessagePolicyHints(ctx, s.ref, f.org, "ws04s4-nobody-"+f.suffix); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("non-member: %v", err)
	}

	// An archived room still answers (hints are read-class).
	f.exec(t, `UPDATE registered_spaces SET registration_state='archived' WHERE space_ref=$1`, s.ref)
	if h := hints(s.editor); !h.ReviseEntitled {
		t.Fatalf("archived room hints = %+v", h)
	}
	f.exec(t, `UPDATE registered_spaces SET registration_state='active' WHERE space_ref=$1`, s.ref)

	// A DM kind never offers moderation. The kind CHECK may refuse the update on
	// a hand-seeded room; the pure matrix already covers the decision itself.
	if _, err := f.pool.Exec(ctx, `UPDATE registered_spaces SET space_kind='dm' WHERE space_ref=$1`, s.ref); err == nil {
		if h := hints(s.owner); h.ModerateEntitled {
			t.Fatalf("a DM must never offer moderation: %+v", h)
		}
		f.exec(t, `UPDATE registered_spaces SET space_kind='room' WHERE space_ref=$1`, s.ref)
	} else {
		t.Logf("DM kind not seedable by hand here (%v); covered by the pure permission matrix", err)
	}

	// Zero Data Retention masks both hints, matching the refusal the decision would give.
	f.exec(t, `UPDATE space_effect_policies SET zero_data_retention=TRUE WHERE org_id=$1`, f.org)
	if h := hints(s.owner); h.ReviseEntitled || h.ModerateEntitled || h.OriginalRetention != OriginalRetentionRetain {
		t.Fatalf("ZDR hints = %+v", h)
	}
}
