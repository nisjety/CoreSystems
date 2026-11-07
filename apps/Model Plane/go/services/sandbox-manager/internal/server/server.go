package server

import (
	"context"
	"errors"
	"strings"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/objectstore"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/telemetry"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// Server implements SandboxManagerServer over LeaseStore/SnapshotStore/
// WorkspaceStore — the real Postgres-backed implementations in production
// (lease.Store/snapshot.Store/workspace.Store), a fast in-memory fake in
// this package's own tests (see server_test.go).
type Server struct {
	mpv1.UnimplementedSandboxManagerServer
	leases    LeaseStore
	snapshots SnapshotStore
	workspace WorkspaceStore
	principal func(context.Context) (authctx.Principal, error)
	// capabilityVerify verifies a Space capability decision + its unsigned
	// claims sidecar, mirroring authz.SpaceCapabilityVerifier.Verify. A
	// func field, not the concrete type, so tests can inject a fake without
	// a real Ed25519 keypair — the same pattern as principal above. Nil
	// means "not configured": any AcquireLease request naming a space_id is
	// then refused, never served unverified.
	capabilityVerify func(token, claimsJSON string, expect authz.CapabilityExpectation) (authz.VerifiedCapability, error)
	// spaceReadVerify verifies a Control model.thread.read decision addressed
	// to this service, admitting a HUMAN to a Space's processes (S4.2 §7). A
	// func field for the same reason as capabilityVerify: tests inject a fake
	// rather than an Ed25519 keypair. Nil means "not configured", and a user
	// identity is then refused rather than served unverified.
	spaceReadVerify func(token string, expect authz.SpaceReadExpectation) (authz.VerifiedSpaceRead, error)
	// backendID is this instance's own SANDBOX_MANAGER_BACKEND_ID. Empty
	// only when capabilityVerify is also nil (an unconfigured instance).
	backendID string
	// processes is the S4.2 background process registry. Nil means this
	// instance has none (no durable database), in which case every process
	// RPC is refused rather than served against state a restart discards.
	processes ProcessStore
	// threadWorkspaceEnabled is SANDBOX_MANAGER_THREAD_WORKSPACE. Off, a thread
	// lease is acquired exactly as before and every thread-workspace RPC is
	// refused.
	threadWorkspaceEnabled bool
	// threads is the durable per-thread workspace. Nil in ephemeral development,
	// where the thread RPCs refuse rather than serve state a restart discards.
	threads      ThreadWorkspaceStore
	threadLimits threadworkspace.Limits
	// threadErase deletes a thread's rows and threadBlobs its blobs (WS22-S5).
	// Both stay nil until WithThreadErasure wires them, and DeleteThreadWorkspace
	// refuses before it removes anything while either is nil.
	threadErase ThreadEraser
	threadBlobs threadworkspace.BlobEraser
	// egressTiersEnabled is SANDBOX_MANAGER_EGRESS_TIERS (EP-9 E3). Off, every
	// lease records egress off, the response leaves egress_tier unset, and an
	// egress-capable claim the decision does not grant is refused exactly as
	// before. On, that claim gets its lease at off instead (D-E2) and the
	// granted tier is recorded on the lease and reported back.
	egressTiersEnabled bool
}

// NewServer constructs a Server with the given stores. Space capability
// verification starts disabled; call WithCapabilityVerifier to enable it.
func NewServer(leases LeaseStore, snaps SnapshotStore, workspace WorkspaceStore) *Server {
	return &Server{leases: leases, snapshots: snaps, workspace: workspace, principal: authz.Principal}
}

// WithCapabilityVerifier wires Space capability-decision verification and
// this instance's own backend id into AcquireLease.
func (s *Server) WithCapabilityVerifier(verify func(token, claimsJSON string, expect authz.CapabilityExpectation) (authz.VerifiedCapability, error), backendID string) *Server {
	s.capabilityVerify = verify
	s.backendID = backendID
	return s
}

// WithSpaceReadVerifier wires human read authority for the process registry.
// Without it a user identity cannot read processes at all, which is the
// correct fail-closed state — it is not the same as reading without a ceiling.
func (s *Server) WithSpaceReadVerifier(verify func(token string, expect authz.SpaceReadExpectation) (authz.VerifiedSpaceRead, error)) *Server {
	s.spaceReadVerify = verify
	return s
}

// WithProcessStore wires the S4.2 background process registry. Without it
// every process RPC fails closed; see the processes field.
func (s *Server) WithProcessStore(processes ProcessStore) *Server {
	s.processes = processes
	return s
}

// WithThreadWorkspace wires the per-thread workspace (WS22-S1). enabled is the
// SANDBOX_MANAGER_THREAD_WORKSPACE switch; a nil store means no durable database.
func (s *Server) WithThreadWorkspace(enabled bool, store ThreadWorkspaceStore, limits threadworkspace.Limits) *Server {
	s.threadWorkspaceEnabled = enabled
	s.threads = store
	s.threadLimits = limits
	return s
}

// WithThreadErasure wires the erase path (WS22-S5): the rows eraser and the blob
// store that holds a thread's bytes. Without both, DeleteThreadWorkspace refuses
// before it removes anything.
func (s *Server) WithThreadErasure(rows ThreadEraser, blobs threadworkspace.BlobEraser) *Server {
	s.threadErase = rows
	s.threadBlobs = blobs
	return s
}

// WithEgressTiers wires the SANDBOX_MANAGER_EGRESS_TIERS switch (EP-9 E3). See
// the egressTiersEnabled field for what it changes.
func (s *Server) WithEgressTiers(enabled bool) *Server {
	s.egressTiersEnabled = enabled
	return s
}

// egressTierProto maps a recorded tier word onto the wire enum. A word this
// build does not know reads as OFF, never as a wider reach.
func egressTierProto(tier string) mpv1.SandboxEgressTier {
	switch tier {
	case authz.EgressTierRegistries:
		return mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_REGISTRIES
	case authz.EgressTierAllowlist:
		return mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_ALLOWLIST
	default:
		return mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_OFF
	}
}

// permittedEgressTier is the tier a lease records for a verified decision.
// 'all' is not storable in Wave 3 (D-E3): the verifier never grants it today,
// and a decision that did is recorded as off rather than as the widest reach.
func permittedEgressTier(enabled bool, granted string) string {
	if !enabled {
		return authz.EgressTierOff
	}
	switch granted {
	case authz.EgressTierRegistries, authz.EgressTierAllowlist:
		return granted
	default:
		return authz.EgressTierOff
	}
}

// AcquireLease creates a new lease for the requested scope.
func (s *Server) AcquireLease(ctx context.Context, req *AcquireLeaseRequest) (*AcquireLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "AcquireLease")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetScopeId() == "" {
		return nil, status.Error(codes.InvalidArgument, "scope_id is required")
	}
	if req.GetScopeType() == "" {
		return nil, status.Error(codes.InvalidArgument, "scope_type is required")
	}
	if req.GetScopeType() != "thread" && req.GetScopeType() != "agent" {
		return nil, status.Error(codes.InvalidArgument, "scope_type must be thread or agent")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	ttl := time.Duration(0)
	if req.GetTtl() != nil {
		ttl = req.GetTtl().AsDuration()
	}
	if ttl <= 0 {
		return nil, status.Error(codes.InvalidArgument, "ttl must be greater than zero")
	}

	threadScope := req.GetScopeType() == "thread"
	if threadScope && req.GetZdr() {
		// Refused whatever the switch says: a ZDR turn keeps no thread files
		// (DEC-23 D8), so nothing is written and nothing is hydrated.
		return nil, status.Error(codes.FailedPrecondition, "thread workspace is refused under zero data retention")
	}
	if threadScope && s.threadWorkspaceEnabled {
		return s.acquireThread(ctx, principal, req, ttl)
	}

	spaceID := strings.TrimSpace(req.GetSpaceId())
	backendID := ""
	processesPermitted := false
	egressTier := authz.EgressTierOff
	var grant lease.SpaceGrant
	if spaceID != "" {
		if s.capabilityVerify == nil {
			return nil, status.Error(codes.FailedPrecondition, "Space capability verification is not configured")
		}
		verified, err := s.capabilityVerify(req.GetCapabilityDecision(), req.GetCapabilityClaimsJson(), authz.CapabilityExpectation{
			OrgID: principal.OrganizationID, SpaceRef: spaceID, SubjectID: principal.ActorID, Now: time.Now().UTC(),
			// D-E2: with the switch on, an egress-capable claim the decision
			// does not grant is verified at off instead of refused, as an
			// ungranted process claim already is. execution-core enforces
			// the recorded tier at its per-call boundary.
			GrantEgressAtOff: s.egressTiersEnabled,
		})
		if err != nil {
			telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "capability_denied")))
			return nil, status.Error(codes.PermissionDenied, err.Error())
		}
		if verified.Claims.BackendID != s.backendID {
			telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "backend_mismatch")))
			return nil, mapErr(lease.ErrLeaseBackendMismatch)
		}
		backendID = verified.Claims.BackendID
		// Decided once, here, and persisted on the lease: a process RPC
		// arrives later on execution-core's own service token with no
		// decision in hand, so the lease row is the only thing it can check.
		// Deliberately NOT a refusal when the backend claims the capability
		// but the Space is not entitled — unlike egress, a process-capable
		// backend is still a perfectly good one-shot substrate, so the lease
		// is granted and only the process RPCs are refused.
		processesPermitted = verified.AllowsBackgroundProcesses()
		// The audience the decision was signed under. Captured here for the
		// same reason as the line above — this is the last point at which a
		// Control decision is in hand — and it becomes the ceiling the human
		// read path applies to every process this lease admits (0004).
		grant.RecipientAudienceRevision = verified.RecipientAudienceRevision
		// Recorded for the same reason as processesPermitted: nothing later
		// holds the decision. Hosts are never recorded, only the tier word.
		egressTier = permittedEgressTier(s.egressTiersEnabled, verified.EgressTier)
	}
	grant.ProcessesPermitted = processesPermitted
	grant.EgressTierPermitted = egressTier

	l, err := s.leases.Create(ctx, req.GetScopeId(), req.GetScopeType(), principal.OrganizationID, principal.ActorID, spaceID, backendID, grant, ttl)
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	// Unset while the switch is off, so the response is unchanged; read back
	// off the created lease otherwise, like processes_permitted.
	responseEgressTier := mpv1.SandboxEgressTier_SANDBOX_EGRESS_TIER_UNSPECIFIED
	if s.egressTiersEnabled {
		responseEgressTier = egressTierProto(l.EgressTierPermitted)
	}
	return &AcquireLeaseResponse{
		LeaseId:   l.ID,
		Endpoint:  l.Endpoint,
		ExpiresAt: timestamppb.New(l.ExpiresAt),
		BackendId: l.BackendID,
		State:     l.State,
		// Read back off the created lease, which is the same value every
		// later process RPC checks: false for a non-Space lease (the local
		// above is only ever set inside the Space branch), and otherwise
		// whatever the verified decision allowed. Telling the host this here
		// lets it refuse `process_start` with the real reason instead of
		// preparing a spawn `RegisterProcess` would deny; that denial remains
		// the authoritative one.
		ProcessesPermitted: l.ProcessesPermitted,
		EgressTier:         responseEgressTier,
	}, nil
}

