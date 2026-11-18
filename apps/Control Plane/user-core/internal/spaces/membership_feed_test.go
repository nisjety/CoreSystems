package spaces

import (
	"context"
	"errors"
	"testing"
	"time"
)

// WS15-S11: the room membership feed (membership_feed.go). The pure tier
// covers NormalizeMembershipFeedParams; the live tier (gated on
// TEST_DATABASE_URL via newRoomLiveFixture, skipped without it) covers what
// only the repository's SQL can answer: the inclusive/exclusive interval,
// the limit+1 truncation, the created_at/event_id ordering, the
// membership-required refusal, and today's actual (gap) history-floor
// behavior.

func mustParseRFC3339(t *testing.T, raw string) time.Time {
	t.Helper()
	parsed, err := time.Parse(time.RFC3339Nano, raw)
	if err != nil {
		t.Fatalf("parse %q: %v", raw, err)
	}
	return parsed.UTC()
}

// TestMembershipFeedIntervalAndLimit has two tiers: a pure subtest for
// NormalizeMembershipFeedParams (always runs), and a live subtest for the
// repository's interval/limit/order behavior (skipped without
// TEST_DATABASE_URL).
func TestMembershipFeedIntervalAndLimit(t *testing.T) {
	t.Run("normalize params", func(t *testing.T) {
		// Defaults: no after/before/limit means unbounded and the default limit.
		p, err := NormalizeMembershipFeedParams("", "", "")
		if err != nil || p.Limit != MembershipFeedDefaultLimit || !p.After.IsZero() || !p.Before.IsZero() {
			t.Fatalf("defaults: %+v, err=%v", p, err)
		}

		after, before := "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"
		p, err = NormalizeMembershipFeedParams(after, before, "50")
		if err != nil {
			t.Fatalf("valid interval: %v", err)
		}
		if !p.After.Equal(mustParseRFC3339(t, after)) || !p.Before.Equal(mustParseRFC3339(t, before)) || p.Limit != 50 {
			t.Fatalf("valid interval parsed as %+v", p)
		}

		// Malformed RFC3339 on either bound.
		for _, bad := range []string{"not-a-time", "2026-01-01", "2026-01-01 00:00:00Z", "2026-13-01T00:00:00Z"} {
			if _, err := NormalizeMembershipFeedParams(bad, "", ""); !errors.Is(err, ErrInvalidMembershipFeed) {
				t.Errorf("after=%q: err=%v, want ErrInvalidMembershipFeed", bad, err)
			}
			if _, err := NormalizeMembershipFeedParams("", bad, ""); !errors.Is(err, ErrInvalidMembershipFeed) {
				t.Errorf("before=%q: err=%v, want ErrInvalidMembershipFeed", bad, err)
			}
		}

		// after >= before is invalid, whether equal or reversed.
		if _, err := NormalizeMembershipFeedParams(before, before, ""); !errors.Is(err, ErrInvalidMembershipFeed) {
			t.Error("after == before must be invalid")
		}
		if _, err := NormalizeMembershipFeedParams(before, after, ""); !errors.Is(err, ErrInvalidMembershipFeed) {
			t.Error("after later than before must be invalid")
		}
		// after with no before (open-ended) is fine.
		if _, err := NormalizeMembershipFeedParams(after, "", ""); err != nil {
			t.Errorf("after with no before: unexpected err %v", err)
		}

		// limit: 0, over the ceiling, and non-numeric are invalid.
		for _, bad := range []string{"0", "101", "abc", "-1", "1.5", "1e2"} {
			if _, err := NormalizeMembershipFeedParams("", "", bad); !errors.Is(err, ErrInvalidMembershipFeed) {
				t.Errorf("limit=%q: err=%v, want ErrInvalidMembershipFeed", bad, err)
			}
		}
		// The boundary values, and a trimmed one, are accepted.
		for _, ok := range []string{"1", "100", "  50  "} {
			if _, err := NormalizeMembershipFeedParams("", "", ok); err != nil {
				t.Errorf("limit=%q: unexpected err %v", ok, err)
			}
		}
		if MembershipFeedMaxLimit != 100 || MembershipFeedDefaultLimit != 100 {
			t.Fatalf("limit constants = %d/%d, want 100/100", MembershipFeedDefaultLimit, MembershipFeedMaxLimit)
		}
	})

	t.Run("interval, limit and order", func(t *testing.T) {
		f := newRoomLiveFixture(t)
		ctx := context.Background()
		owner := f.user(t, "feedlimit-owner", f.org)
		ref := f.room(t, "feedlimit", owner)

		base := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)
		at := func(offsetSeconds int) time.Time { return base.Add(time.Duration(offsetSeconds) * time.Second) }
		seed := func(id string, createdAt time.Time) string {
			eventID := "sme_ws15s11_" + id + "_" + f.suffix
			f.exec(t, `INSERT INTO space_membership_events
				(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, created_at)
				VALUES ($1,$2,$3,1,'member_added','user','ws15s11-actor',$4)`,
				eventID, ref, f.org, createdAt)
			return eventID
		}

		seed("e1", at(0)) // excluded by After below; kept only to prove the lower bound bites.
		e2 := seed("e2", at(1))
		// tieA and tieZ share the same created_at (at(2)); tieZ's event_id sorts
		// after tieA's, so DESC ordering must put tieZ first.
		tieA := seed("tieA", at(2))
		tieZ := seed("tieZ", at(2))
		e5 := seed("e5", at(3))
		e6 := seed("e6", at(4))

		// after is inclusive (e2 at exactly at(1) is in), before is exclusive
		// (e6 at exactly at(4) is out); e1 at at(0) is out because after excludes
		// everything strictly earlier than at(1). Also pins created_at DESC,
		// event_id DESC ordering via the tieA/tieZ tie at at(2).
		feed, err := f.repo.MembershipFeed(ctx, ref, f.org, owner, MembershipFeedParams{
			After: at(1), Before: at(4), Limit: MembershipFeedDefaultLimit,
		})
		if err != nil {
			t.Fatalf("feed: %v", err)
		}
		if feed.Truncated {
			t.Fatalf("unexpected truncation: %+v", feed)
		}
		wantIDs := []string{e5, tieZ, tieA, e2}
		if len(feed.Events) != len(wantIDs) {
			t.Fatalf("got %d events, want %d: %+v", len(feed.Events), len(wantIDs), feed.Events)
		}
		for i, id := range wantIDs {
			if feed.Events[i].EventID != id {
				t.Fatalf("event %d = %s, want %s (after inclusive, before exclusive, created_at desc/event_id desc tiebreak): %+v",
					i, feed.Events[i].EventID, id, feed.Events)
			}
		}

		// Unbounded with limit 3 over 6 total rows: truncated, and exactly the
		// limit's worth of rows, newest first (not limit+1 rows).
		limited, err := f.repo.MembershipFeed(ctx, ref, f.org, owner, MembershipFeedParams{Limit: 3})
		if err != nil {
			t.Fatalf("limited feed: %v", err)
		}
		if !limited.Truncated {
			t.Fatalf("want truncated with 6 rows and limit 3: %+v", limited)
		}
		wantTop3 := []string{e6, e5, tieZ}
		if len(limited.Events) != len(wantTop3) {
			t.Fatalf("got %d events, want %d (not limit+1): %+v", len(limited.Events), len(wantTop3), limited.Events)
		}
		for i, id := range wantTop3 {
			if limited.Events[i].EventID != id {
				t.Fatalf("limited event %d = %s, want %s: %+v", i, limited.Events[i].EventID, id, limited.Events)
			}
		}

		// Unbounded with a limit covering every row: not truncated.
		all, err := f.repo.MembershipFeed(ctx, ref, f.org, owner, MembershipFeedParams{Limit: 10})
		if err != nil {
			t.Fatalf("unbounded feed: %v", err)
		}
		if all.Truncated || len(all.Events) != 6 {
			t.Fatalf("want all 6 rows untruncated, got %+v", all)
		}
	})
}

