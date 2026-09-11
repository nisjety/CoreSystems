// Package lease holds the durable (Postgres-backed) sandbox lease store and
// domain types. A lease represents a bounded right to use a sandbox for a
// thread- or agent-scoped workload.
//
// Ported from an in-memory map + sync.RWMutex per
// apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
// §2, following capability-core's internal/registry/scope_store.go template:
// a narrow Exec/Query/QueryRow interface wrapping *pgxpool.Pool (so unit
// tests substitute a stub without a real database), and — like that store —
// no explicit transactions or row locks; each method is one or two
// independent round-trip statements. A read-then-conditional-update pattern
// (used by Activate/BeginSnapshot/ReleaseScoped below) has a narrower
// consistency guarantee than a single atomic statement would, but matches
// this codebase's own established practice rather than introducing new
// locking machinery for a single-process-owned resource that was previously
// protected only by an in-memory mutex anyway (which itself never protected
// against a horizontally-scaled second sandbox-manager instance — a
// Postgres-backed store is a strict improvement on that axis even with this
// pattern, not a regression).
package lease

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

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// Sentinel errors for lease lookups.
var (
	ErrLeaseNotFound = errors.New("lease not found")
	ErrLeaseExpired  = errors.New("lease expired")
	// ErrLeaseBackendMismatch signals that a caller's asserted backend id
	// does not match the backend this lease was pinned to at AcquireLease
	// time. A Space-scoped lease request that lands on a different
	// sandbox-manager instance than the one its capability decision named
	// must be refused, never silently served by whichever instance answered.
	ErrLeaseBackendMismatch = errors.New("lease backend mismatch")
	// ErrLeaseNotActivated signals a snapshot attempt against a Space-scoped
	// lease still in SCRATCH — nothing durable exists yet by definition.
	ErrLeaseNotActivated = errors.New("lease has not been activated")
)

// Lease is an issued sandbox reservation.
type Lease struct {
	ID        string
	ScopeID   string
	ScopeType string
	OrgID     string
	OwnerID   string
	Endpoint  string
	// SpaceID is set only for a Space-scoped lease (one acquired with a
	// verified Space capability decision); empty for the pre-existing
	// thread/agent-scoped acquisition path.
	SpaceID string
	// BackendID is this process's own configured backend id, stamped at
	// Create time — empty for a non-Space lease, where there is nothing to
	// pin.
	BackendID string
	State     mpv1.SandboxLifecycleState
	ExpiresAt time.Time
	CreatedAt time.Time
}

// IsExpired reports whether the lease has passed its TTL relative to now.
func (l *Lease) IsExpired(now time.Time) bool {
	return now.After(l.ExpiresAt)
}

