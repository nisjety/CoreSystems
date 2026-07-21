package http

import (
	"bytes"
	"context"
	nethttp "net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// TestControlLifecycleSuccessionHTTPRouteIsServicePrincipalGated proves the
// full round-trip for the admin-succession handoff (design doc Flow B):
// user-core's Service.EnsureSuccession calls this exact route before a sole
// owner/admin's erasure saga starts.
func TestControlLifecycleSuccessionHTTPRouteIsServicePrincipalGated(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(
		ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	userCoreToken := strings.Repeat("u", 48)
	readOnlyToken := strings.Repeat("r", 48)
	t.Setenv(serviceCredentialEnv, `[
  {"principal":"auth-core","audience":"org-core","token":"`+strings.Repeat("a", 48)+`","scopes":["org:projection:write:any","org:projection:delete:any"]},
  {"principal":"user-core","audience":"org-core","token":"`+userCoreToken+`","scopes":["org:membership:succession:any"]},
  {"principal":"read-only-fixture","audience":"org-core","token":"`+readOnlyToken+`","scopes":["org:read:any"]}
]`)
	repo := orgcore.NewRepository(db)
	server := NewServer(0, orgcore.NewService(repo, nil), nil, "", "")

	request := func(method, path, body, principal, token string) *httptest.ResponseRecorder {
		t.Helper()
		req := httptest.NewRequest(method, path, bytes.NewBufferString(body))
		if body != "" {
			req.Header.Set("Content-Type", "application/json")
		}
		if principal != "" {
			req.Header.Set("X-Service-Id", principal)
		}
		if token != "" {
			req.Header.Set("X-Service-Token", token)
		}
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, req)
		return response
	}

	orgID := "org-succession-http"
	ownerID := "owner-succession-http"
	memberID := "member-succession-http"

	if err := repo.ProvisionOrganizationWithOwner(ctx, orgcore.Organization{
		ID: orgID, Name: "Succession HTTP Org", Plan: "free", Status: "active",
	}, ownerID); err != nil {
		t.Fatalf("provision organization: %v", err)
	}
	if err := repo.AddOrganizationMember(ctx, orgID, memberID, "member"); err != nil {
		t.Fatalf("seed member: %v", err)
	}

	successionPath := "/internal/orgs/" + orgID + "/members/" + memberID + "/succession"

	// No credentials at all.
	if response := request(nethttp.MethodPost, successionPath, `{"role":"owner"}`, "", ""); response.Code != nethttp.StatusUnauthorized {
		t.Fatalf("missing principal status=%d body=%s", response.Code, response.Body.String())
	}
	// Wrong scope (read-only fixture cannot promote).
	if response := request(nethttp.MethodPost, successionPath, `{"role":"owner"}`, "read-only-fixture", readOnlyToken); response.Code != nethttp.StatusForbidden {
		t.Fatalf("wrong-scope principal status=%d body=%s", response.Code, response.Body.String())
	}
	// Right principal, invalid role.
	if response := request(nethttp.MethodPost, successionPath, `{"role":"viewer"}`, "user-core", userCoreToken); response.Code != nethttp.StatusBadRequest {
		t.Fatalf("invalid role status=%d body=%s", response.Code, response.Body.String())
	}
	// Right principal, non-member target.
	nonMemberPath := "/internal/orgs/" + orgID + "/members/stranger-succession-http/succession"
	if response := request(nethttp.MethodPost, nonMemberPath, `{"role":"owner"}`, "user-core", userCoreToken); response.Code != nethttp.StatusConflict {
		t.Fatalf("non-member promotion status=%d body=%s", response.Code, response.Body.String())
	}

	// The real handoff.
	if response := request(nethttp.MethodPost, successionPath, `{"role":"owner"}`, "user-core", userCoreToken); response.Code != nethttp.StatusOK {
		t.Fatalf("succession promotion status=%d body=%s", response.Code, response.Body.String())
	}

	members, err := repo.ListOrganizationMembers(ctx, orgID)
	if err != nil {
		t.Fatalf("list organization members: %v", err)
	}
	roles := map[string]string{}
	for _, m := range members {
		roles[m.UserID] = m.Role
	}
	if roles[memberID] != "owner" {
		t.Fatalf("member role after HTTP succession = %q, want owner", roles[memberID])
	}
	if roles[ownerID] != "owner" {
		t.Fatalf("original owner role changed to %q via succession route", roles[ownerID])
	}
}