// threadWorkspaceErr maps an error from the thread lease or the thread store.
// A quota or a malformed batch is the caller's fault and is reported as such,
// and an unowned thread lease is refused as an authority answer.
func threadWorkspaceErr(err error) error {
	switch {
	case errors.Is(err, lease.ErrThreadLeaseOwnerRequired):
		return status.Error(codes.PermissionDenied, err.Error())
	case errors.Is(err, threadworkspace.ErrQuotaExceeded):
		return status.Error(codes.ResourceExhausted, err.Error())
	case errors.Is(err, threadworkspace.ErrInvalidChange):
		return status.Error(codes.InvalidArgument, err.Error())
	default:
		return mapErr(err)
	}
}

// acquireThread is AcquireLease for a thread scope with the switch on. It is
// the only way a thread lease is minted: idempotent per user and thread, never
// Space-scoped, and refused for a service principal.
func (s *Server) acquireThread(ctx context.Context, principal authctx.Principal, req *AcquireLeaseRequest, ttl time.Duration) (*AcquireLeaseResponse, error) {
	if principal.PrincipalType != "user" {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "policy_denied")))
		return nil, status.Error(codes.PermissionDenied, "a thread lease belongs to a user, not a service principal")
	}
	if strings.TrimSpace(req.GetSpaceId()) != "" {
		return nil, status.Error(codes.InvalidArgument, "a thread lease cannot name a Space")
	}
	if s.threads == nil {
		return nil, status.Error(codes.FailedPrecondition, "thread workspace requires a durable database")
	}
	l, err := s.leases.AcquireThread(ctx, req.GetScopeId(), principal.OrganizationID, principal.ActorID, s.backendID, ttl)
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, threadWorkspaceErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "thread_acquired")))
	return &AcquireLeaseResponse{
		LeaseId:   l.ID,
		Endpoint:  l.Endpoint,
		ExpiresAt: timestamppb.New(l.ExpiresAt),
		BackendId: l.BackendID,
		State:     l.State,
	}, nil
}