// TestMembershipFeedRefusesNonMember proves the membership-required refusal
// for a caller with no membership row, and shows that a caller from a
// different organization is refused the SAME way (ErrNoCurrentMembership),
// not with a silent empty result: resolveUserMembership's join requires
// s.org_id=$2 (and an active user_org_memberships row in that org) before any
// space_memberships row is even considered, so a cross-org caller never
// reaches a branch inside MembershipFeed that could return an empty feed --
// it is refused at membership resolution, identically to a plain non-member.
func TestMembershipFeedRefusesNonMember(t *testing.T) {
	f := newRoomLiveFixture(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "feednonmem")

	outsider := f.user(t, "feednonmem-outsider", f.org)
	if _, err := f.repo.MembershipFeed(ctx, s.ref, f.org, outsider, MembershipFeedParams{Limit: MembershipFeedDefaultLimit}); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("outsider: err = %v, want ErrNoCurrentMembership", err)
	}

	otherOrgUser := f.user(t, "feednonmem-otherorg", f.other)
	if _, err := f.repo.MembershipFeed(ctx, s.ref, f.other, otherOrgUser, MembershipFeedParams{Limit: MembershipFeedDefaultLimit}); !errors.Is(err, ErrNoCurrentMembership) {
		t.Fatalf("other org: err = %v, want ErrNoCurrentMembership", err)
	}
}

