package spaces

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"
)

// WS04-S10 live proof of the durable membership publisher. Gated on
// TEST_DATABASE_URL through newRoomLiveFixture (skipped without it). Other
// pending rows in a shared database are ignored: every assertion is about the
// rows this test inserted.

type outboxLive struct {
	*roomLiveFixture
	ref, owner string
	ids        map[string]bool
}

func newOutboxLive(t *testing.T, name string) *outboxLive {
	t.Helper()
	f := newRoomLiveFixture(t)
	owner := f.user(t, name+"-owner", f.org)
	return &outboxLive{roomLiveFixture: f, ref: f.room(t, name, owner), owner: owner, ids: map[string]bool{}}
}

// ledger inserts one pending ledger row and returns its event id.
func (o *outboxLive) ledger(t *testing.T, id, action, subjectType, subject, toRole string) string {
	t.Helper()
	eventID := "sme_ws04s10_" + id + "_" + o.suffix
	o.exec(t, `INSERT INTO space_membership_events
		(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, subject_type, subject_id, to_role)
		VALUES ($1,$2,$3,5,$4,'user','actor-1',$5,$6,NULLIF($7,''))`,
		eventID, o.ref, o.org, action, subjectType, subject, toRole)
	o.ids[eventID] = true
	return eventID
}

func (o *outboxLive) row(t *testing.T, eventID string) (published bool, attempts int, nextAt *time.Time, lastError string) {
	t.Helper()
	var publishedAt *time.Time
	var errText *string
	if err := o.pool.QueryRow(context.Background(), `
		SELECT published_at, publish_attempts, next_publish_at, last_error
		FROM space_membership_events WHERE event_id=$1`, eventID).Scan(&publishedAt, &attempts, &nextAt, &errText); err != nil {
		t.Fatalf("read ledger row: %v", err)
	}
	if errText != nil {
		lastError = *errText
	}
	return publishedAt != nil, attempts, nextAt, lastError
}

// mine filters a fake publisher's calls down to this test's events.
func (o *outboxLive) mine(calls []recordedPublish) []recordedPublish {
	var out []recordedPublish
	for _, call := range calls {
		if o.ids[call.msgID] {
			out = append(out, call)
		}
	}
	return out
}

var bigBatch = MembershipEventsOutboxConfig{BatchSize: 1000, Lease: time.Minute}

func TestMembershipOutboxClaimsOnlyPendingUserRowsOldestFirstLive(t *testing.T) {
	o := newOutboxLive(t, "ob-claim")
	ctx := context.Background()
	first := o.ledger(t, "a", ActionMemberAdded, "user", "u-a", "editor")
	second := o.ledger(t, "b", ActionMemberJoined, "user", "u-b", "viewer")
	third := o.ledger(t, "c", ActionMemberRemoved, "user", "u-c", "")
	fourth := o.ledger(t, "d", ActionMemberLeft, "user", "u-d", "")
	// Never claimed: a role change, an archive, and a service subject.
	roleChanged := o.ledger(t, "e", "role_changed", "user", "u-e", "manager")
	archived := o.ledger(t, "f", "archived", "", "", "")
	service := o.ledger(t, "g", ActionMemberAdded, "service", "agent-1", "editor")
	// Already published.
	done := o.ledger(t, "h", ActionMemberAdded, "user", "u-h", "editor")
	o.exec(t, `UPDATE space_membership_events SET published_at=NOW() WHERE event_id=$1`, done)

	publisher := &fakeMembershipPublisher{}
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, publisher, bigBatch); err != nil {
		t.Fatalf("publish: %v", err)
	}
	got := o.mine(publisher.published())
	want := []string{first, second, third, fourth}
	if len(got) != len(want) {
		t.Fatalf("published %d of my rows, want %d: %v", len(got), len(want), got)
	}
	for i, id := range want {
		if got[i].msgID != id || got[i].subject != MembershipChangedSubject {
			t.Fatalf("publish %d = %s on %s, want %s on %s (oldest first)", i, got[i].msgID, got[i].subject, id, MembershipChangedSubject)
		}
	}
	for _, id := range []string{roleChanged, archived, service, done} {
		for _, call := range got {
			if call.msgID == id {
				t.Errorf("row %s must never be claimed", id)
			}
		}
	}
	for _, id := range want {
		if published, attempts, next, lastError := o.row(t, id); !published || attempts != 1 || next != nil || lastError != "" {
			t.Errorf("%s after a PubAck: published=%v attempts=%d next=%v err=%q", id, published, attempts, next, lastError)
		}
	}
	for _, id := range []string{roleChanged, archived, service} {
		if published, attempts, _, _ := o.row(t, id); published || attempts != 0 {
			t.Errorf("%s was touched: published=%v attempts=%d", id, published, attempts)
		}
	}
}

