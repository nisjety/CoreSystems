package server

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
)

// MemoryLeaseStore and MemorySnapshotStore are the pre-S3.3 in-memory
// implementations of LeaseStore/SnapshotStore, kept for two real,
// deliberate consumers rather than deleted outright: cmd/main.go's
// ephemeral-development fallback (SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT=true
// with no DATABASE_URL configured — mirroring cost-core's own established
// "runs against its in-memory ledger" precedent for exactly this case), and
// this package's own fast unit tests (server_test.go), which should not
// need a real Postgres pool to exercise Server's request-handling logic.
// The durable, production implementations are lease.Store/snapshot.Store
// (Postgres-backed, migrations 0001-0002).
//
// Ctx is accepted on every method for interface compliance but never used
// internally — a map lookup has no meaningful cancellation/deadline to
// respect.

// MemoryLeaseStore is an in-memory LeaseStore.
type MemoryLeaseStore struct {
	mu     sync.RWMutex
	byID   map[string]*lease.Lease
	nowFn  func() time.Time
	randFn func([]byte) (int, error)
}

// NewMemoryLeaseStore constructs an empty MemoryLeaseStore.
func NewMemoryLeaseStore() *MemoryLeaseStore {
	return &MemoryLeaseStore{
		byID:   make(map[string]*lease.Lease),
		nowFn:  time.Now,
		randFn: rand.Read,
	}
}

func (s *MemoryLeaseStore) Create(_ context.Context, scopeID, scopeType, orgID, ownerID, spaceID, backendID string, ttl time.Duration) (*lease.Lease, error) {
	id, err := s.newID()
	if err != nil {
		return nil, err
	}
	now := s.nowFn()
	l := &lease.Lease{
		ID: id, ScopeID: scopeID, ScopeType: scopeType, OrgID: orgID, OwnerID: ownerID,
		Endpoint: "sandbox://" + id, SpaceID: spaceID, BackendID: backendID,
		State: mpv1.SandboxLifecycleState_SCRATCH, ExpiresAt: now.Add(ttl), CreatedAt: now,
	}
	s.mu.Lock()
	s.byID[id] = cloneLease(l)
	s.mu.Unlock()
	return cloneLease(l), nil
}

func (s *MemoryLeaseStore) lookup(id, orgID, ownerID, backendID string) (*lease.Lease, error) {
	l, ok := s.byID[id]
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) || l.State == mpv1.SandboxLifecycleState_DESTROYED {
		return nil, lease.ErrLeaseNotFound
	}
	if l.IsExpired(s.nowFn()) {
		return nil, lease.ErrLeaseExpired
	}
	if l.BackendID != backendID {
		return nil, lease.ErrLeaseBackendMismatch
	}
	return l, nil
}

// GetScoped joined the LeaseStore interface for 3.5.C's GetWorkspaceManifest
// handler (design doc §8 item 3.5.C); this in-memory implementation mirrors
// lease.Store's own GetScoped exactly.
func (s *MemoryLeaseStore) GetScoped(_ context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	l, err := s.lookup(id, orgID, ownerID, backendID)
	if err != nil {
		return nil, err
	}
	return cloneLease(l), nil
}

func (s *MemoryLeaseStore) Activate(_ context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	l, err := s.lookup(id, orgID, ownerID, backendID)
	if err != nil {
		return nil, err
	}
	if l.State == mpv1.SandboxLifecycleState_SCRATCH {
		l.State = mpv1.SandboxLifecycleState_ACTIVE
	}
	return cloneLease(l), nil
}

func (s *MemoryLeaseStore) BeginSnapshot(_ context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	l, err := s.lookup(id, orgID, ownerID, backendID)
	if err != nil {
		return nil, err
	}
	if l.SpaceID != "" {
		if l.State == mpv1.SandboxLifecycleState_SCRATCH {
			return nil, lease.ErrLeaseNotActivated
		}
		l.State = mpv1.SandboxLifecycleState_SNAPSHOTTING
	}
	return cloneLease(l), nil
}