// ReleaseLease releases an existing lease.
func (s *Server) ReleaseLease(ctx context.Context, req *ReleaseLeaseRequest) (*ReleaseLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ReleaseLease")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	ok, err := s.leases.ReleaseScoped(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	// A released lease must leave no row claiming to still be running. This
	// is the registry half only — the host does the actual killing on its
	// own release path — so a failure here is logged as an outcome and never
	// fails the release itself: the lease IS gone, and the sweeper will
	// catch any row this missed once its host stops heartbeating.
	if s.processes != nil {
		if killed, killErr := s.processes.KillForLease(ctx, principal.OrganizationID, req.GetLeaseId()); killErr != nil {
			processDecision(ctx, processOutcome(killErr))
		} else if killed > 0 {
			processDecision(ctx, "killed_with_lease")
		}
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "released")))
	return &ReleaseLeaseResponse{Released: ok}, nil
}

// SnapshotSandbox persists a snapshot reference for the given lease.
func (s *Server) SnapshotSandbox(ctx context.Context, req *SnapshotRequest) (*SnapshotResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "SnapshotSandbox")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	if req.GetLabel() == "" {
		return nil, status.Error(codes.InvalidArgument, "label is required")
	}
	l, err := s.leases.BeginSnapshot(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", snapshotLeaseOutcome(err))))
		return nil, mapErr(err)
	}
	defer s.leases.EndSnapshot(ctx, l.ID)
	// Record this run's overlay BEFORE creating the snapshot record — fail
	// closed rather than risk a snapshot referencing content whose overlay
	// row was never durably written. Upserting is idempotent, so if
	// snapshots.Create itself fails next, leaving these rows in place is
	// safe (design doc §8 item 3.5.C).
	if l.SpaceID != "" && len(req.GetChangedFiles()) > 0 {
		files := make([]workspace.ChangedFile, 0, len(req.GetChangedFiles()))
		for _, f := range req.GetChangedFiles() {
			files = append(files, workspace.ChangedFile{
				Path:        f.GetPath(),
				ContentHash: f.GetContentHash(),
				SizeBytes:   f.GetSizeBytes(),
				BaseHash:    f.GetBaseHash(),
			})
		}
		if err := s.workspace.UpsertOverlay(ctx, principal.OrganizationID, l.SpaceID, l.ID, files); err != nil {
			telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "workspace_overlay_failed")))
			return nil, status.Errorf(codes.Internal, "record workspace overlay: %v", err)
		}
	}
	sn, err := s.snapshots.Create(ctx, l, req.GetLabel())
	if err != nil {
		telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", snapshotCreateOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.SnapshotDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "created")))
	return &SnapshotResponse{
		SnapshotId: sn.ID,
		ObjectKey:  sn.ObjectKey,
	}, nil
}

