package server

import (
	"context"
	"reflect"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/authz"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/threadworkspace"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/workspace"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/reflect/protodesc"
	"google.golang.org/protobuf/reflect/protoreflect"
	"google.golang.org/protobuf/types/descriptorpb"
	"google.golang.org/protobuf/types/dynamicpb"
	"google.golang.org/protobuf/types/known/durationpb"
)

// recordingWorkspaceStore wraps the in-memory store and records every
// UpsertTombstones call, so a test can prove the store was never reached.
type recordingWorkspaceStore struct {
	*MemoryWorkspaceStore
	tombstoneCalls [][]workspace.DeletedPath
}

func (r *recordingWorkspaceStore) UpsertTombstones(ctx context.Context, orgID, spaceID, runID string, deleted []workspace.DeletedPath) error {
	r.tombstoneCalls = append(r.tombstoneCalls, deleted)
	return r.MemoryWorkspaceStore.UpsertTombstones(ctx, orgID, spaceID, runID, deleted)
}

// tombstoneHarness is a Server over the in-memory stores with an active
// Space lease. pending is what the next snapshot's deleted_paths reads as: the
// generated SnapshotRequest does not carry the field until F-f05-99
// regenerates it, so the server's reader is injected here.
type tombstoneHarness struct {
	s       *Server
	store   *recordingWorkspaceStore
	leaseID string
	pending []workspace.DeletedPath
}

func newTombstoneHarness(t *testing.T, enabled bool) *tombstoneHarness {
	t.Helper()
	h := &tombstoneHarness{store: &recordingWorkspaceStore{MemoryWorkspaceStore: NewMemoryWorkspaceStore()}}
	h.s = NewServer(NewMemoryLeaseStore(), NewMemorySnapshotStore(), h.store).WithOverlayTombstones(enabled)
	h.s.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user"}, nil
	}
	h.s.capabilityVerify = func(string, string, authz.CapabilityExpectation) (authz.VerifiedCapability, error) {
		return authz.VerifiedCapability{Claims: authz.SpaceCapabilityClaims{BackendID: "backend-1"}}, nil
	}
	h.s.backendID = "backend-1"
	h.s.deletedPaths = func(*SnapshotRequest) []workspace.DeletedPath { return h.pending }

	acquired, err := h.s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", SpaceId: "space-1",
		CapabilityDecision: "token", CapabilityClaimsJson: "{}", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	h.leaseID = acquired.GetLeaseId()
	if _, err := h.s.ActivateLease(context.Background(), &ActivateLeaseRequest{LeaseId: h.leaseID, BackendId: "backend-1"}); err != nil {
		t.Fatalf("ActivateLease: %v", err)
	}
	return h
}

func (h *tombstoneHarness) seedSpace(t *testing.T, files ...workspace.ChangedFile) {
	t.Helper()
	if err := h.store.UpsertOverlay(context.Background(), "org-1", "space-1", "", files); err != nil {
		t.Fatalf("seed space rows: %v", err)
	}
}

func (h *tombstoneHarness) snapshot(t *testing.T, deleted []workspace.DeletedPath, changed ...*WorkspaceChangedFile) {
	t.Helper()
	h.pending = deleted
	if _, err := h.s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: h.leaseID, Label: "x", BackendId: "backend-1", ChangedFiles: changed,
	}); err != nil {
		t.Fatalf("SnapshotSandbox: %v", err)
	}
}

func (h *tombstoneHarness) manifest(t *testing.T) map[string]string {
	t.Helper()
	resp, err := h.s.GetWorkspaceManifest(context.Background(), &GetWorkspaceManifestRequest{LeaseId: h.leaseID, BackendId: "backend-1"})
	if err != nil {
		t.Fatalf("GetWorkspaceManifest: %v", err)
	}
	out := make(map[string]string, len(resp.GetEntries()))
	for _, e := range resp.GetEntries() {
		out[e.GetPath()] = e.GetContentHash()
	}
	return out
}

