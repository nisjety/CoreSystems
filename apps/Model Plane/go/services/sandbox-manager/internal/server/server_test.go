package server

import (
	"context"
	"net"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/types/known/durationpb"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
	"github.com/triodelab/model-plane/services/sandbox-manager/internal/snapshot"
)

func newTestClient(t *testing.T) mpv1.SandboxManagerClient {
	t.Helper()

	listener := bufconn.Listen(1024 * 1024)
	grpcServer := grpc.NewServer()
	Register(grpcServer, newTestServer())

	go func() {
		_ = grpcServer.Serve(listener)
	}()

	t.Cleanup(func() {
		grpcServer.Stop()
		_ = listener.Close()
	})

	conn, err := grpc.DialContext(
		context.Background(),
		"bufnet",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(context.Context, string) (net.Conn, error) {
			return listener.Dial()
		}),
	)
	if err != nil {
		t.Fatalf("DialContext: unexpected error: %v", err)
	}
	t.Cleanup(func() {
		_ = conn.Close()
	})

	return mpv1.NewSandboxManagerClient(conn)
}

func newTestServer() *Server {
	return NewServer(lease.NewStore(), snapshot.NewStore())
}

func TestHealth_ReturnsServing(t *testing.T) {
	s := newTestServer()
	resp, err := s.Health(context.Background(), &SandboxHealthRequest{})
	if err != nil {
		t.Fatalf("Health: unexpected error: %v", err)
	}
	if resp.Status != "SERVING" {
		t.Errorf("expected status SERVING, got %q", resp.Status)
	}
}

func TestAcquireLease_Success(t *testing.T) {
	s := newTestServer()
	resp, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(30 * time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: unexpected error: %v", err)
	}
	if resp.GetLeaseId() == "" {
		t.Error("expected non-empty LeaseID")
	}
	if resp.GetEndpoint() == "" {
		t.Error("expected non-empty Endpoint")
	}
	if resp.GetExpiresAt() == nil || !resp.GetExpiresAt().AsTime().After(time.Now()) {
		t.Errorf("expected ExpiresAt in future, got %v", resp.GetExpiresAt())
	}
}

func TestReleaseLease_NotFound(t *testing.T) {
	s := newTestServer()
	_, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: "does-not-exist"})
	if got := status.Code(err); got != codes.NotFound {
		t.Errorf("expected NotFound, got %v (err=%v)", got, err)
	}
}

func TestAcquireLease_ValidatesRequiredFields(t *testing.T) {
	tests := []struct {
		name string
		req  *AcquireLeaseRequest
	}{
		{
			name: "missing scope_id",
			req:  &AcquireLeaseRequest{ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "missing scope_type",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", OrgId: "org-1", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "invalid scope_type",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", ScopeType: "workspace", OrgId: "org-1", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "missing org_id",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", ScopeType: "agent", Ttl: durationpb.New(time.Minute)},
		},
		{
			name: "missing ttl",
			req:  &AcquireLeaseRequest{ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1"},
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			s := newTestServer()
			_, err := s.AcquireLease(context.Background(), tc.req)
			if got := status.Code(err); got != codes.InvalidArgument {
				t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
			}
		})
	}
}

func TestReleaseLease_Success(t *testing.T) {
	s := newTestServer()
	acq, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	resp, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{LeaseId: acq.GetLeaseId()})
	if err != nil {
		t.Fatalf("ReleaseLease: unexpected error: %v", err)
	}
	if !resp.Released {
		t.Error("expected Released=true")
	}
}

func TestSnapshotSandbox_LeaseNotFound(t *testing.T) {
	s := newTestServer()
	_, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: "does-not-exist", Label: "test",
	})
	if got := status.Code(err); got != codes.NotFound {
		t.Errorf("expected NotFound, got %v (err=%v)", got, err)
	}
}

func TestReleaseLease_EmptyLeaseIDReturnsInvalidArgument(t *testing.T) {
	s := newTestServer()
	_, err := s.ReleaseLease(context.Background(), &ReleaseLeaseRequest{})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
	}
}

func TestSnapshotSandbox_ValidatesRequiredFields(t *testing.T) {
	tests := []struct {
		name string
		req  *SnapshotRequest
	}{
		{name: "missing lease_id", req: &SnapshotRequest{Label: "checkpoint"}},
		{name: "missing label", req: &SnapshotRequest{LeaseId: "lease-1"}},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			s := newTestServer()
			_, err := s.SnapshotSandbox(context.Background(), tc.req)
			if got := status.Code(err); got != codes.InvalidArgument {
				t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
			}
		})
	}
}

func TestSnapshotSandbox_Success(t *testing.T) {
	s := newTestServer()
	acq, err := s.AcquireLease(context.Background(), &AcquireLeaseRequest{
		ScopeId: "scope-1", ScopeType: "agent", OrgId: "org-1", Ttl: durationpb.New(time.Minute),
	})
	if err != nil {
		t.Fatalf("AcquireLease: %v", err)
	}
	resp, err := s.SnapshotSandbox(context.Background(), &SnapshotRequest{
		LeaseId: acq.GetLeaseId(), Label: "checkpoint-1",
	})
	if err != nil {
		t.Fatalf("SnapshotSandbox: unexpected error: %v", err)
	}
	if resp.GetSnapshotId() == "" {
		t.Error("expected non-empty SnapshotID")
	}
	if resp.GetObjectKey() == "" {
		t.Error("expected non-empty ObjectKey")
	}
}

func TestAcquireLease_TransportRoundTrip(t *testing.T) {
	client := newTestClient(t)

	resp, err := client.AcquireLease(context.Background(), &mpv1.AcquireLeaseRequest{
		ScopeId:   "scope-transport",
		ScopeType: "agent",
		Ttl:       durationpb.New(time.Minute),
		OrgId:     "org-transport",
	})
	if err != nil {
		t.Fatalf("AcquireLease transport: unexpected error: %v", err)
	}
	if resp.GetLeaseId() == "" {
		t.Error("expected non-empty lease_id")
	}
	if resp.GetEndpoint() == "" {
		t.Error("expected non-empty endpoint")
	}
	if resp.GetExpiresAt() == nil || resp.GetExpiresAt().AsTime().Before(time.Now()) {
		t.Fatalf("expected future expires_at, got %v", resp.GetExpiresAt())
	}
}