func TestMembershipOutboxFailureBacksOffAndKeepsTheRowPendingLive(t *testing.T) {
	o := newOutboxLive(t, "ob-fail")
	ctx := context.Background()
	failing := o.ledger(t, "a", ActionMemberAdded, "user", "u-a", "editor")
	fine := o.ledger(t, "b", ActionMemberAdded, "user", "u-b", "editor")
	publisher := &fakeMembershipPublisher{failIDs: map[string]error{failing: errors.New("broker unavailable")}}

	if _, err := o.repo.PublishPendingMembershipEvents(ctx, publisher, bigBatch); err != nil {
		t.Fatalf("a failed publish is not an error of the batch: %v", err)
	}
	published, attempts, next, lastError := o.row(t, failing)
	if published || attempts != 1 || lastError != "broker unavailable" {
		t.Fatalf("failed row: published=%v attempts=%d err=%q", published, attempts, lastError)
	}
	if next == nil || !next.After(time.Now().Add(-time.Second)) {
		t.Fatalf("next_publish_at = %v, want a time in the future (backoff)", next)
	}
	if ok, _, _, _ := o.row(t, fine); !ok {
		t.Fatal("the healthy row was held back by its failing neighbour")
	}
	// Inside the backoff the row is not claimed again.
	before := len(o.mine(publisher.published()))
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, publisher, bigBatch); err != nil {
		t.Fatal(err)
	}
	if got := len(o.mine(publisher.published())); got != before {
		t.Fatalf("a backed-off row was claimed again (%d -> %d publishes)", before, got)
	}
	// Once the backoff has passed it is retried and, with the broker back, published.
	o.exec(t, `UPDATE space_membership_events SET next_publish_at = NOW() - INTERVAL '1 second' WHERE event_id=$1`, failing)
	publisher.mu.Lock()
	publisher.failIDs = nil
	publisher.mu.Unlock()
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, publisher, bigBatch); err != nil {
		t.Fatal(err)
	}
	if published, attempts, next, lastError := o.row(t, failing); !published || attempts != 2 || next != nil || lastError != "" {
		t.Fatalf("retried row: published=%v attempts=%d next=%v err=%q", published, attempts, next, lastError)
	}
}

func TestMembershipOutboxTwoClaimersNeverPublishTheSameRowLive(t *testing.T) {
	o := newOutboxLive(t, "ob-race")
	ctx := context.Background()
	const rows = 40
	for i := range rows {
		o.ledger(t, fmt.Sprintf("r%02d", i), ActionMemberAdded, "user", fmt.Sprintf("u-%02d", i), "editor")
	}
	publisher := &fakeMembershipPublisher{}
	cfg := MembershipEventsOutboxConfig{BatchSize: 7, Lease: time.Minute}
	var wg sync.WaitGroup
	for range 2 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for range 10 {
				if n, err := o.repo.PublishPendingMembershipEvents(ctx, publisher, cfg); err != nil {
					t.Errorf("claimer: %v", err)
					return
				} else if n == 0 {
					return
				}
			}
		}()
	}
	wg.Wait()
	seen := map[string]int{}
	for _, call := range o.mine(publisher.published()) {
		seen[call.msgID]++
	}
	for id, n := range seen {
		if n != 1 {
			t.Errorf("event %s was published %d times", id, n)
		}
	}
	if len(seen) != rows {
		t.Fatalf("published %d distinct rows, want %d", len(seen), rows)
	}
}

func TestMembershipOutboxLeaseReturnsACrashedClaimLive(t *testing.T) {
	o := newOutboxLive(t, "ob-lease")
	ctx := context.Background()
	id := o.ledger(t, "a", ActionMemberAdded, "user", "u-a", "editor")
	// A worker claims the row and dies before publishing: the publish fails with
	// a cancelled context, so the lease (not the row state) is all that remains.
	crashed := &fakeMembershipPublisher{failAll: errors.New("process died")}
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, crashed, MembershipEventsOutboxConfig{BatchSize: 1000, Lease: time.Hour}); err != nil {
		t.Fatal(err)
	}
	// Backoff is a second after one attempt, but the lease was an hour; the failure path
	// resets next_publish_at to the short backoff, so force the lease state.
	o.exec(t, `UPDATE space_membership_events SET next_publish_at = NOW() + INTERVAL '1 hour' WHERE event_id=$1`, id)
	live := &fakeMembershipPublisher{}
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, live, bigBatch); err != nil {
		t.Fatal(err)
	}
	if len(o.mine(live.published())) != 0 {
		t.Fatal("a row inside its lease was claimed by a second worker")
	}
	o.exec(t, `UPDATE space_membership_events SET next_publish_at = NOW() - INTERVAL '1 second' WHERE event_id=$1`, id)
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, live, bigBatch); err != nil {
		t.Fatal(err)
	}
	if got := o.mine(live.published()); len(got) != 1 || got[0].msgID != id {
		t.Fatalf("after the lease the row must come back: %v", got)
	}
}