func (s *MemoryLeaseStore) EndSnapshot(_ context.Context, id string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if l, ok := s.byID[id]; ok && l.SpaceID != "" {
		l.State = mpv1.SandboxLifecycleState_ACTIVE
	}
}

func (s *MemoryLeaseStore) ReleaseScoped(_ context.Context, id, orgID, ownerID, backendID string) (bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	l, ok := s.byID[id]
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) {
		return false, lease.ErrLeaseNotFound
	}
	if l.BackendID != backendID {
		return false, lease.ErrLeaseBackendMismatch
	}
	l.State = mpv1.SandboxLifecycleState_DESTROYED
	return true, nil
}

// GetAny mirrors lease.Store's own GetAny: resolves id within (orgID,
// ownerID, backendID) without excluding a DESTROYED lease or rejecting an
// expired one — PromoteWorkspace's own consumer, resolving which Space a
// run's overlay belongs to even after the lease itself has been released.
func (s *MemoryLeaseStore) GetAny(_ context.Context, id, orgID, ownerID, backendID string) (*lease.Lease, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	l, ok := s.byID[id]
	if !ok || l.OrgID != orgID || (ownerID != "" && l.OwnerID != ownerID) {
		return nil, lease.ErrLeaseNotFound
	}
	if l.BackendID != backendID {
		return nil, lease.ErrLeaseBackendMismatch
	}
	return cloneLease(l), nil
}

func cloneLease(value *lease.Lease) *lease.Lease {
	copy := *value
	return &copy
}

