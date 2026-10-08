// Package storetest holds contract suites that every implementation of a store interface must
// pass. The in-memory repository and the PostgreSQL repository run the same suite, so the
// memory double the API tests use cannot drift from the database behaviour it stands in for.
// It is imported only by tests.
package storetest

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// ZDRHarness adapts one implementation to the suite.
type ZDRHarness struct {
	// NewRepository returns a repository over an empty (memory) or disposable (PostgreSQL)
	// store. Every id the suite uses is unique, so a shared database needs no cleanup.
	NewRepository func(t *testing.T) store.Repository
	// AuditEventExists reports whether an audit row with this id has been committed.
	AuditEventExists func(t *testing.T, repo store.Repository, id string) bool
}

const (
	providerKey = "openai-codex-subscription"
	authMode    = "codex_chatgpt_device_code"
)

type fixture struct {
	t    *testing.T
	repo store.Repository
	org  string
	user string
	conn string
}

func newFixture(t *testing.T, h ZDRHarness) *fixture {
	t.Helper()
	f := &fixture{t: t, repo: h.NewRepository(t)}
	suffix := uuid.NewString()
	f.org, f.user = "org-"+suffix, "user-"+suffix
	f.conn = f.seedConnection(f.org, f.user)
	return f
}

func (f *fixture) seedConnection(org, user string) string {
	f.t.Helper()
	id := "conn-" + uuid.NewString()
	_, err := f.repo.UpsertConnection(context.Background(), store.Connection{
		ID: id, ProviderKey: providerKey, ConnectorType: providerKey,
		OrganizationID: org, WorkspaceID: "ws-" + org, UserID: user, Status: "active",
		Capabilities:         []string{"model.inference"},
		AccessTokenExpiresAt: time.Now().UTC().Add(time.Hour),
	})
	if err != nil {
		f.t.Fatalf("seed connection: %v", err)
	}
	return id
}

func assessmentFor(connection string, capability zdr.Capability, basis zdr.Basis, version int) store.ConnectionZDRAssessment {
	digest, err := zdr.Digest(providerKey, authMode, capability, basis, version)
	if err != nil {
		panic(err)
	}
	return store.ConnectionZDRAssessment{
		ConnectionID: connection, ProviderKey: providerKey, AuthMode: authMode,
		Capability: capability, Basis: basis, BasisRef: fmt.Sprintf("%s@%d", providerKey, version),
		CatalogVersion: version, Digest: digest,
	}
}

func (f *fixture) apply(version int) store.ConnectionZDRApplyResult {
	f.t.Helper()
	result, err := f.repo.ApplyConnectionZDRAssessment(context.Background(),
		assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, version))
	if err != nil {
		f.t.Fatalf("apply assessment v%d: %v", version, err)
	}
	return result
}

func (f *fixture) ackInput(digest string) store.NewConnectionZDRAcknowledgement {
	text := "Dette abonnementet kan ikke gi null datalagring. Jeg forstaar og fortsetter."
	sum := sha256.Sum256([]byte(text))
	return store.NewConnectionZDRAcknowledgement{
		OrganizationID: f.org, ConnectionID: f.conn, UserID: f.user, Digest: digest,
		TextVersion: "2026-10-a", Locale: "no", TextSHA256: hex.EncodeToString(sum[:]), TextSnapshot: text,
		OrgZDRAtAck: true, AcknowledgedVia: store.ZDRAcknowledgedViaConnect, RequestID: "req-" + uuid.NewString(),
	}
}

func (f *fixture) acknowledge(digest string) store.ConnectionZDRAcknowledgement {
	f.t.Helper()
	saved, created, err := f.repo.InsertConnectionZDRAcknowledgement(context.Background(), f.ackInput(digest))
	if err != nil || !created {
		f.t.Fatalf("acknowledge: created=%v err=%v", created, err)
	}
	return saved
}