// TestMembershipFeedAppliesHistoryFloor documents a KNOWN GAP (see
// .plan-work/space-build/depth/WS15-S11.md, "P1 gap: WS04-S11 history floor
// ... is not applied to the membership feed"). WS04-S11 has since landed
// (history_window.go, history_window_live_test.go), but membership_feed.go
// was never updated to use it: it does not read history_mode or
// HistoryFloorRevision at all, and there is no existing mapping in this
// package from a recipient-audience floor REVISION to the created_at
// INTERVAL this feed filters on (applyHistoryFloor only ever fills a
// PersonalThreadDecisionEvidence), so wiring the floor into this feed is not
// the minimal fix this slice allows.
//
// D18 closes that gap: MembershipFeed now reads the same floor the transcript does
// (resolveHistoryFloor), so a late joiner no longer sees membership rows from before
// their join. This test asserts the floor is ENFORCED: the row after the join is
// visible, and the row before it is absent.
func TestMembershipFeedAppliesHistoryFloor(t *testing.T) {
	f := newRoomLiveFixture(t)
	f.historyPolicy(t)
	ctx := context.Background()
	s := f.staffedRoom(t, "feedfloor")
	f.repo.SetHistoryWindowClaim(true)
	f.exec(t, `UPDATE registered_spaces SET history_mode='since_join' WHERE space_ref=$1`, s.ref)

	// rev 1: the editor is not yet in the audience. rev 2: the editor joins,
	// so their history floor is revision 2 (same derivation as
	// TestHistoryFloorFollowsTheAudienceSnapshotsLive in history_window_live_test.go).
	f.publishAudience(t, s.ref, 1, s.owner, s.manager, s.viewer)
	f.publishAudience(t, s.ref, 2, s.owner, s.manager, s.viewer, s.editor)
	if floor := f.floorFor(t, s.ref, s.editor); floor != 2 {
		t.Fatalf("setup: editor floor = %d, want 2", floor)
	}

	beforeJoin := "sme_ws15s11_floor_before_" + f.suffix
	afterJoin := "sme_ws15s11_floor_after_" + f.suffix
	f.exec(t, `INSERT INTO space_membership_events
		(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, created_at)
		VALUES ($1,$2,$3,1,'member_added','user','ws15s11-actor',NOW() - INTERVAL '1 hour')`,
		beforeJoin, s.ref, f.org)
	f.exec(t, `INSERT INTO space_membership_events
		(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, created_at)
		VALUES ($1,$2,$3,2,'member_added','user','ws15s11-actor',NOW())`,
		afterJoin, s.ref, f.org)

	feed, err := f.repo.MembershipFeed(ctx, s.ref, f.org, s.editor, MembershipFeedParams{Limit: MembershipFeedDefaultLimit})
	if err != nil {
		t.Fatalf("feed: %v", err)
	}
	seen := map[string]bool{}
	for _, e := range feed.Events {
		seen[e.EventID] = true
	}
	if !seen[afterJoin] {
		t.Fatalf("the event after the editor joined must be visible: %+v", feed.Events)
	}
	if seen[beforeJoin] {
		t.Fatalf("a floored member must not see a membership row from before their join: %+v", feed.Events)
	}
}