func (s *MemoryLeaseStore) newID() (string, error) {
	var buf [16]byte
	if _, err := s.randFn(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}

// MemorySnapshotStore is an in-memory SnapshotStore.
type MemorySnapshotStore struct {
	mu     sync.RWMutex
	byID   map[string]*snapshot.Snapshot
	nowFn  func() time.Time
	randFn func([]byte) (int, error)
}

// NewMemorySnapshotStore constructs an empty MemorySnapshotStore.
func NewMemorySnapshotStore() *MemorySnapshotStore {
	return &MemorySnapshotStore{
		byID:   make(map[string]*snapshot.Snapshot),
		nowFn:  time.Now,
		randFn: rand.Read,
	}
}

func (s *MemorySnapshotStore) Create(_ context.Context, l *lease.Lease, label string) (*snapshot.Snapshot, error) {
	if l == nil || l.ID == "" {
		return nil, snapshot.ErrInvalidLease
	}
	id, err := s.newID()
	if err != nil {
		return nil, err
	}
	snap := &snapshot.Snapshot{
		ID: id, LeaseID: l.ID, Label: label,
		ObjectKey: "snapshots/" + l.ID + "/" + id, CreatedAt: s.nowFn(),
	}
	s.mu.Lock()
	s.byID[id] = snap
	s.mu.Unlock()
	return snap, nil
}

// Get returns a snapshot by ID, or snapshot.ErrSnapshotNotFound. Kept for
// direct test use, mirroring snapshot.Store's own public surface.
func (s *MemorySnapshotStore) Get(_ context.Context, id string) (*snapshot.Snapshot, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	snap, ok := s.byID[id]
	if !ok {
		return nil, snapshot.ErrSnapshotNotFound
	}
	return snap, nil
}

func (s *MemorySnapshotStore) newID() (string, error) {
	var buf [16]byte
	if _, err := s.randFn(buf[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(buf[:]), nil
}

// MemoryWorkspaceStore is an in-memory WorkspaceStore, the same
// pre-Postgres-durability convenience MemoryLeaseStore/MemorySnapshotStore
// already provide: cmd/main.go's ephemeral-development fallback and this
// package's own fast unit tests (server_test.go). The durable, production
// implementation is workspace.Store (Postgres-backed, migration 0001).
type MemoryWorkspaceStore struct {
	mu   sync.RWMutex
	rows map[string]workspace.ChangedFile // key: org_id + "/" + space_id + "/" + coalesce(run_id, "") + "/" + path
}

// NewMemoryWorkspaceStore constructs an empty MemoryWorkspaceStore.
func NewMemoryWorkspaceStore() *MemoryWorkspaceStore {
	return &MemoryWorkspaceStore{rows: make(map[string]workspace.ChangedFile)}
}

func workspaceRowKey(orgID, spaceID, runID, path string) string {
	return orgID + "/" + spaceID + "/" + runID + "/" + path
}

// GetManifest mirrors workspace.Store.GetManifest's layered view: this
// runID's own overlay rows shadow the Space's rows (runID == "") for the
// same path.
func (s *MemoryWorkspaceStore) GetManifest(_ context.Context, orgID, spaceID, runID string) ([]workspace.ManifestEntry, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()

	prefix := orgID + "/" + spaceID + "/"
	spaceRows := make(map[string]workspace.ManifestEntry)
	overlayRows := make(map[string]workspace.ManifestEntry)
	for key, row := range s.rows {
		rest, ok := strings.CutPrefix(key, prefix)
		if !ok {
			continue
		}
		parts := strings.SplitN(rest, "/", 2)
		if len(parts) != 2 {
			continue
		}
		rowRunID, path := parts[0], parts[1]
		entry := workspace.ManifestEntry{Path: path, ContentHash: row.ContentHash}
		switch rowRunID {
		case "":
			spaceRows[path] = entry
		case runID:
			overlayRows[path] = entry
		}
	}

	merged := spaceRows
	for path, entry := range overlayRows {
		merged[path] = entry // this run's overlay shadows the Space row for the same path
	}
	entries := make([]workspace.ManifestEntry, 0, len(merged))
	for _, e := range merged {
		entries = append(entries, e)
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Path < entries[j].Path })
	return entries, nil
}

// UpsertOverlay mirrors workspace.Store.UpsertOverlay: one upsert per file
// under this runID's own overlay key.
func (s *MemoryWorkspaceStore) UpsertOverlay(_ context.Context, orgID, spaceID, runID string, files []workspace.ChangedFile) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, f := range files {
		s.rows[workspaceRowKey(orgID, spaceID, runID, f.Path)] = f
	}
	return nil
}

// Promote mirrors workspace.Store.Promote's own compare-and-swap semantics:
// a path merges into the Space row (run_id == "") when that row doesn't
// exist yet, its content matches the overlay row's base_hash (first-time
// merge), or already matches the overlay row's own content_hash (an
// already-merged path — a second Promote call is a safe no-op, not a false
// conflict). Every other path still merges independently of a conflict
// elsewhere in the same overlay.
func (s *MemoryWorkspaceStore) Promote(_ context.Context, orgID, spaceID, runID string) ([]string, error) {
	if orgID == "" || spaceID == "" || runID == "" {
		return nil, fmt.Errorf("org_id, space_id, and run_id are required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()

	overlayPrefix := orgID + "/" + spaceID + "/" + runID + "/"
	type overlayEntry struct {
		path string
		file workspace.ChangedFile
	}
	var overlay []overlayEntry
	for key, row := range s.rows {
		if path, ok := strings.CutPrefix(key, overlayPrefix); ok {
			overlay = append(overlay, overlayEntry{path: path, file: row})
		}
	}
	sort.Slice(overlay, func(i, j int) bool { return overlay[i].path < overlay[j].path })

	var conflicts []string
	for _, entry := range overlay {
		spaceKey := workspaceRowKey(orgID, spaceID, "", entry.path)
		existing, hasExisting := s.rows[spaceKey]
		mergeable := !hasExisting ||
			existing.ContentHash == entry.file.BaseHash ||
			existing.ContentHash == entry.file.ContentHash
		if mergeable {
			s.rows[spaceKey] = entry.file
		} else {
			conflicts = append(conflicts, entry.path)
		}
	}
	return conflicts, nil
}
