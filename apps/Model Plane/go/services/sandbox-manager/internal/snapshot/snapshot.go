// Package snapshot holds the durable (Postgres-backed) sandbox snapshot
// store and domain types. A snapshot captures a labelled checkpoint of a
// sandbox associated with a lease.
//
// Ported from an in-memory map + sync.RWMutex per
// apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
// §2, mirroring internal/lease's own port (and, before that,
// capability-core's internal/registry/scope_store.go template).
package snapshot

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
)

// Sentinel errors for snapshot lookups.
var (
	ErrSnapshotNotFound = errors.New("snapshot not found")
	ErrInvalidLease     = errors.New("invalid lease for snapshot")
)

// Snapshot is a labelled checkpoint of a sandbox.
type Snapshot struct {
	ID        string
	LeaseID   string
	Label     string
	ObjectKey string
	CreatedAt time.Time
}

type snapshotDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// Store is the durable snapshot registry backed by the `snapshots` table
// (migration 0002).
type Store struct {
	pool   snapshotDatabase
	nowFn  func() time.Time
	randFn func([]byte) (int, error)
}

// NewStore constructs a Store over pool. Returns an error for a nil pool.
func NewStore(pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &Store{pool: pool, nowFn: time.Now, randFn: rand.Read}, nil
}

// Create produces a new snapshot bound to the supplied lease.
func (s *Store) Create(ctx context.Context, l *lease.Lease, label string) (*Snapshot, error) {
	if l == nil || l.ID == "" {
		return nil, ErrInvalidLease
	}
	id, err := s.newID()
	if err != nil {
		return nil, err
	}
	snap := &Snapshot{
		ID:        id,
		LeaseID:   l.ID,
		Label:     label,
		ObjectKey: "snapshots/" + l.ID + "/" + id,
		CreatedAt: s.nowFn(),
	}
	_, err = s.pool.Exec(ctx, `
		INSERT INTO snapshots (id, lease_id, label, object_key, created_at)
		VALUES ($1, $2, $3, $4, $5)
	`, snap.ID, snap.LeaseID, snap.Label, snap.ObjectKey, snap.CreatedAt)
	if err != nil {
		return nil, fmt.Errorf("create snapshot: %w", err)
	}
	return snap, nil
}

// Get returns a snapshot by ID, or ErrSnapshotNotFound.
func (s *Store) Get(ctx context.Context, id string) (*Snapshot, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, lease_id, label, object_key, created_at FROM snapshots WHERE id = $1
	`, id)
	var snap Snapshot
	err := row.Scan(&snap.ID, &snap.LeaseID, &snap.Label, &snap.ObjectKey, &snap.CreatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrSnapshotNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("get snapshot: %w", err)
	}
	return &snap, nil
}

func (s *Store) newID() (string, error) {
	var buf [16]byte
	if _, err := s.randFn(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}