// ActivateLease promotes a Space-scoped lease from SCRATCH to ACTIVE — the
// first time a caller needs more than the credential-free scratch
// allowlist (e.g. real backend network/egress permission per the
// capability decision's granted Permissions). A no-op returning the
// lease's current state if it is already ACTIVE or SNAPSHOTTING.
func (s *Server) ActivateLease(ctx context.Context, req *ActivateLeaseRequest) (*ActivateLeaseResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ActivateLease")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	l, err := s.leases.Activate(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", leaseOutcome(err))))
		return nil, mapErr(err)
	}
	telemetry.LeaseDecisionsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("outcome", "activated")))
	return &ActivateLeaseResponse{State: l.State}, nil
}

// GetWorkspaceManifest resolves the layered workspace manifest a
// Space-scoped lease sees: the Space's own durable files shadowed
// path-for-path by this lease's own not-yet-merged overlay. Returns an
// empty entries list for a non-Space lease — the same exemption
// BeginSnapshot already grants that case.
func (s *Server) GetWorkspaceManifest(ctx context.Context, req *GetWorkspaceManifestRequest) (*GetWorkspaceManifestResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetWorkspaceManifest")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	l, err := s.leases.GetScoped(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		return nil, mapErr(err)
	}
	if l.SpaceID == "" {
		return &GetWorkspaceManifestResponse{}, nil
	}
	entries, err := s.workspace.GetManifest(ctx, principal.OrganizationID, l.SpaceID, l.ID)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "get workspace manifest: %v", err)
	}
	out := make([]*WorkspaceManifestEntry, 0, len(entries))
	for _, e := range entries {
		out = append(out, &WorkspaceManifestEntry{Path: e.Path, ContentHash: e.ContentHash})
	}
	return &GetWorkspaceManifestResponse{Entries: out}, nil
}