func (h *tombstoneHarness) spaceView(t *testing.T) map[string]string {
	t.Helper()
	entries, err := h.store.GetManifest(context.Background(), "org-1", "space-1", "no-run")
	if err != nil {
		t.Fatalf("GetManifest (Space view): %v", err)
	}
	out := make(map[string]string, len(entries))
	for _, e := range entries {
		out[e.Path] = e.ContentHash
	}
	return out
}

func (h *tombstoneHarness) promote(t *testing.T) []string {
	t.Helper()
	resp, err := h.s.PromoteWorkspace(context.Background(), &PromoteWorkspaceRequest{LeaseId: h.leaseID, BackendId: "backend-1"})
	if err != nil {
		t.Fatalf("PromoteWorkspace: %v", err)
	}
	return resp.GetConflictingPaths()
}

func TestOverlayTombstoneHidesThePathFromTheRunManifestAndPromoteDeletesTheSpaceRow(t *testing.T) {
	h := newTombstoneHarness(t, true)
	h.seedSpace(t,
		workspace.ChangedFile{Path: "gone.txt", ContentHash: "sha256:gone", SizeBytes: 4},
		workspace.ChangedFile{Path: "kept.txt", ContentHash: "sha256:kept", SizeBytes: 4},
	)

	h.snapshot(t, []workspace.DeletedPath{{Path: "gone.txt", BaseHash: "sha256:gone"}})

	if got := h.manifest(t); !reflect.DeepEqual(got, map[string]string{"kept.txt": "sha256:kept"}) {
		t.Fatalf("run manifest = %v, want the tombstoned path hidden and the other kept", got)
	}
	// Before Promote the Space itself is untouched.
	if got := h.spaceView(t); got["gone.txt"] != "sha256:gone" {
		t.Fatalf("Space view before promote = %v, want gone.txt still present", got)
	}
	if conflicts := h.promote(t); len(conflicts) != 0 {
		t.Fatalf("conflicts = %v, want none", conflicts)
	}
	if got := h.spaceView(t); !reflect.DeepEqual(got, map[string]string{"kept.txt": "sha256:kept"}) {
		t.Fatalf("Space view after promote = %v, want gone.txt deleted", got)
	}
}

func TestOverlayTombstonePromoteIsIdempotentOnReplay(t *testing.T) {
	h := newTombstoneHarness(t, true)
	h.seedSpace(t, workspace.ChangedFile{Path: "gone.txt", ContentHash: "sha256:gone", SizeBytes: 4})
	h.snapshot(t, []workspace.DeletedPath{{Path: "gone.txt", BaseHash: "sha256:gone"}})

	if conflicts := h.promote(t); len(conflicts) != 0 {
		t.Fatalf("first promote conflicts = %v", conflicts)
	}
	if conflicts := h.promote(t); len(conflicts) != 0 {
		t.Fatalf("replayed promote conflicts = %v, want none (the Space row is already gone)", conflicts)
	}
}

func TestOverlayTombstonePromoteReportsAConflictWhenTheSpaceRowChanged(t *testing.T) {
	h := newTombstoneHarness(t, true)
	h.seedSpace(t,
		workspace.ChangedFile{Path: "gone.txt", ContentHash: "sha256:gone", SizeBytes: 4},
		workspace.ChangedFile{Path: "other.txt", ContentHash: "sha256:other", SizeBytes: 4},
	)
	h.snapshot(t, []workspace.DeletedPath{
		{Path: "gone.txt", BaseHash: "sha256:gone"},
		{Path: "other.txt", BaseHash: "sha256:other"},
	})
	// Someone else changed gone.txt after this run hydrated.
	h.seedSpace(t, workspace.ChangedFile{Path: "gone.txt", ContentHash: "sha256:changed", SizeBytes: 7})

	conflicts := h.promote(t)
	if !reflect.DeepEqual(conflicts, []string{"gone.txt"}) {
		t.Fatalf("conflicts = %v, want [gone.txt]", conflicts)
	}
	got := h.spaceView(t)
	if got["gone.txt"] != "sha256:changed" {
		t.Fatalf("Space gone.txt = %q, want the other writer's content untouched", got["gone.txt"])
	}
	if _, present := got["other.txt"]; present {
		t.Fatalf("Space view = %v, want other.txt deleted independently of the conflict", got)
	}
}

