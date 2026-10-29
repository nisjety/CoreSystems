package service

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/clients"
	"github.com/I-Dacosta/CoreSystem/apps/session-core/internal/domain"
)

// ---------------------------------------------------------------------------
// routeToV2 — pure FNV-1a hash rollout logic
// ---------------------------------------------------------------------------

func TestRouteToV2_Rollout100_AlwaysTrue(t *testing.T) {
	tenants := []string{"a", "b", "tenant-xyz", "t1", "org-123", "another-long-tenant-id"}
	for _, tid := range tenants {
		if !routeToV2(tid, 100) {
			t.Fatalf("rolloutPct=100 should always return true, failed for %q", tid)
		}
	}
}

func TestRouteToV2_Rollout0_AlwaysFalse(t *testing.T) {
	tenants := []string{"a", "b", "tenant-xyz", "t1", "org-123", "another-long-tenant-id"}
	for _, tid := range tenants {
		if routeToV2(tid, 0) {
			t.Fatalf("rolloutPct=0 should always return false, failed for %q", tid)
		}
	}
}

func TestRouteToV2_NegativeRollout_AlwaysFalse(t *testing.T) {
	tenants := []string{"any-tenant", "other", ""}
	for _, tid := range tenants {
		if routeToV2(tid, -1) {
			t.Fatalf("negative rolloutPct should return false, failed for %q", tid)
		}
	}
}

func TestRouteToV2_IntermediatePct_ReturnsBool(t *testing.T) {
	// Verify no panic and a valid bool is returned for intermediate values.
	// We don't assert true/false since the result depends on the FNV-1a hash.
	for pct := 1; pct < 100; pct += 10 {
		_ = routeToV2("test-tenant", pct)
	}
}

func TestRouteToV2_Deterministic(t *testing.T) {
	// Same input must always produce the same result.
	first := routeToV2("stable-tenant", 50)
	for i := 0; i < 10; i++ {
		if routeToV2("stable-tenant", 50) != first {
			t.Fatal("routeToV2 produced non-deterministic result for same inputs")
		}
	}
}

// ---------------------------------------------------------------------------
// CreateSession — org membership gating (repo-safe 403 path only)
// ---------------------------------------------------------------------------

// TestCreateSession_OrgClientRejectsUser verifies that when the org-core
// service returns a non-200 status, CreateSession wraps and returns
// ErrOrgMembershipDenied.  The session repository is left nil because
// CreateSession exits before s.repo.Create is reached on this code path.
func TestCreateSession_OrgClientRejectsUser_ReturnsErrOrgMembershipDenied(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusForbidden)
	}))
	defer ts.Close()

	// Struct literal is accessible here because this test is in package service.
	// Fields not relevant to the 403 path remain nil — CreateSession returns
	// before touching repo, nats, cache, or convex on this path.
	svc := &SessionService{
		orgClient:  clients.NewOrgClient(ts.URL),
		rolloutPct: 100,
	}

	req := &domain.CreateSessionRequest{
		TenantID:    "tenant-1",
		WorkspaceID: "workspace-1",
		OrgID:       "org-1",
	}

	_, err := svc.CreateSession(context.Background(), "user-1", req)
	if !errors.Is(err, domain.ErrOrgMembershipDenied) {
		t.Fatalf("expected ErrOrgMembershipDenied, got: %v", err)
	}
}

func TestCreateSession_OrgClientUserNotMember_ReturnsErrOrgMembershipDenied(t *testing.T) {
	import_json_members := `{"members":[],"count":0}`
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(import_json_members))
	}))
	defer ts.Close()

	svc := &SessionService{
		orgClient:  clients.NewOrgClient(ts.URL),
		rolloutPct: 0,
	}

	req := &domain.CreateSessionRequest{
		TenantID:    "tenant-2",
		WorkspaceID: "workspace-2",
		OrgID:       "org-2",
	}

	_, err := svc.CreateSession(context.Background(), "non-member", req)
	if !errors.Is(err, domain.ErrOrgMembershipDenied) {
		t.Fatalf("expected ErrOrgMembershipDenied for non-member, got: %v", err)
	}
}

// ---------------------------------------------------------------------------
// ErrOrgMembershipDenied sentinel — wrapping / unwrapping
// ---------------------------------------------------------------------------

func TestErrOrgMembershipDenied_WrapsViaErrorsIs(t *testing.T) {
	// Verify that the wrapping pattern used in CreateSession unwraps correctly.
	wrapped := fmt.Errorf("%w: user is not an active member of org %s", domain.ErrOrgMembershipDenied, "org-1")
	if !errors.Is(wrapped, domain.ErrOrgMembershipDenied) {
		t.Fatal("errors.Is should unwrap wrapped ErrOrgMembershipDenied")
	}
}

func TestErrOrgMembershipDenied_UnrelatedError_DoesNotMatch(t *testing.T) {
	other := errors.New("some other error")
	if errors.Is(other, domain.ErrOrgMembershipDenied) {
		t.Fatal("unrelated error should not match ErrOrgMembershipDenied")
	}
}