// PromoteWorkspace merges a Space-scoped lease's own workspace_files
// overlay into the Space's durable rows — S3.3 step 4 (design doc §4, §8
// item 4). Deliberately its own explicit step: not called from
// SnapshotSandbox or ReleaseLease, and resolves the lease via GetAny (not
// GetScoped) precisely so it keeps working after ReleaseLease has already
// marked the lease DESTROYED — merging must not race losing against, or
// depend on ordering before, the lease's own automatic release.
func (s *Server) PromoteWorkspace(ctx context.Context, req *PromoteWorkspaceRequest) (*PromoteWorkspaceResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "PromoteWorkspace")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	l, err := s.leases.GetAny(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		return nil, mapErr(err)
	}
	if l.SpaceID == "" {
		return nil, status.Error(codes.FailedPrecondition, "lease is not Space-scoped; nothing to promote")
	}
	conflicts, err := s.workspace.Promote(ctx, principal.OrganizationID, l.SpaceID, l.ID)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "promote workspace: %v", err)
	}
	return &PromoteWorkspaceResponse{ConflictingPaths: conflicts}, nil
}

// threadLease resolves the lease a thread-workspace RPC names. It refuses while
// the switch is off or no durable store is wired, then requires a user caller
// and a thread lease. A Space lease is refused here, so no thread RPC can reach
// a Space's rows.
func (s *Server) threadLease(ctx context.Context, principal authctx.Principal, leaseID, backendID string) (*lease.Lease, error) {
	if !s.threadWorkspaceEnabled || s.threads == nil {
		return nil, status.Error(codes.FailedPrecondition, "thread workspace is not enabled")
	}
	if leaseID == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	if principal.PrincipalType != "user" {
		return nil, status.Error(codes.PermissionDenied, "a thread workspace belongs to a user, not a service principal")
	}
	l, err := s.leases.GetScoped(ctx, leaseID, principal.OrganizationID, authz.OwnerFilter(principal), backendID)
	if err != nil {
		return nil, mapErr(err)
	}
	if l.ScopeType != "thread" || l.SpaceID != "" {
		return nil, status.Error(codes.FailedPrecondition, "lease is not a thread lease")
	}
	return l, nil
}

// GetThreadWorkspace reads the files the thread behind a thread lease holds.
func (s *Server) GetThreadWorkspace(ctx context.Context, req *GetThreadWorkspaceRequest) (*GetThreadWorkspaceResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetThreadWorkspace")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	l, err := s.threadLease(ctx, principal, req.GetLeaseId(), req.GetBackendId())
	if err != nil {
		return nil, err
	}
	snap, err := s.threads.Get(ctx, principal.OrganizationID, l.OwnerID, l.ScopeID)
	if err != nil {
		return nil, status.Errorf(codes.Internal, "get thread workspace: %v", err)
	}
	entries := make([]*ThreadWorkspaceEntry, 0, len(snap.Files))
	for _, f := range snap.Files {
		entries = append(entries, &ThreadWorkspaceEntry{Path: f.Path, ContentHash: f.ContentHash, SizeBytes: f.SizeBytes})
	}
	return &GetThreadWorkspaceResponse{Entries: entries, Revision: snap.Revision}, nil
}

