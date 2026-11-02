package storetest

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

// RunConnectionZDRLifecycleContract covers what Z-13 adds to the ledger: revocation when a
// connection is removed (audited once per row, in the caller's transaction), the organisation
// purge, and the organisation's admin listing. Like RunConnectionZDRContract it runs against the
// in-memory repository and against PostgreSQL, so both must agree.
func RunConnectionZDRLifecycleContract(t *testing.T, h ZDRHarness) {
	t.Run("removal revokes every active acknowledgement and audits each row once", func(t *testing.T) {
		f := newFixture(t, h)
		f.apply(1)
		ack := f.acknowledge(digestOf(1))
		at := time.Now().UTC()

		var events []store.AuditEvent
		err := f.repo.WithAuditTransaction(context.Background(), func(tx store.AuditTransaction) error {
			var inner error
			events, inner = store.RevokeAcknowledgementsOnRemoval(context.Background(), tx, f.conn, at)
			return inner
		})
		if err != nil {
			t.Fatalf("removal inside the audit transaction: %v", err)
		}
		if len(events) != 1 {
			t.Fatalf("removal audited %d events, want one per revoked acknowledgement (1)", len(events))
		}
		event := events[0]
		if event.EventType != "subscription_connection.zdr_acknowledgement_revoked" || event.ConnectionID != f.conn ||
			event.OrganizationID != f.org || event.UserID != f.user || event.Metadata["reason"] != store.ZDRRevokeConnectionRemoved {
			t.Fatalf("removal audit event is wrong: %+v", event)
		}
		if !h.AuditEventExists(t, f.repo, event.ID) {
			t.Fatalf("the removal audit row %s was not committed with the revocation", event.ID)
		}
		rows := f.ledger(false)
		if len(rows) != 1 || rows[0].ID != ack.ID || rows[0].RevokedAt == nil || rows[0].RevokeReason != store.ZDRRevokeConnectionRemoved {
			t.Fatalf("ledger after removal = %+v, want the acknowledgement revoked for connection_removed", rows)
		}

		// Replaying the removal finds nothing active: no second revocation and no second audit row.
		err = f.repo.WithAuditTransaction(context.Background(), func(tx store.AuditTransaction) error {
			var inner error
			events, inner = store.RevokeAcknowledgementsOnRemoval(context.Background(), tx, f.conn, at)
			return inner
		})
		if err != nil || len(events) != 0 {
			t.Fatalf("replayed removal: events=%d err=%v, want none", len(events), err)
		}
	})

	t.Run("removal of a connection without acknowledgements writes nothing", func(t *testing.T) {
		f := newFixture(t, h)
		f.apply(1)
		var events []store.AuditEvent
		err := f.repo.WithAuditTransaction(context.Background(), func(tx store.AuditTransaction) error {
			var inner error
			events, inner = store.RevokeAcknowledgementsOnRemoval(context.Background(), tx, f.conn, time.Time{})
			return inner
		})
		if err != nil || len(events) != 0 {
			t.Fatalf("removal with nothing to revoke: events=%d err=%v", len(events), err)
		}
	})

	t.Run("a purge removes the organisation's records and acknowledgements and no other organisation's", func(t *testing.T) {
		mine := newFixture(t, h)
		mine.apply(1)
		mine.acknowledge(digestOf(1))
		theirs := newFixture(t, h)
		theirs.apply(1)
		theirsAck := theirs.acknowledge(digestOf(1))

		result, err := mine.repo.PurgeOrganizationConnectionZDR(context.Background(), mine.org)
		if err != nil {
			t.Fatalf("purge: %v", err)
		}
		if result.Acknowledgements != 1 || result.Records != 1 {
			t.Fatalf("purge removed %+v, want 1 acknowledgement and 1 record", result)
		}
		if _, err := mine.repo.GetConnectionZDR(context.Background(), mine.conn); !errors.Is(err, store.ErrNotFound) {
			t.Fatalf("purged record still readable: %v", err)
		}
		if rows := mine.ledger(false); len(rows) != 0 {
			t.Fatalf("purged acknowledgements still listed: %+v", rows)
		}
		if _, err := theirs.repo.GetConnectionZDR(context.Background(), theirs.conn); err != nil {
			t.Fatalf("another organisation's record was touched by the purge: %v", err)
		}
		rows := theirs.ledger(false)
		if len(rows) != 1 || rows[0].ID != theirsAck.ID {
			t.Fatalf("another organisation's acknowledgement was touched by the purge: %+v", rows)
		}
		again, err := mine.repo.PurgeOrganizationConnectionZDR(context.Background(), mine.org)
		if err != nil || again.Acknowledgements != 0 || again.Records != 0 {
			t.Fatalf("a second purge must be a no-op, got %+v err=%v", again, err)
		}
	})

	t.Run("the admin listing counts active and revoked rows and is scoped to the organisation", func(t *testing.T) {
		f := newFixture(t, h)
		base := time.Now().UTC().Add(-time.Hour)
		ackAt := func(digest string, at time.Time) {
			in := f.ackInput(digest)
			in.AcknowledgedAt = at
			if _, created, err := f.repo.InsertConnectionZDRAcknowledgement(context.Background(), in); err != nil || !created {
				t.Fatalf("acknowledge at %s: created=%v err=%v", at, created, err)
			}
		}
		f.apply(1)
		ackAt(digestOf(1), base)
		f.apply(2) // a catalog change revokes the first acknowledgement (capability_changed)
		ackAt(digestOf(2), base.Add(time.Minute))
		other := newFixture(t, h)
		other.apply(1)
		other.acknowledge(digestOf(1))

		list, err := f.repo.ListOrganizationConnectionZDRAcknowledgements(context.Background(), f.org, 10)
		if err != nil {
			t.Fatalf("list: %v", err)
		}
		if list.Active != 1 || list.Revoked != 1 || len(list.Items) != 2 {
			t.Fatalf("listing counts active=%d revoked=%d items=%d, want 1, 1, 2", list.Active, list.Revoked, len(list.Items))
		}
		for _, item := range list.Items {
			if item.OrganizationID != f.org {
				t.Fatalf("listing leaked a row from another organisation: %+v", item)
			}
		}
		if list.Items[0].Digest != digestOf(2) {
			t.Fatalf("listing is not newest first: %+v", list.Items)
		}
		limited, err := f.repo.ListOrganizationConnectionZDRAcknowledgements(context.Background(), f.org, 1)
		if err != nil || len(limited.Items) != 1 || limited.Active != 1 || limited.Revoked != 1 {
			t.Fatalf("a limit must cap the rows, not the counts: %+v err=%v", limited, err)
		}
	})
}
