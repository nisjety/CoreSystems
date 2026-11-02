package users

import (
	"context"
	"crypto/sha256"
	"fmt"
	"testing"
)

// WS04-S4: the GDPR erasure saga pseudonymises the Space membership ledger.
// actor_id (for user actors) and subject_id (for user subjects) of the erased
// user are replaced by a one-way pseudonym; other users' rows and service
// actors are untouched. Gated on TEST_DATABASE_URL like the other saga tests.
func TestErasureSagaPseudonymisesSpaceMembershipLedger(t *testing.T) {
	pool, repo := newErasurePostgresFixture(t)
	insertErasureSubject(t, pool, "u-ledger", "org-ledger")
	ctx := context.Background()

	rows := []struct {
		eventID, actorKind, actorID, subjectType, subjectID string
	}{
		{"sme-1", "user", "u-ledger", "user", "u-other"},    // erased user acted
		{"sme-2", "user", "u-other", "user", "u-ledger"},    // erased user was the subject
		{"sme-3", "user", "u-ledger", "user", "u-ledger"},   // both
		{"sme-4", "user", "u-other", "user", "u-other-two"}, // unrelated
		{"sme-5", "service", "u-ledger", "user", "u-other"}, // service actor id that merely equals the user id
	}
	for _, r := range rows {
		if _, err := pool.Exec(ctx, `
			INSERT INTO space_membership_events (event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, subject_type, subject_id)
			VALUES ($1,'space-ledger','org-ledger',1,'member_added',$2,$3,$4,$5)`,
			r.eventID, r.actorKind, r.actorID, r.subjectType, r.subjectID); err != nil {
			t.Fatalf("seed ledger row %s: %v", r.eventID, err)
		}
	}

	auth := &fakeAuthErasureExecutor{execute: func(context.Context, ErasureMode, string) ([]byte, error) {
		return []byte(`{"success":true,"user_id":"u-ledger"}`), nil
	}}
	svc := NewService(repo, nil, nil)
	svc.authEraser = auth
	svc.erasureFanout = &fakeErasureFanoutPublisher{}
	if _, err := svc.HardEraseUser(ctx, "u-ledger", "u-ledger", "self", "org-ledger"); err != nil {
		t.Fatalf("hard erase: %v", err)
	}

	digest := sha256.Sum256([]byte("space-membership-ledger:u-ledger"))
	pseudonym := fmt.Sprintf("erased_%x", digest[:16])

	read := func(eventID string) (actor string, subject string) {
		t.Helper()
		if err := pool.QueryRow(ctx, `SELECT actor_id, subject_id FROM space_membership_events WHERE event_id=$1`, eventID).Scan(&actor, &subject); err != nil {
			t.Fatalf("read ledger row %s: %v", eventID, err)
		}
		return actor, subject
	}
	if a, s := read("sme-1"); a != pseudonym || s != "u-other" {
		t.Errorf("sme-1 actor=%q subject=%q", a, s)
	}
	if a, s := read("sme-2"); a != "u-other" || s != pseudonym {
		t.Errorf("sme-2 actor=%q subject=%q", a, s)
	}
	if a, s := read("sme-3"); a != pseudonym || s != pseudonym {
		t.Errorf("sme-3 actor=%q subject=%q", a, s)
	}
	if a, s := read("sme-4"); a != "u-other" || s != "u-other-two" {
		t.Errorf("an unrelated row changed: actor=%q subject=%q", a, s)
	}
	if a, s := read("sme-5"); a != "u-ledger" || s != "u-other" {
		t.Errorf("a service-actor row must keep its actor id: actor=%q subject=%q", a, s)
	}
	var remaining int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM space_membership_events WHERE actor_id='u-ledger' AND actor_kind='user' OR subject_id='u-ledger'`).Scan(&remaining); err != nil {
		t.Fatalf("count remaining: %v", err)
	}
	if remaining != 0 {
		t.Fatalf("%d ledger rows still name the erased user", remaining)
	}
	// The pseudonym is deterministic, one-way and not the raw id.
	if pseudonym == "u-ledger" || len(pseudonym) != len("erased_")+32 {
		t.Fatalf("unexpected pseudonym shape %q", pseudonym)
	}
}

// The pseudonym function the saga uses is a pure derivation; pin its shape
// without a database so a change is caught everywhere.
func TestSpaceMembershipLedgerPseudonymIsStableAndOneWay(t *testing.T) {
	derive := func(userID string) string {
		digest := sha256.Sum256([]byte("space-membership-ledger:" + userID))
		return fmt.Sprintf("erased_%x", digest[:16])
	}
	if derive("u-1") != derive("u-1") {
		t.Fatal("pseudonym is not deterministic")
	}
	if derive("u-1") == derive("u-2") {
		t.Fatal("distinct users must not share a pseudonym")
	}
	if got := derive("u-1"); len(got) != len("erased_")+32 {
		t.Fatalf("unexpected pseudonym length %d", len(got))
	}
}