// ApplyThreadWorkspaceChanges applies a batch through a thread lease. Each path
// is compare-and-swapped on its base_hash: a conflicting path is reported and
// never overwritten, and every other path in the batch still applies.
func (s *Server) ApplyThreadWorkspaceChanges(ctx context.Context, req *ApplyThreadWorkspaceChangesRequest) (*ApplyThreadWorkspaceChangesResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "ApplyThreadWorkspaceChanges")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	l, err := s.threadLease(ctx, principal, req.GetLeaseId(), req.GetBackendId())
	if err != nil {
		return nil, err
	}
	changes := make([]threadworkspace.Change, 0, len(req.GetChanges()))
	for _, c := range req.GetChanges() {
		changes = append(changes, threadworkspace.Change{
			Path: c.GetPath(), ContentHash: c.GetContentHash(), SizeBytes: c.GetSizeBytes(), BaseHash: c.GetBaseHash(),
		})
	}
	deletions := make([]threadworkspace.Deletion, 0, len(req.GetDeleted()))
	for _, d := range req.GetDeleted() {
		deletions = append(deletions, threadworkspace.Deletion{Path: d.GetPath(), BaseHash: d.GetBaseHash()})
	}
	res, err := s.threads.Apply(ctx, principal.OrganizationID, l.OwnerID, l.ScopeID, changes, deletions, s.threadLimits)
	if err != nil {
		if errors.Is(err, threadworkspace.ErrQuotaExceeded) {
			s.recordActivity(ctx, l, mpv1.SandboxFailureCause_SANDBOX_FAILURE_CAUSE_QUOTA_EXCEEDED)
		}
		return nil, threadWorkspaceErr(err)
	}
	cause := mpv1.SandboxFailureCause_SANDBOX_FAILURE_CAUSE_UNSPECIFIED
	if len(res.Conflicts) > 0 {
		cause = mpv1.SandboxFailureCause_SANDBOX_FAILURE_CAUSE_WORKSPACE_CONFLICT
	}
	s.recordActivity(ctx, l, cause)
	return &ApplyThreadWorkspaceChangesResponse{
		ConflictingPaths: res.Conflicts,
		Revision:         res.Revision,
		FileCount:        res.FileCount,
		TotalBytes:       res.TotalBytes,
	}, nil
}

// ThreadEraser is the rows half of erasure. threadworkspace.Eraser is the
// production implementation.
type ThreadEraser interface {
	Erase(ctx context.Context, orgID, ownerID, threadID string) (int64, error)
}

// DeleteThreadWorkspace erases the caller's own thread workspace (WS22-S5). The
// rows go first and the blobs under the thread's prefix after them. A blob
// failure is reported, and a retry finds no rows and removes the prefix again,
// so the erase always finishes. Only a user may erase, and only their own
// threads: the rows are keyed by the caller's identity and no request field
// names an owner.
func (s *Server) DeleteThreadWorkspace(ctx context.Context, req *DeleteThreadWorkspaceRequest) (*DeleteThreadWorkspaceResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "DeleteThreadWorkspace")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if !s.threadWorkspaceEnabled || s.threads == nil {
		return nil, status.Error(codes.FailedPrecondition, "thread workspace is not enabled")
	}
	if principal.PrincipalType != "user" {
		return nil, status.Error(codes.PermissionDenied, "a thread workspace belongs to a user, not a service principal")
	}
	if req.GetOrgId() == "" {
		return nil, status.Error(codes.InvalidArgument, "org_id is required")
	}
	if req.GetOrgId() != principal.OrganizationID {
		return nil, status.Error(codes.PermissionDenied, "organization does not match verified identity")
	}
	prefix, err := objectstore.ThreadPrefix(principal.OrganizationID, req.GetThreadId())
	if err != nil {
		return nil, status.Error(codes.InvalidArgument, "thread_id must be a plain key segment")
	}
	if s.threadErase == nil || s.threadBlobs == nil {
		return nil, status.Error(codes.FailedPrecondition, "thread workspace erasure requires the object store")
	}
	files, err := s.threadErase.Erase(ctx, principal.OrganizationID, principal.ActorID, req.GetThreadId())
	if errors.Is(err, threadworkspace.ErrThreadShared) {
		return nil, status.Error(codes.PermissionDenied, "this thread is not yours to erase")
	}
	if err != nil {
		return nil, status.Errorf(codes.Internal, "erase thread workspace rows: %v", err)
	}
	if _, err := s.threadBlobs.DeleteThreadBlobs(ctx, principal.OrganizationID, req.GetThreadId()); err != nil {
		return nil, status.Errorf(codes.Internal, "thread workspace rows are erased but its blobs were not removed; retry the erase: %v", err)
	}
	return &DeleteThreadWorkspaceResponse{FilesDeleted: files, ObjectPrefix: prefix}, nil
}

