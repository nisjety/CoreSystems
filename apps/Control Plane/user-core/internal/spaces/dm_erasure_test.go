package spaces

import (
	"context"
	"testing"
)

// D22o (F-t1-016): EraseDMParticipantTx refuses before any statement when it
// is not given an erased id and a distinct pseudonym, so the ledger can never
// be written with the raw id in place of the pseudonym. A nil tx proves no
// statement is reached.
func TestEraseDMParticipantTxRefusesMissingOrRawPseudonym(t *testing.T) {
	for _, tc := range []struct{ name, user, pseudonym string }{
		{"no user", "", "erased_ab"},
		{"no pseudonym", "u-1", ""},
		{"blank pseudonym", "u-1", "   "},
		{"pseudonym equals the raw id", "u-1", "u-1"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			result, err := EraseDMParticipantTx(context.Background(), nil, tc.user, tc.pseudonym)
			if err == nil {
				t.Fatalf("err = nil, want a refusal")
			}
			if result.Spaces != 0 {
				t.Fatalf("spaces = %d, want 0", result.Spaces)
			}
		})
	}
}

// D22o live proof: an erased DM creator leaves the roster of their DM and
// group DM; the DMs stay registered and active, the peers keep their active
// memberships (so their messages stay readable), the revision advances, the
// ledger row carries the pseudonym and is already published, and a non-DM room
// the same person is in is untouched. A re-run changes nothing.
// Gated on TEST_DATABASE_URL (skipped without it).
func TestEraseDMParticipantKeepsTheDMAndThePeers(t *testing.T) {
	d := newDMLive(t, true)
	ctx := context.Background()
	creator, peer, third := d.user(t, "dmerase-a", d.org), d.user(t, "dmerase-b", d.org), d.user(t, "dmerase-c", d.org)
	dm, err := d.register(t, "dmerase1", KindDM, creator, creator, peer)
	if err != nil {
		t.Fatalf("register dm: %v", err)
	}
	group, err := d.register(t, "dmerase2", KindGroupDM, creator, creator, peer, third)
	if err != nil {
		t.Fatalf("register group dm: %v", err)
	}
	room := d.room(t, "dmerase-room", creator)
	t.Cleanup(func() {
		_, _ = d.pool.Exec(ctx, `DELETE FROM space_member_history_windows WHERE space_ref LIKE 'ws04s4-%-'||$1`, d.suffix)
	})
	d.exec(t, `INSERT INTO space_member_history_windows (space_ref, subject_id, mode, set_by) VALUES ($1,$2,'full','test')`, dm.SpaceRef, creator)
	before := d.authority(t, dm.SpaceRef)
	ledgerBefore := d.ledgerCount(t, dm.SpaceRef)
	const pseudonym = "erased_dmerase_pseudonym"

	tx, err := d.pool.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	result, err := EraseDMParticipantTx(ctx, tx, creator, pseudonym)
	if err != nil {
		_ = tx.Rollback(ctx)
		t.Fatalf("erase: %v", err)
	}
	if err := tx.Commit(ctx); err != nil {
		t.Fatal(err)
	}
	if result.Spaces != 2 {
		t.Fatalf("spaces = %d, want the DM and the group DM", result.Spaces)
	}

	for _, ref := range []string{dm.SpaceRef, group.SpaceRef} {
		if n := d.count(t, `SELECT count(*) FROM space_memberships WHERE space_ref=$1 AND subject_id=$2`, ref, creator); n != 0 {
			t.Fatalf("%s: erased creator still has %d roster rows", ref, n)
		}
		if role, active, _ := d.role(t, ref, peer); role != "editor" || !active {
			t.Fatalf("%s: peer = %s active=%v, want an active editor", ref, role, active)
		}
		if n := d.count(t, `SELECT count(*) FROM registered_spaces WHERE space_ref=$1 AND registration_state='active' AND owner_principal_id=$2 AND participant_hash IS NOT NULL`, ref, creator); n != 1 {
			t.Fatalf("%s: the DM registration changed (rows matching = %d)", ref, n)
		}
		if n := d.count(t, `SELECT count(*) FROM space_membership_events WHERE space_ref=$1 AND action='member_removed' AND actor_kind='service' AND subject_id=$2 AND published_at IS NOT NULL`, ref, pseudonym); n != 1 {
			t.Fatalf("%s: pseudonymous published erasure ledger rows = %d, want 1", ref, n)
		}
		if n := d.count(t, `SELECT count(*) FROM space_membership_events WHERE space_ref=$1 AND (subject_id=$2 OR actor_id=$2)`, ref, creator); n != 0 {
			t.Fatalf("%s: the ledger still names the erased id (%d rows)", ref, n)
		}
	}
	if d.authority(t, dm.SpaceRef) != before+1 {
		t.Fatalf("authority revision did not advance by one")
	}
	if d.ledgerCount(t, dm.SpaceRef) != ledgerBefore+1 {
		t.Fatalf("ledger rows did not grow by one")
	}
	if n := d.count(t, `SELECT count(*) FROM space_member_history_windows WHERE space_ref=$1 AND subject_id=$2`, dm.SpaceRef, creator); n != 0 {
		t.Fatalf("erased creator's history window row remains")
	}
	if role, active, _ := d.role(t, room, creator); role != "owner" || !active {
		t.Fatalf("non-DM room membership changed: %s active=%v", role, active)
	}

	// A re-run finds nothing.
	tx, err = d.pool.Begin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	again, err := EraseDMParticipantTx(ctx, tx, creator, pseudonym)
	_ = tx.Commit(ctx)
	if err != nil || again.Spaces != 0 {
		t.Fatalf("re-run = %+v err=%v, want nothing changed", again, err)
	}
	if d.ledgerCount(t, dm.SpaceRef) != ledgerBefore+1 {
		t.Fatalf("re-run appended a ledger row")
	}
}
