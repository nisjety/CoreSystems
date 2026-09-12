package server

import (
	"context"
	"time"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
)

// LeaseStore is the narrow surface Server actually calls — exactly
// *lease.Store's methods this package uses. Letting Server depend on this
// interface rather than the concrete *lease.Store means its own tests can
// inject a fast in-memory fake instead of requiring a real Postgres pool
// for every test — the same "seam at the consumer, not a redundant
// abstraction on the store" shape capability-core uses for its own
// Postgres-backed ScopeStore (policy.WithScopeResolver takes an interface,
// not *registry.ScopeStore directly).
//
// GetScoped joined this interface for 3.5.C's GetWorkspaceManifest handler
// (design doc §8 item 3.5.C) — it needs to resolve a lease's org_id/space_id
// from a caller-supplied lease_id/backend_id, the same lookup+validate shape
// every other method here already uses. It was real and tested on
// *lease.Store itself well before this, just unused by Server until now.
type LeaseStore interface {
	Create(ctx context.Context, scopeID, scopeType, orgID, ownerID, spaceID, backendID string, ttl time.Duration) (*lease.Lease, error)
	Activate(ctx context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error)
	BeginSnapshot(ctx context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error)
	EndSnapshot(ctx context.Context, id string)
	ReleaseScoped(ctx context.Context, id, orgID, ownerID, backendID string) (bool, error)
	GetScoped(ctx context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error)
	// GetAny joined this interface for step 4's PromoteWorkspace handler
	// (design doc §8 item 4) — unlike GetScoped, it must still resolve a
	// lease already marked DESTROYED, since merging a run's overlay is
	// deliberately never tied to (or blocked by) the lease's own release.
	GetAny(ctx context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error)
}

// SnapshotStore is the narrow surface Server actually calls.
type SnapshotStore interface {
	Create(ctx context.Context, l *lease.Lease, label string) (*snapshot.Snapshot, error)
}

// WorkspaceStore is the narrow surface Server actually calls — exactly
// *workspace.Store's methods, the same seam-at-the-consumer shape as
// LeaseStore/SnapshotStore above. Backs GetWorkspaceManifest and
// SnapshotSandbox's overlay upsert (design doc §8 item 3.5.C) and
// PromoteWorkspace's merge (§8 item 4).
type WorkspaceStore interface {
	GetManifest(ctx context.Context, orgID, spaceID, runID string) ([]workspace.ManifestEntry, error)
	UpsertOverlay(ctx context.Context, orgID, spaceID, runID string, files []workspace.ChangedFile) error
	// Promote merges runID's own overlay into the Space's durable rows,
	// per path, and returns the paths that conflicted (every other path
	// still merged).
	Promote(ctx context.Context, orgID, spaceID, runID string) ([]string, error)
}