func TestOverlayRecreatedFileUnTombstonesThePath(t *testing.T) {
	h := newTombstoneHarness(t, true)
	h.seedSpace(t, workspace.ChangedFile{Path: "a.txt", ContentHash: "sha256:a1", SizeBytes: 2})
	h.snapshot(t, []workspace.DeletedPath{{Path: "a.txt", BaseHash: "sha256:a1"}})
	if got := h.manifest(t); len(got) != 0 {
		t.Fatalf("manifest after delete = %v, want empty", got)
	}

	h.snapshot(t, nil, &WorkspaceChangedFile{Path: "a.txt", ContentHash: "sha256:a2", SizeBytes: 3, BaseHash: "sha256:a1"})

	if got := h.manifest(t); !reflect.DeepEqual(got, map[string]string{"a.txt": "sha256:a2"}) {
		t.Fatalf("manifest after recreate = %v, want the recreated file", got)
	}
	if conflicts := h.promote(t); len(conflicts) != 0 {
		t.Fatalf("conflicts = %v", conflicts)
	}
	if got := h.spaceView(t); got["a.txt"] != "sha256:a2" {
		t.Fatalf("Space view = %v, want the recreated content merged, not deleted", got)
	}
}

func TestOverlayTombstoneForAPathTheRunCreatedDropsItsOwnOverlayRow(t *testing.T) {
	h := newTombstoneHarness(t, true)
	h.snapshot(t, nil, &WorkspaceChangedFile{Path: "scratch.txt", ContentHash: "sha256:s", SizeBytes: 1})
	h.snapshot(t, []workspace.DeletedPath{{Path: "scratch.txt"}})

	if got := h.manifest(t); len(got) != 0 {
		t.Fatalf("manifest = %v, want the created-then-removed file gone", got)
	}
	if conflicts := h.promote(t); len(conflicts) != 0 {
		t.Fatalf("conflicts = %v", conflicts)
	}
	if got := h.spaceView(t); len(got) != 0 {
		t.Fatalf("Space view = %v, want nothing promoted", got)
	}
}

func TestOverlayTombstonesAreIgnoredWithTheSwitchOff(t *testing.T) {
	h := newTombstoneHarness(t, false)
	h.seedSpace(t, workspace.ChangedFile{Path: "gone.txt", ContentHash: "sha256:gone", SizeBytes: 4})

	h.snapshot(t, []workspace.DeletedPath{{Path: "gone.txt", BaseHash: "sha256:gone"}})

	if len(h.store.tombstoneCalls) != 0 {
		t.Fatalf("UpsertTombstones called %d time(s) with the switch off", len(h.store.tombstoneCalls))
	}
	if got := h.manifest(t); got["gone.txt"] != "sha256:gone" {
		t.Fatalf("manifest = %v, want deleted_paths ignored", got)
	}
}

func TestOverlayTombstonesAreIgnoredForAThreadLease(t *testing.T) {
	store := &recordingWorkspaceStore{MemoryWorkspaceStore: NewMemoryWorkspaceStore()}
	h := newThreadHarness(true, threadworkspace.Limits{MaxFiles: 8, MaxBytes: 1 << 20})
	h.s.workspace = store
	h.s.WithOverlayTombstones(true)
	h.s.deletedPaths = func(*SnapshotRequest) []workspace.DeletedPath {
		return []workspace.DeletedPath{{Path: "x.txt", BaseHash: "sha256:x"}}
	}
	thread, err := h.acquireThread("thread-1", time.Minute)
	if err != nil {
		t.Fatalf("acquire thread: %v", err)
	}
	if _, err := h.s.SnapshotSandbox(context.Background(), &SnapshotRequest{LeaseId: thread.GetLeaseId(), Label: "x", BackendId: "backend-1"}); err != nil {
		t.Fatalf("SnapshotSandbox on a thread lease: %v", err)
	}
	if len(store.tombstoneCalls) != 0 {
		t.Fatalf("UpsertTombstones called %d time(s) for a thread lease", len(store.tombstoneCalls))
	}
}