func TestMembershipOutboxPayloadStatesTheCurrentMembershipRowLive(t *testing.T) {
	o := newOutboxLive(t, "ob-fact")
	ctx := context.Background()
	member := o.user(t, "ob-fact-member", o.org)
	o.member(t, o.ref, member, "manager", SourceMemberOps)
	// The ledger says "added as editor", but the live row is a manager.
	added := o.ledger(t, "a", ActionMemberAdded, "user", member, "editor")
	// The ledger says "removed", but the member is active again.
	removed := o.ledger(t, "b", ActionMemberRemoved, "user", member, "")
	// A row whose membership is gone altogether (deactivated).
	gone := o.user(t, "ob-fact-gone", o.org)
	o.member(t, o.ref, gone, "viewer", SourceMemberOps)
	o.exec(t, `UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, o.ref, gone)
	goneAdded := o.ledger(t, "c", ActionMemberAdded, "user", gone, "viewer")

	publisher := &fakeMembershipPublisher{}
	if _, err := o.repo.PublishPendingMembershipEvents(ctx, publisher, bigBatch); err != nil {
		t.Fatal(err)
	}
	byID := map[string]map[string]any{}
	for _, call := range o.mine(publisher.published()) {
		byID[call.msgID] = call.payload
	}
	if p := byID[added]; p["active"] != true || p["role"] != "manager" || p["action"] != ActionMemberAdded || p["space_seq"] != int64(5) {
		t.Errorf("added payload = %v, want the current manager row", p)
	}
	if p := byID[removed]; p["active"] != true || p["role"] != "manager" {
		t.Errorf("removed-then-readded payload = %v, want active=true", p)
	}
	if p := byID[goneAdded]; p["active"] != false {
		t.Errorf("deactivated member payload = %v, want active=false", p)
	}
	if p := byID[added]; p["space_ref"] != o.ref || p["org_id"] != o.org || p["subject_id"] != member || p["actor_kind"] != ActorKindUser {
		t.Errorf("identity fields = %v", p)
	}
}

func TestReannounceMembershipFactsWindowAndStableMsgIDLive(t *testing.T) {
	o := newOutboxLive(t, "ob-rean")
	ctx := context.Background()
	recent := o.user(t, "ob-rean-recent", o.org)
	old := o.user(t, "ob-rean-old", o.org)
	inactive := o.user(t, "ob-rean-inactive", o.org)
	o.member(t, o.ref, recent, "editor", SourceMemberOps)
	o.member(t, o.ref, old, "viewer", SourceMemberOps)
	o.member(t, o.ref, inactive, "viewer", SourceMemberOps)
	o.exec(t, `UPDATE space_memberships SET updated_at = NOW() - INTERVAL '3 days' WHERE space_ref=$1 AND subject_id=$2`, o.ref, old)
	o.exec(t, `UPDATE space_memberships SET active=FALSE, updated_at=NOW() WHERE space_ref=$1 AND subject_id=$2`, o.ref, inactive)
	o.exec(t, `UPDATE space_memberships SET updated_at = NOW() - INTERVAL '3 days' WHERE space_ref=$1 AND subject_id=$2`, o.ref, o.owner)

	cfg := MembershipEventsOutboxConfig{ReannounceWindow: 24 * time.Hour, ReannounceEvery: time.Hour}
	sweep := func() map[string]recordedPublish {
		publisher := &fakeMembershipPublisher{}
		if err := o.repo.ReannounceMembershipFacts(ctx, publisher, cfg); err != nil {
			t.Fatalf("sweep: %v", err)
		}
		out := map[string]recordedPublish{}
		for _, call := range publisher.published() {
			if call.payload["space_ref"] == o.ref {
				out[fmt.Sprint(call.payload["subject_id"])] = call
			}
		}
		return out
	}
	first := sweep()
	if _, ok := first[old]; ok {
		t.Error("a membership outside the window was re-announced")
	}
	if _, ok := first[o.owner]; ok {
		t.Error("the long-unchanged owner row was re-announced")
	}
	r, i := first[recent], first[inactive]
	if r.payload == nil || r.payload["active"] != true || r.payload["role"] != "editor" || r.payload["action"] != ActionReannounce ||
		r.payload["actor_kind"] != ActorKindService || r.subject != MembershipChangedSubject {
		t.Fatalf("recent member re-announcement = %+v", r)
	}
	if i.payload == nil || i.payload["active"] != false || i.payload["role"] != "" {
		t.Fatalf("inactive member re-announcement = %+v, want active=false with no role", i)
	}
	// Within one bucket the message id is stable (replicas deduplicate at the broker).
	second := sweep()
	if first[recent].msgID == "" || first[recent].msgID != second[recent].msgID {
		t.Fatalf("message id changed within a bucket: %q vs %q", first[recent].msgID, second[recent].msgID)
	}
	if r.payload["event_id"] != r.msgID {
		t.Fatalf("event_id %v must equal the message id %s", r.payload["event_id"], r.msgID)
	}

	// A failing broker stops the sweep with an error rather than hammering it.
	failing := &fakeMembershipPublisher{failAll: errors.New("broker unavailable")}
	if err := o.repo.ReannounceMembershipFacts(ctx, failing, cfg); err == nil {
		t.Fatal("a sweep against a failing broker must report the failure")
	}
}