// recordActivity stamps the lease's last use and the cause the apply produced. It is best effort: the
// apply has already landed, and failing the call over a status write would make a client retry a batch
// whose base hashes no longer match what it wrote.
func (s *Server) recordActivity(ctx context.Context, l *lease.Lease, cause mpv1.SandboxFailureCause) {
	_ = s.leases.RecordActivity(ctx, l.ID, l.OrgID, l.OwnerID, cause, time.Now().UTC())
}

// GetLeaseStatus reports a lease's lifecycle and the counts its thread workspace holds (WS22-S6). Like
// GetProcess it is descriptive: a released or expired lease still answers, so a client can say why its
// files are not kept. A thread lease belongs to its user, so a service principal may not read it.
func (s *Server) GetLeaseStatus(ctx context.Context, req *GetLeaseStatusRequest) (*GetLeaseStatusResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "GetLeaseStatus")))
	principal, err := s.principal(ctx)
	if err != nil {
		return nil, err
	}
	if req.GetLeaseId() == "" {
		return nil, status.Error(codes.InvalidArgument, "lease_id is required")
	}
	l, err := s.leases.GetAny(ctx, req.GetLeaseId(), principal.OrganizationID, authz.OwnerFilter(principal), req.GetBackendId())
	if err != nil {
		return nil, mapErr(err)
	}
	if l.ScopeType == "thread" && principal.PrincipalType != "user" {
		return nil, status.Error(codes.PermissionDenied, "a thread workspace belongs to a user, not a service principal")
	}
	resp := &GetLeaseStatusResponse{
		LeaseId:      l.ID,
		State:        l.State,
		FailureCause: l.FailureCause,
		ExpiresAt:    timestamppb.New(l.ExpiresAt),
		ScopeType:    l.ScopeType,
	}
	if !l.LastActivityAt.IsZero() {
		resp.LastActivityAt = timestamppb.New(l.LastActivityAt)
	}
	if l.HeartbeatAt != nil {
		resp.HeartbeatAt = timestamppb.New(*l.HeartbeatAt)
	}
	if l.ScopeType == "thread" && s.threads != nil {
		snap, err := s.threads.Get(ctx, principal.OrganizationID, l.OwnerID, l.ScopeID)
		if err != nil {
			return nil, status.Errorf(codes.Internal, "get thread workspace: %v", err)
		}
		for _, f := range snap.Files {
			resp.FileCount++
			resp.TotalBytes += f.SizeBytes
		}
	}
	return resp, nil
}

// Health reports serving status.
func (s *Server) Health(ctx context.Context, _ *SandboxHealthRequest) (*SandboxHealthResponse, error) {
	telemetry.RequestsTotal.Add(ctx, 1, metric.WithAttributes(attribute.String("method", "Health")))
	if _, err := s.principal(ctx); err != nil {
		return nil, err
	}
	return &SandboxHealthResponse{Status: "SERVING"}, nil
}

// Register wires the SandboxManager service onto the provided gRPC server.
func Register(g *grpc.Server, impl mpv1.SandboxManagerServer) {
	mpv1.RegisterSandboxManagerServer(g, impl)
}