// TestSnapshotDeletedPathsReadsTheRepeatedMessageField pins the reflection
// reader against a descriptor shaped like the proto's deleted_paths (tag 5) and
// WorkspaceDeletedPath, which the generated Go does not carry before F-f05-99.
func TestSnapshotDeletedPathsReadsTheRepeatedMessageField(t *testing.T) {
	str := descriptorpb.FieldDescriptorProto_TYPE_STRING.Enum()
	msgType := descriptorpb.FieldDescriptorProto_TYPE_MESSAGE.Enum()
	optional := descriptorpb.FieldDescriptorProto_LABEL_OPTIONAL.Enum()
	repeated := descriptorpb.FieldDescriptorProto_LABEL_REPEATED.Enum()
	file, err := protodesc.NewFile(&descriptorpb.FileDescriptorProto{
		Name:    proto.String("deleted_paths_test.proto"),
		Package: proto.String("t"),
		Syntax:  proto.String("proto3"),
		MessageType: []*descriptorpb.DescriptorProto{
			{Name: proto.String("Del"), Field: []*descriptorpb.FieldDescriptorProto{
				{Name: proto.String("path"), Number: proto.Int32(1), Type: str, Label: optional, JsonName: proto.String("path")},
				{Name: proto.String("base_hash"), Number: proto.Int32(2), Type: str, Label: optional, JsonName: proto.String("baseHash")},
			}},
			{Name: proto.String("Req"), Field: []*descriptorpb.FieldDescriptorProto{
				{Name: proto.String("deleted_paths"), Number: proto.Int32(5), Type: msgType, TypeName: proto.String(".t.Del"), Label: repeated, JsonName: proto.String("deletedPaths")},
			}},
			{Name: proto.String("Bare"), Field: []*descriptorpb.FieldDescriptorProto{
				{Name: proto.String("label"), Number: proto.Int32(1), Type: str, Label: optional, JsonName: proto.String("label")},
			}},
		},
	}, nil)
	if err != nil {
		t.Fatalf("build descriptor: %v", err)
	}
	reqDesc := file.Messages().ByName("Req")
	delDesc := file.Messages().ByName("Del")

	req := dynamicpb.NewMessage(reqDesc)
	list := req.Mutable(reqDesc.Fields().ByName("deleted_paths")).List()
	for _, d := range []struct{ path, hash string }{{"a.txt", "sha256:a"}, {"b.txt", ""}} {
		item := dynamicpb.NewMessage(delDesc)
		item.Set(delDesc.Fields().ByName("path"), protoreflect.ValueOfString(d.path))
		item.Set(delDesc.Fields().ByName("base_hash"), protoreflect.ValueOfString(d.hash))
		list.Append(protoreflect.ValueOfMessage(item))
	}
	want := []workspace.DeletedPath{{Path: "a.txt", BaseHash: "sha256:a"}, {Path: "b.txt"}}
	if got := deletedPathsFromMessage(req); !reflect.DeepEqual(got, want) {
		t.Fatalf("deleted paths = %+v, want %+v", got, want)
	}

	// A message without the field, an empty list, and a request that predates
	// the field all read as no deletions.
	if got := deletedPathsFromMessage(dynamicpb.NewMessage(file.Messages().ByName("Bare"))); got != nil {
		t.Fatalf("message without the field read as %+v, want none", got)
	}
	if got := deletedPathsFromMessage(dynamicpb.NewMessage(reqDesc)); got != nil {
		t.Fatalf("empty list read as %+v, want none", got)
	}
	if got := snapshotDeletedPaths(&SnapshotRequest{LeaseId: "l", Label: "x"}); got != nil {
		t.Fatalf("plain SnapshotRequest read as %+v, want none", got)
	}
	if got := snapshotDeletedPaths(nil); got != nil {
		t.Fatalf("nil request read as %+v, want none", got)
	}
}
