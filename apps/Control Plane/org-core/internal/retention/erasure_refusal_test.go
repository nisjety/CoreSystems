package retention

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

// refusalStore records erasure refusals. The embedded Store is nil: AuditRefusedErasure
// must touch nothing but EnqueueErasureRefusal.
type refusalStore struct {
	Store
	rows []map[string]any
	fail error
}

func (s *refusalStore) EnqueueErasureRefusal(_ context.Context, orgID string, actor Actor, details map[string]any) error {
	if s.fail != nil {
		return s.fail
	}
	s.rows = append(s.rows, erasureRefusalPayload("evt-1", orgID, actor, details, time.Unix(0, 0)))
	return nil
}

// F-f10-06: one refusal enqueues exactly one content-free row: kind, operation and
// counts, the actor's id and role, outcome denied, and no hold reference.
func TestAuditRefusedErasureEnqueuesOneContentFreeRow(t *testing.T) {
	store := &refusalStore{}
	service := NewService(store, nil, nil)
	actor := Actor{ID: "system:org-core-purge-cron", Role: "system"}
	if err := service.AuditRefusedErasure(context.Background(), "org-1", actor, ErasureOpPurge, 1, 2); err != nil {
		t.Fatalf("audit refusal: %v", err)
	}
	if len(store.rows) != 1 {
		t.Fatalf("rows = %d, want exactly 1", len(store.rows))
	}
	row := store.rows[0]
	details, _ := row["details"].(map[string]any)
	if details["kind"] != "erasure_refused" || details["op"] != ErasureOpPurge ||
		details["org_holds"] != 1 || details["thread_holds"] != 2 || len(details) != 4 {
		t.Fatalf("details = %v", details)
	}
	if row["outcome"] != "denied" || row["event"] != AuditEvent || row["org_id"] != "org-1" ||
		row["user_id"] != actor.ID || row["actor_role"] != actor.Role || row["resource_id"] != "org-1" {
		t.Fatalf("payload = %v", row)
	}
	body, err := json.Marshal(row)
	if err != nil {
		t.Fatalf("encode payload: %v", err)
	}
	if strings.Contains(string(body), "hold_ref") {
		t.Fatalf("a refusal row must never carry a hold reference: %s", body)
	}
}

func TestAuditRefusedErasureAcceptsEveryOpAndUnknownCounts(t *testing.T) {
	for _, op := range []string{ErasureOpHardDelete, ErasureOpSoftDelete, ErasureOpPurge} {
		store := &refusalStore{}
		if err := NewService(store, nil, nil).AuditRefusedErasure(context.Background(), "org-1",
			Actor{ID: "u-1", Role: "owner"}, op, HoldCountUnknown, HoldCountUnknown); err != nil || len(store.rows) != 1 {
			t.Fatalf("op %s: rows=%d err=%v", op, len(store.rows), err)
		}
	}
}

func TestAuditRefusedErasureRefusesBadInputWithoutWriting(t *testing.T) {
	cases := map[string]func(*Service) error{
		"no org": func(s *Service) error {
			return s.AuditRefusedErasure(context.Background(), " ", Actor{ID: "u", Role: "owner"}, ErasureOpPurge, 1, 0)
		},
		"no actor id": func(s *Service) error {
			return s.AuditRefusedErasure(context.Background(), "org-1", Actor{Role: "owner"}, ErasureOpPurge, 1, 0)
		},
		"no actor role": func(s *Service) error {
			return s.AuditRefusedErasure(context.Background(), "org-1", Actor{ID: "u"}, ErasureOpPurge, 1, 0)
		},
		"free-text op": func(s *Service) error {
			return s.AuditRefusedErasure(context.Background(), "org-1", Actor{ID: "u", Role: "owner"}, "hard delete", 1, 0)
		},
		"negative count": func(s *Service) error {
			return s.AuditRefusedErasure(context.Background(), "org-1", Actor{ID: "u", Role: "owner"}, ErasureOpPurge, -2, 0)
		},
	}
	for name, call := range cases {
		store := &refusalStore{}
		if err := call(NewService(store, nil, nil)); err == nil || len(store.rows) != 0 {
			t.Fatalf("%s: err=%v rows=%d; want an error and no row", name, err, len(store.rows))
		}
	}
}

func TestAuditRefusedErasureWithoutARefusalStoreIsUnavailable(t *testing.T) {
	var plain Store // a Store that cannot record refusals
	err := NewService(plain, nil, nil).AuditRefusedErasure(context.Background(), "org-1", Actor{ID: "u", Role: "owner"}, ErasureOpHardDelete, 1, 0)
	if !errors.Is(err, ErrRefusalAuditUnavailable) {
		t.Fatalf("err = %v, want ErrRefusalAuditUnavailable", err)
	}
	store := &refusalStore{fail: errors.New("outbox down")}
	if err := NewService(store, nil, nil).AuditRefusedErasure(context.Background(), "org-1", Actor{ID: "u", Role: "owner"}, ErasureOpHardDelete, 1, 0); err == nil {
		t.Fatal("a store failure must be returned to the caller")
	}
}

// The repository writes the refusal row under the organisation's scope, as RLS and
// migration 031's payload checks require: exactly one row, outcome denied, no hold
// reference, and the hold it refused for stays untouched.
func TestControlLifecycleErasureRefusalRowUnderRLS(t *testing.T) {
	ctx, db := openRetentionLifecycleDB(t)
	orgID := fmt.Sprintf("org-retention-refusal-%d", time.Now().UnixNano())
	seedRetentionOrganization(t, ctx, db, orgID)
	repo := NewRepository(db)
	if _, err := repo.PlaceHold(ctx, orgID, Actor{ID: "u-comp", Role: "compliance"},
		hold("lh_"+strings.Repeat("e", 32), ScopeOrg, orgID)); err != nil {
		t.Fatalf("place hold: %v", err)
	}
	service := NewService(repo, nil, nil)
	if err := service.AuditRefusedErasure(ctx, orgID, Actor{ID: "u-owner", Role: "owner"}, ErasureOpHardDelete, 1, 0); err != nil {
		t.Fatalf("audit refusal: %v", err)
	}
	var rows, leaked int
	var outcome, op string
	if err := db.Pool.QueryRow(ctx, `
SELECT count(*), count(*) FILTER (WHERE payload::text LIKE '%Matter 2026-17%'),
       max(payload->>'outcome'), max(payload->'details'->>'op')
FROM org_retention_audit_outbox
WHERE org_id = $1 AND subject = $2 AND payload->'details'->>'kind' = 'erasure_refused'`,
		orgID, AuditSubject).Scan(&rows, &leaked, &outcome, &op); err != nil {
		t.Fatalf("read refusal rows: %v", err)
	}
	if rows != 1 || leaked != 0 || outcome != "denied" || op != ErasureOpHardDelete {
		t.Fatalf("refusal rows=%d leaked=%d outcome=%q op=%q; want one content-free denied row", rows, leaked, outcome, op)
	}
}