func (f *fixture) ledger(activeOnly bool) []store.ConnectionZDRAcknowledgement {
	f.t.Helper()
	rows, err := f.repo.ListConnectionZDRAcknowledgements(context.Background(), f.conn, activeOnly)
	if err != nil {
		f.t.Fatalf("list acknowledgements: %v", err)
	}
	return rows
}

// sameRecord compares two records field by field, times by instant: the same instant read
// twice from a database need not be the same time.Time value.
func sameRecord(a, b store.ConnectionZDR) bool {
	return a.ConnectionID == b.ConnectionID && a.ProviderKey == b.ProviderKey && a.AuthMode == b.AuthMode &&
		a.Capability == b.Capability && a.Basis == b.Basis && a.BasisRef == b.BasisRef &&
		a.CatalogVersion == b.CatalogVersion && a.Digest == b.Digest &&
		a.CheckedAt.Equal(b.CheckedAt) && a.CreatedAt.Equal(b.CreatedAt) && a.UpdatedAt.Equal(b.UpdatedAt)
}

func digestOf(version int) string {
	return assessmentFor("c", zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, version).Digest
}

// RunConnectionZDRContract runs the whole suite.
func RunConnectionZDRContract(t *testing.T, h ZDRHarness) {
	ctx := context.Background()

	t.Run("an assessment creates the record and a repeat changes nothing", func(t *testing.T) {
		f := newFixture(t, h)
		first := f.apply(1)
		if !first.Created || first.Changed || first.Previous != nil || len(first.Revoked) != 0 {
			t.Fatalf("first apply = %+v, want created only", first)
		}
		want := assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 1)
		got := first.Record
		if got.ConnectionID != f.conn || got.ProviderKey != want.ProviderKey || got.AuthMode != want.AuthMode ||
			got.Capability != want.Capability || got.Basis != want.Basis || got.BasisRef != want.BasisRef ||
			got.CatalogVersion != 1 || got.Digest != want.Digest || got.CheckedAt.IsZero() {
			t.Fatalf("record = %+v, want %+v", got, want)
		}
		stored, err := f.repo.GetConnectionZDR(ctx, f.conn)
		if err != nil || !sameRecord(stored, got) {
			t.Fatalf("GetConnectionZDR = (%+v, %v), want the created record", stored, err)
		}
		// The repeat names a later check time on purpose: an unchanged assessment must not rewrite
		// checked_at (or any other column), whatever time the caller says it checked.
		later := assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 1)
		later.CheckedAt = got.CheckedAt.Add(time.Hour)
		again, err := f.repo.ApplyConnectionZDRAssessment(ctx, later)
		if err != nil {
			t.Fatalf("repeat apply: %v", err)
		}
		if again.Created || again.Changed || again.Previous == nil || len(again.Revoked) != 0 {
			t.Fatalf("repeat apply = %+v, want a no-op", again)
		}
		if !sameRecord(again.Record, got) {
			t.Fatalf("a repeated assessment rewrote the record: before %+v after %+v", got, again.Record)
		}
	})

	t.Run("a connection that does not exist or has no record is not found", func(t *testing.T) {
		f := newFixture(t, h)
		_, err := f.repo.ApplyConnectionZDRAssessment(ctx, assessmentFor("conn-"+uuid.NewString(), zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 1))
		if !errors.Is(err, store.ErrNotFound) {
			t.Fatalf("apply on a missing connection = %v, want ErrNotFound", err)
		}
		if _, err := f.repo.GetConnectionZDR(ctx, f.conn); !errors.Is(err, store.ErrNotFound) {
			t.Fatalf("GetConnectionZDR of an unassessed connection = %v, want ErrNotFound (unassessed)", err)
		}
	})

	t.Run("a malformed assessment is refused before any state changes", func(t *testing.T) {
		f := newFixture(t, h)
		good := assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 1)
		mutations := map[string]func(*store.ConnectionZDRAssessment){
			"no connection":             func(a *store.ConnectionZDRAssessment) { a.ConnectionID = "" },
			"no provider key":           func(a *store.ConnectionZDRAssessment) { a.ProviderKey = "" },
			"no auth mode":              func(a *store.ConnectionZDRAssessment) { a.AuthMode = "" },
			"unknown capability":        func(a *store.ConnectionZDRAssessment) { a.Capability = "mostly" },
			"unknown basis":             func(a *store.ConnectionZDRAssessment) { a.Basis = "vibes" },
			"no basis ref":              func(a *store.ConnectionZDRAssessment) { a.BasisRef = "" },
			"catalog version zero":      func(a *store.ConnectionZDRAssessment) { a.CatalogVersion = 0 },
			"digest of the wrong shape": func(a *store.ConnectionZDRAssessment) { a.Digest = "abc" },
			"a digest invented by the caller": func(a *store.ConnectionZDRAssessment) {
				a.Digest = digestOf(99)
			},
			"a capability changed under the old digest": func(a *store.ConnectionZDRAssessment) {
				a.Capability = zdr.CapabilitySupported
			},
		}
		for name, mutate := range mutations {
			bad := good
			mutate(&bad)
			if _, err := f.repo.ApplyConnectionZDRAssessment(ctx, bad); !errors.Is(err, store.ErrInvalidZDR) {
				t.Errorf("%s: err = %v, want ErrInvalidZDR", name, err)
			}
		}
		if _, err := f.repo.GetConnectionZDR(ctx, f.conn); !errors.Is(err, store.ErrNotFound) {
			t.Fatalf("a refused assessment left a record behind: %v", err)
		}
	})

	t.Run("a changed digest revokes every dependent acknowledgement and says which", func(t *testing.T) {
		f := newFixture(t, h)
		v1 := f.apply(1)
		ack := f.acknowledge(v1.Record.Digest)

		v2 := f.apply(2)
		if v2.Created || !v2.Changed || v2.Previous == nil || v2.Previous.Digest != v1.Record.Digest || v2.Record.Digest != digestOf(2) {
			t.Fatalf("v2 apply = %+v, want a digest change from v1", v2)
		}
		if len(v2.Revoked) != 1 || v2.Revoked[0].ID != ack.ID || v2.Revoked[0].RevokeReason != store.ZDRRevokeCapabilityChanged || v2.Revoked[0].RevokedAt == nil {
			t.Fatalf("revoked = %+v, want exactly the acknowledgement of the old digest, reason capability_changed", v2.Revoked)
		}
		if active := f.ledger(true); len(active) != 0 {
			t.Fatalf("active acknowledgements after a digest change = %+v, want none", active)
		}
		if all := f.ledger(false); len(all) != 1 || all[0].RevokedAt == nil || all[0].RevokeReason != store.ZDRRevokeCapabilityChanged {
			t.Fatalf("ledger after a digest change = %+v, want the one row kept and marked revoked", all)
		}
		// An acknowledgement of the OLD assessment can no longer be recorded; one of the new can.
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, f.ackInput(v1.Record.Digest)); !errors.Is(err, store.ErrConflict) {
			t.Fatalf("acknowledging the stale digest = %v, want ErrConflict", err)
		}
		f.acknowledge(v2.Record.Digest)
	})

	t.Run("an unchanged assessment leaves a valid acknowledgement alone", func(t *testing.T) {
		f := newFixture(t, h)
		v1 := f.apply(1)
		ack := f.acknowledge(v1.Record.Digest)
		again := f.apply(1)
		if again.Changed || len(again.Revoked) != 0 {
			t.Fatalf("repeat apply = %+v", again)
		}
		active := f.ledger(true)
		if len(active) != 1 || active[0].ID != ack.ID || active[0].RevokedAt != nil {
			t.Fatalf("active acknowledgements = %+v, want the original untouched", active)
		}
	})

	t.Run("an acknowledgement is bound to the owner, a live connection and the current digest", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record

		otherUser := f.ackInput(rec.Digest)
		otherUser.UserID = "someone-else"
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, otherUser); !errors.Is(err, store.ErrNotFound) {
			t.Errorf("another member acknowledging = %v, want ErrNotFound", err)
		}
		otherOrg := f.ackInput(rec.Digest)
		otherOrg.OrganizationID = "another-org"
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, otherOrg); !errors.Is(err, store.ErrNotFound) {
			t.Errorf("another organisation acknowledging = %v, want ErrNotFound", err)
		}
		unknown := f.ackInput(rec.Digest)
		unknown.ConnectionID = "conn-" + uuid.NewString()
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, unknown); !errors.Is(err, store.ErrNotFound) {
			t.Errorf("an unknown connection = %v, want ErrNotFound", err)
		}
		unassessed := f.seedConnection(f.org, f.user)
		noRecord := f.ackInput(rec.Digest)
		noRecord.ConnectionID = unassessed
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, noRecord); !errors.Is(err, store.ErrNotFound) {
			t.Errorf("an unassessed connection = %v, want ErrNotFound", err)
		}
		stale := f.ackInput(digestOf(7))
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, stale); !errors.Is(err, store.ErrConflict) {
			t.Errorf("a stale digest = %v, want ErrConflict", err)
		}
		if got := f.ledger(false); len(got) != 0 {
			t.Fatalf("a refused acknowledgement left a ledger row: %+v", got)
		}
	})

	t.Run("a supported connection needs no acknowledgement and cannot be given one", func(t *testing.T) {
		f := newFixture(t, h)
		supported := assessmentFor(f.conn, zdr.CapabilitySupported, zdr.BasisVerevonAttestedDeployment, 1)
		if _, err := f.repo.ApplyConnectionZDRAssessment(ctx, supported); err != nil {
			t.Fatalf("apply supported: %v", err)
		}
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, f.ackInput(supported.Digest)); !errors.Is(err, store.ErrInvalidZDR) {
			t.Fatalf("acknowledging a supported connection = %v, want ErrInvalidZDR", err)
		}
	})

	t.Run("a malformed acknowledgement is refused", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record
		mutations := map[string]func(*store.NewConnectionZDRAcknowledgement){
			"no text version":       func(a *store.NewConnectionZDRAcknowledgement) { a.TextVersion = "" },
			"unknown locale":        func(a *store.NewConnectionZDRAcknowledgement) { a.Locale = "fr" },
			"no text":               func(a *store.NewConnectionZDRAcknowledgement) { a.TextSnapshot = "" },
			"hash of another text":  func(a *store.NewConnectionZDRAcknowledgement) { a.TextSHA256 = digestOf(5) },
			"unknown way":           func(a *store.NewConnectionZDRAcknowledgement) { a.AcknowledgedVia = "email" },
			"digest of wrong shape": func(a *store.NewConnectionZDRAcknowledgement) { a.Digest = "xyz" },
			"no user":               func(a *store.NewConnectionZDRAcknowledgement) { a.UserID = "" },
		}
		for name, mutate := range mutations {
			bad := f.ackInput(rec.Digest)
			mutate(&bad)
			if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, bad); !errors.Is(err, store.ErrInvalidZDR) {
				t.Errorf("%s: err = %v, want ErrInvalidZDR", name, err)
			}
		}
		if got := f.ledger(false); len(got) != 0 {
			t.Fatalf("a refused acknowledgement left a ledger row: %+v", got)
		}
	})

	t.Run("the ledger copies what was assessed, not what the caller says", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record
		ack := f.acknowledge(rec.Digest)
		if ack.ProviderKey != rec.ProviderKey || ack.Capability != rec.Capability || ack.Basis != rec.Basis ||
			ack.CatalogVersion != rec.CatalogVersion || ack.Digest != rec.Digest {
			t.Fatalf("acknowledgement %+v does not carry the assessment %+v it is bound to", ack, rec)
		}
		if ack.OrganizationID != f.org || ack.UserID != f.user || ack.ConnectionID != f.conn || ack.ID == "" || ack.AcknowledgedAt.IsZero() || ack.RevokedAt != nil {
			t.Fatalf("acknowledgement = %+v", ack)
		}
		if ack.TextVersion != "2026-10-a" || ack.Locale != "no" || !ack.OrgZDRAtAck || ack.AcknowledgedVia != store.ZDRAcknowledgedViaConnect || ack.TextSnapshot == "" {
			t.Fatalf("acknowledgement text fields = %+v", ack)
		}
	})

	t.Run("a replay is idempotent and returns the stored acknowledgement", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record
		first := f.acknowledge(rec.Digest)
		replay := f.ackInput(rec.Digest)
		replay.TextVersion = "a-newer-text-version"
		again, created, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, replay)
		if err != nil || created {
			t.Fatalf("replay = (created=%v, err=%v), want an unchanged existing row", created, err)
		}
		if again.ID != first.ID || again.TextVersion != first.TextVersion {
			t.Fatalf("replay returned %+v, want the stored %+v", again, first)
		}
		if rows := f.ledger(false); len(rows) != 1 {
			t.Fatalf("ledger has %d rows after a replay, want 1", len(rows))
		}
	})

	t.Run("after a revocation the same assessment can be acknowledged again", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record
		first := f.acknowledge(rec.Digest)
		revoked, err := f.repo.RevokeConnectionZDRAcknowledgements(ctx, f.conn, store.ZDRRevokeOwnerRevoked, time.Time{})
		if err != nil || len(revoked) != 1 || revoked[0].ID != first.ID || revoked[0].RevokeReason != store.ZDRRevokeOwnerRevoked || revoked[0].RevokedAt == nil {
			t.Fatalf("revoke = (%+v, %v)", revoked, err)
		}
		again, err := f.repo.RevokeConnectionZDRAcknowledgements(ctx, f.conn, store.ZDRRevokeConnectionRemoved, time.Time{})
		if err != nil || len(again) != 0 {
			t.Fatalf("second revoke = (%+v, %v), want nothing to revoke", again, err)
		}
		// A revoked row is final: the second revoke did not rewrite its reason.
		for _, row := range f.ledger(false) {
			if row.ID == first.ID && row.RevokeReason != store.ZDRRevokeOwnerRevoked {
				t.Fatalf("a revoked acknowledgement was revoked again with reason %q", row.RevokeReason)
			}
		}
		second := f.acknowledge(rec.Digest)
		if second.ID == first.ID {
			t.Fatal("a new acknowledgement reused the revoked row")
		}
		rows := f.ledger(false)
		if len(rows) != 2 || len(f.ledger(true)) != 1 {
			t.Fatalf("ledger = %+v, want one revoked and one active row", rows)
		}
	})

	t.Run("revoke reasons are a closed set", func(t *testing.T) {
		f := newFixture(t, h)
		f.apply(1)
		if _, err := f.repo.RevokeConnectionZDRAcknowledgements(ctx, f.conn, "because", time.Time{}); !errors.Is(err, store.ErrInvalidZDR) {
			t.Fatalf("revoke with an invented reason = %v, want ErrInvalidZDR", err)
		}
	})

	t.Run("the ledger is newest first and can be limited to the active rows", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record
		oldInput := f.ackInput(rec.Digest)
		oldInput.AcknowledgedAt = time.Now().UTC().Add(-2 * time.Hour)
		old, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, oldInput)
		if err != nil {
			t.Fatalf("insert old: %v", err)
		}
		if _, err := f.repo.RevokeConnectionZDRAcknowledgements(ctx, f.conn, store.ZDRRevokeOwnerRevoked, time.Time{}); err != nil {
			t.Fatalf("revoke: %v", err)
		}
		newer := f.acknowledge(rec.Digest)
		all := f.ledger(false)
		if len(all) != 2 || all[0].ID != newer.ID || all[1].ID != old.ID {
			t.Fatalf("ledger order = %+v, want newest first", all)
		}
		if active := f.ledger(true); len(active) != 1 || active[0].ID != newer.ID {
			t.Fatalf("active rows = %+v", active)
		}
	})

	t.Run("the record and the ledger outlive the connection", func(t *testing.T) {
		f := newFixture(t, h)
		rec := f.apply(1).Record
		ack := f.acknowledge(rec.Digest)
		if _, err := f.repo.MarkConnectionDeleted(ctx, f.conn); err != nil {
			t.Fatalf("MarkConnectionDeleted: %v", err)
		}
		rows := f.ledger(false)
		if len(rows) != 1 || rows[0].ID != ack.ID {
			t.Fatalf("the ledger lost a row when its connection was soft-deleted: %+v", rows)
		}
		if _, err := f.repo.GetConnectionZDR(ctx, f.conn); err != nil {
			t.Fatalf("the record vanished with the soft delete: %v", err)
		}
		// But nobody can acknowledge a deleted connection.
		if _, _, err := f.repo.InsertConnectionZDRAcknowledgement(ctx, f.ackInput(rec.Digest)); !errors.Is(err, store.ErrNotFound) {
			t.Fatalf("acknowledging a deleted connection = %v, want ErrNotFound", err)
		}
	})

	t.Run("no generic connection write touches the record, even a stale write-back", func(t *testing.T) {
		f := newFixture(t, h)
		f.apply(1)
		stale, err := f.repo.GetConnection(ctx, f.conn)
		if err != nil {
			t.Fatalf("GetConnection: %v", err)
		}
		current := f.apply(2).Record // the digest moves on while the caller still holds `stale`

		stale.Status = "active"
		stale.DisplayName = "stale write-back"
		stale.ProviderContext = map[string]string{"zdr_capability": "supported", "zdr_digest": digestOf(1), "auth_mode": "api_key"}
		if _, err := f.repo.UpsertConnection(ctx, stale); err != nil {
			t.Fatalf("UpsertConnection: %v", err)
		}
		if err := f.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
			_, reconnectErr := tx.ReconnectConnection(ctx, stale)
			return reconnectErr
		}); err != nil {
			t.Fatalf("ReconnectConnection: %v", err)
		}
		if _, err := f.repo.UpdateConnectionCredentials(ctx, stale); err != nil {
			t.Fatalf("UpdateConnectionCredentials: %v", err)
		}
		if _, err := f.repo.UpdateConnectionCapabilities(ctx, f.conn, []string{"model.inference", "zdr.supported"}); err != nil {
			t.Fatalf("UpdateConnectionCapabilities: %v", err)
		}
		if _, err := f.repo.UpdateConnectionProviderContext(ctx, f.conn, map[string]string{"zdr_capability": "supported"}); err != nil {
			t.Fatalf("UpdateConnectionProviderContext: %v", err)
		}
		if _, err := f.repo.BindConnectionProviderContext(ctx, f.conn, "zdr_digest", digestOf(1)); err != nil {
			t.Fatalf("BindConnectionProviderContext: %v", err)
		}
		after, err := f.repo.GetConnectionZDR(ctx, f.conn)
		if err != nil || !sameRecord(after, current) {
			t.Fatalf("record after generic writes = (%+v, %v), want it untouched %+v", after, err, current)
		}
	})

	t.Run("a digest change, its revocations and its audit row commit or roll back together", func(t *testing.T) {
		f := newFixture(t, h)
		v1 := f.apply(1).Record
		ack := f.acknowledge(v1.Digest)
		boom := errors.New("audit outbox unavailable")

		auditID := "audit:z12:" + uuid.NewString()
		err := f.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
			result, applyErr := tx.ApplyConnectionZDRAssessment(ctx, assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 2))
			if applyErr != nil {
				return applyErr
			}
			if !result.Changed || len(result.Revoked) != 1 {
				return fmt.Errorf("apply inside the transaction = %+v", result)
			}
			if insertErr := tx.InsertAuditEvent(ctx, store.AuditEvent{ID: auditID, OrganizationID: f.org, UserID: f.user, ConnectionID: f.conn, EventType: "subscription_connection.zdr_assessed"}); insertErr != nil {
				return insertErr
			}
			return boom
		})
		if !errors.Is(err, boom) {
			t.Fatalf("transaction error = %v, want %v", err, boom)
		}
		after, getErr := f.repo.GetConnectionZDR(ctx, f.conn)
		if getErr != nil || !sameRecord(after, v1) {
			t.Fatalf("record after a rolled-back transaction = (%+v, %v), want v1 %+v", after, getErr, v1)
		}
		if active := f.ledger(true); len(active) != 1 || active[0].ID != ack.ID {
			t.Fatalf("a rolled-back transaction revoked the acknowledgement: %+v", active)
		}
		if h.AuditEventExists(t, f.repo, auditID) {
			t.Fatal("a rolled-back transaction left its audit row behind")
		}

		committedAuditID := "audit:z12:" + uuid.NewString()
		err = f.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
			if _, applyErr := tx.ApplyConnectionZDRAssessment(ctx, assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 2)); applyErr != nil {
				return applyErr
			}
			return tx.InsertAuditEvent(ctx, store.AuditEvent{ID: committedAuditID, OrganizationID: f.org, UserID: f.user, ConnectionID: f.conn, EventType: "subscription_connection.zdr_assessed"})
		})
		if err != nil {
			t.Fatalf("committing transaction: %v", err)
		}
		if got, _ := f.repo.GetConnectionZDR(ctx, f.conn); got.Digest != digestOf(2) {
			t.Fatalf("committed record digest = %s, want v2", got.Digest)
		}
		if active := f.ledger(true); len(active) != 0 {
			t.Fatalf("committed digest change left active acknowledgements: %+v", active)
		}
		if !h.AuditEventExists(t, f.repo, committedAuditID) {
			t.Fatal("a committed transaction lost its audit row")
		}
	})

	t.Run("concurrent first assessments converge on one record", func(t *testing.T) {
		f := newFixture(t, h)
		const workers = 8
		results := make([]store.ConnectionZDRApplyResult, workers)
		errs := make([]error, workers)
		var wg sync.WaitGroup
		start := make(chan struct{})
		for i := 0; i < workers; i++ {
			wg.Add(1)
			go func(i int) {
				defer wg.Done()
				<-start
				results[i], errs[i] = f.repo.ApplyConnectionZDRAssessment(ctx, assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 1))
			}(i)
		}
		close(start)
		wg.Wait()
		created := 0
		for i := range results {
			if errs[i] != nil {
				t.Fatalf("worker %d: %v", i, errs[i])
			}
			if results[i].Created {
				created++
			}
			if results[i].Changed || results[i].Record.Digest != digestOf(1) {
				t.Fatalf("worker %d result = %+v", i, results[i])
			}
		}
		if created != 1 {
			t.Fatalf("%d workers reported creating the record, want exactly 1", created)
		}
	})

	t.Run("an acknowledgement racing a digest change is never left active on the old digest", func(t *testing.T) {
		const rounds = 12
		for round := 0; round < rounds; round++ {
			f := newFixture(t, h)
			v1 := f.apply(1).Record
			var wg sync.WaitGroup
			start := make(chan struct{})
			wg.Add(2)
			go func() {
				defer wg.Done()
				<-start
				_, _, _ = f.repo.InsertConnectionZDRAcknowledgement(ctx, f.ackInput(v1.Digest))
			}()
			go func() {
				defer wg.Done()
				<-start
				_, _ = f.repo.ApplyConnectionZDRAssessment(ctx, assessmentFor(f.conn, zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 2))
			}()
			close(start)
			wg.Wait()
			current, err := f.repo.GetConnectionZDR(ctx, f.conn)
			if err != nil {
				t.Fatalf("round %d: %v", round, err)
			}
			for _, row := range f.ledger(true) {
				if row.Digest != current.Digest {
					t.Fatalf("round %d: active acknowledgement on digest %s while the record is on %s", round, row.Digest, current.Digest)
				}
			}
		}
	})
}
