package server

import (
	"context"
	"time"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
)

// LeaseStore is the narrow surface Server actually calls — exactly
// *lease.Store's methods this package uses, nothing more (GetScoped is
// real and tested on *lease.Store itself but has no caller here). Letting
// Server depend on this interface rather than the concrete *lease.Store
// means its own tests can inject a fast in-memory fake instead of requiring
// a real Postgres pool for every test — the same "seam at the consumer,
// not a redundant abstraction on the store" shape capability-core uses for
// its own Postgres-backed ScopeStore (policy.WithScopeResolver takes an
// interface, not *registry.ScopeStore directly).
type LeaseStore interface {
	Create(ctx context.Context, scopeID, scopeType, orgID, ownerID, spaceID, backendID string, ttl time.Duration) (*lease.Lease, error)
	Activate(ctx context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error)
	BeginSnapshot(ctx context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error)
	EndSnapshot(ctx context.Context, id string)
	ReleaseScoped(ctx context.Context, id, orgID, ownerID, backendID string) (bool, error)
}

// SnapshotStore is the narrow surface Server actually calls.
type SnapshotStore interface {
	Create(ctx context.Context, l *lease.Lease, label string) (*snapshot.Snapshot, error)
}