type leaseDatabase interface {
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// Store is the durable lease registry backed by the `leases` table
// (migration 0002).
type Store struct {
	pool   leaseDatabase
	nowFn  func() time.Time
	randFn func([]byte) (int, error)
}

// NewStore constructs a Store over pool. Returns an error for a nil pool —
// there is no in-memory fallback here; see cmd/main.go for the
// ephemeral-development escape hatch that chooses not to construct a Store
// at all in that mode.
func NewStore(pool *pgxpool.Pool) (*Store, error) {
	if pool == nil {
		return nil, fmt.Errorf("pgx pool required")
	}
	return &Store{pool: pool, nowFn: time.Now, randFn: rand.Read}, nil
}

// Create mints a new lease with the given scope and TTL. spaceID and
// backendID are empty for the pre-existing thread/agent-scoped acquisition
// path; a Space-scoped caller supplies both, and the lease starts in
// SCRATCH — credential-free by construction until ActivateLease promotes it.
func (s *Store) Create(ctx context.Context, scopeID, scopeType, orgID, ownerID, spaceID, backendID string, ttl time.Duration) (*Lease, error) {
	id, err := s.newID()
	if err != nil {
		return nil, err
	}
	now := s.nowFn()
	l := &Lease{
		ID:        id,
		ScopeID:   scopeID,
		ScopeType: scopeType,
		OrgID:     orgID,
		OwnerID:   ownerID,
		Endpoint:  "sandbox://" + id,
		SpaceID:   spaceID,
		BackendID: backendID,
		State:     mpv1.SandboxLifecycleState_SCRATCH,
		ExpiresAt: now.Add(ttl),
		CreatedAt: now,
	}
	_, err = s.pool.Exec(ctx, `
		INSERT INTO leases (id, scope_id, scope_type, org_id, owner_id, endpoint, space_id, backend_id, state, expires_at, created_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
	`, l.ID, l.ScopeID, l.ScopeType, l.OrgID, l.OwnerID, l.Endpoint, l.SpaceID, l.BackendID, int32(l.State), l.ExpiresAt, l.CreatedAt)
	if err != nil {
		return nil, fmt.Errorf("create lease: %w", err)
	}
	return l, nil
}

// lookup fetches a lease and validates it in the order every scoped
// operation needs: not-found (missing id, wrong org/owner, or DESTROYED — a
// destroyed lease is unfindable, not merely inert, so further
// Snapshot/Activate calls against it fail closed the same way a stale id
// does), then expiry, then the caller's asserted backend id. ownerID
// participates in the WHERE clause only when non-empty, matching the
// original in-memory semantics (a service-principal caller passes "" and
// may see any owner's lease within its org).
func (s *Store) lookup(ctx context.Context, id, orgID, ownerID, backendID string) (*Lease, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, scope_id, scope_type, org_id, owner_id, endpoint, space_id, backend_id, state, expires_at, created_at
		FROM leases
		WHERE id = $1 AND org_id = $2 AND ($3 = '' OR owner_id = $3) AND state <> $4
	`, id, orgID, ownerID, int32(mpv1.SandboxLifecycleState_DESTROYED))
	l, err := scanLease(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrLeaseNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("lookup lease: %w", err)
	}
	if l.IsExpired(s.nowFn()) {
		return nil, ErrLeaseExpired
	}
	if l.BackendID != backendID {
		return nil, ErrLeaseBackendMismatch
	}
	return l, nil
}

// GetScoped returns a lease by ID only within the verified organization,
// optional user owner, and asserted backend id. backendID is only
// meaningful for a Space-scoped lease (both sides are empty for the
// pre-existing path, so it trivially matches).
func (s *Store) GetScoped(ctx context.Context, id, orgID, ownerID, backendID string) (*Lease, error) {
	return s.lookup(ctx, id, orgID, ownerID, backendID)
}

// Activate transitions a Space-scoped lease from SCRATCH to ACTIVE — the
// first time a caller needs more than the credential-free scratch
// allowlist. A no-op (state unchanged) if already ACTIVE or SNAPSHOTTING.
// Meaningless for a non-Space lease: its State starts at SCRATCH but
// nothing in this store gates behavior on it, so Activate on one always
// succeeds without doing anything.
func (s *Store) Activate(ctx context.Context, id, orgID, ownerID, backendID string) (*Lease, error) {
	l, err := s.lookup(ctx, id, orgID, ownerID, backendID)
	if err != nil {
		return nil, err
	}
	if l.State != mpv1.SandboxLifecycleState_SCRATCH {
		return l, nil
	}
	_, err = s.pool.Exec(ctx, `UPDATE leases SET state = $2 WHERE id = $1 AND state = $3`,
		id, int32(mpv1.SandboxLifecycleState_ACTIVE), int32(mpv1.SandboxLifecycleState_SCRATCH))
	if err != nil {
		return nil, fmt.Errorf("activate lease: %w", err)
	}
	l.State = mpv1.SandboxLifecycleState_ACTIVE
	return l, nil
}

// BeginSnapshot marks a Space-scoped lease SNAPSHOTTING, or reports
// ErrLeaseNotActivated if it is still SCRATCH — nothing durable exists yet
// by definition. A non-Space lease (SpaceID == "") is exempt from this gate
// entirely: the SCRATCH/ACTIVE/SNAPSHOTTING state machine exists only for
// the Space capability feature, and the pre-existing thread/agent-scoped
// snapshot path predates it and must keep working exactly as it did before.
// Every call must be paired with EndSnapshot so a lease is never left
// stuck.
func (s *Store) BeginSnapshot(ctx context.Context, id, orgID, ownerID, backendID string) (*Lease, error) {
	l, err := s.lookup(ctx, id, orgID, ownerID, backendID)
	if err != nil {
		return nil, err
	}
	if l.SpaceID == "" {
		return l, nil
	}
	if l.State == mpv1.SandboxLifecycleState_SCRATCH {
		return nil, ErrLeaseNotActivated
	}
	_, err = s.pool.Exec(ctx, `UPDATE leases SET state = $2 WHERE id = $1`,
		id, int32(mpv1.SandboxLifecycleState_SNAPSHOTTING))
	if err != nil {
		return nil, fmt.Errorf("begin snapshot: %w", err)
	}
	l.State = mpv1.SandboxLifecycleState_SNAPSHOTTING
	return l, nil
}

// EndSnapshot returns a Space-scoped lease to ACTIVE after a snapshot
// attempt, regardless of whether it succeeded — callers invoke this via
// defer immediately after a successful BeginSnapshot so a lease is never
// left stuck in SNAPSHOTTING. A no-op for a non-Space lease or one no
// longer present (e.g. released concurrently) — the UPDATE simply matches
// zero rows.
func (s *Store) EndSnapshot(ctx context.Context, id string) {
	_, _ = s.pool.Exec(ctx, `
		UPDATE leases SET state = $2 WHERE id = $1 AND space_id <> '' AND state = $3
	`, id, int32(mpv1.SandboxLifecycleState_ACTIVE), int32(mpv1.SandboxLifecycleState_SNAPSHOTTING))
}

// ReleaseScoped marks a lease DESTROYED rather than deleting it outright.
// Keeping the row — instead of removing it — means a second release of the
// same lease, or one racing a concurrent release, sees a clean idempotent
// success instead of a confusing ErrLeaseNotFound. Deliberately does not
// use lookup: unlike every other operation, a lease already DESTROYED must
// still be found here, not treated as absent.
func (s *Store) ReleaseScoped(ctx context.Context, id, orgID, ownerID, backendID string) (bool, error) {
	row := s.pool.QueryRow(ctx, `
		SELECT id, scope_id, scope_type, org_id, owner_id, endpoint, space_id, backend_id, state, expires_at, created_at
		FROM leases
		WHERE id = $1 AND org_id = $2 AND ($3 = '' OR owner_id = $3)
	`, id, orgID, ownerID)
	l, err := scanLease(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, ErrLeaseNotFound
	}
	if err != nil {
		return false, fmt.Errorf("release lease: %w", err)
	}
	if l.BackendID != backendID {
		return false, ErrLeaseBackendMismatch
	}
	if _, err := s.pool.Exec(ctx, `UPDATE leases SET state = $2 WHERE id = $1`,
		id, int32(mpv1.SandboxLifecycleState_DESTROYED)); err != nil {
		return false, fmt.Errorf("release lease: %w", err)
	}
	return true, nil
}

func scanLease(row pgx.Row) (*Lease, error) {
	var l Lease
	var state int32
	if err := row.Scan(&l.ID, &l.ScopeID, &l.ScopeType, &l.OrgID, &l.OwnerID, &l.Endpoint,
		&l.SpaceID, &l.BackendID, &state, &l.ExpiresAt, &l.CreatedAt); err != nil {
		return nil, err
	}
	l.State = mpv1.SandboxLifecycleState(state)
	return &l, nil
}

func (s *Store) newID() (string, error) {
	var buf [16]byte
	if _, err := s.randFn(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}
