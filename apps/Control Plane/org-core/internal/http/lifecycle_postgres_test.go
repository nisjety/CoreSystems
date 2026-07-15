package http

import (
	"bytes"
	"context"
	"encoding/json"
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

func TestControlLifecycleOrgScopedHTTPRoutesWithPostgres(t *testing.T) {
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
		ctx,
		db.Pool,
		dsn,
		"org_lifecycle",
		os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	authToken := strings.Repeat("a", 48)
	readOnlyToken := strings.Repeat("r", 48)
	t.Setenv(serviceCredentialEnv, `[
  {"principal":"auth-core","audience":"org-core","token":"`+authToken+`","scopes":["org:projection:write:any","org:projection:delete:any"]},
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

	orgID := "org-http-lifecycle"
	projectionPath := "/internal/orgs/" + orgID + "/reconcile"
	initialBody := `{"name":"HTTP Revision One","slug":"http-revision-one","ownerUserId":"owner-http-lifecycle","revision":1}`
	if response := request(nethttp.MethodPost, projectionPath, initialBody, "", ""); response.Code != nethttp.StatusUnauthorized {
		t.Fatalf("missing principal status=%d body=%s", response.Code, response.Body.String())
	}
	if response := request(nethttp.MethodPost, projectionPath, initialBody, "read-only-fixture", readOnlyToken); response.Code != nethttp.StatusForbidden {
		t.Fatalf("wrong-scope principal status=%d body=%s", response.Code, response.Body.String())
	}
	legacy := httptest.NewRequest(nethttp.MethodPost, projectionPath, bytes.NewBufferString(initialBody))
	legacy.Header.Set("Content-Type", "application/json")
	legacy.Header.Set("X-Internal-Api-Key", authToken)
	legacyResponse := httptest.NewRecorder()
	server.router.ServeHTTP(legacyResponse, legacy)
	if legacyResponse.Code != nethttp.StatusUnauthorized {
		t.Fatalf("legacy key status=%d body=%s", legacyResponse.Code, legacyResponse.Body.String())
	}

	assertApplied := func(body string, want bool) {
		t.Helper()
		response := request(nethttp.MethodPost, projectionPath, body, "auth-core", authToken)
		if response.Code != nethttp.StatusOK {
			t.Fatalf("projection status=%d body=%s", response.Code, response.Body.String())
		}
		var payload struct {
			Applied bool `json:"applied"`
		}
		if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
			t.Fatalf("decode projection response: %v", err)
		}
		if payload.Applied != want {
			t.Fatalf("projection applied=%t want=%t body=%s", payload.Applied, want, response.Body.String())
		}
	}
	assertApplied(initialBody, true)
	assertApplied(initialBody, false)
	conflictingProjection := request(
		nethttp.MethodPost,
		projectionPath,
		`{"name":"Conflicting Revision One","ownerUserId":"owner-http-lifecycle","revision":1}`,
		"auth-core",
		authToken,
	)
	if conflictingProjection.Code != nethttp.StatusConflict {
		t.Fatalf("same-revision organization conflict status=%d body=%s", conflictingProjection.Code, conflictingProjection.Body.String())
	}
	assertApplied(`{"name":"HTTP Revision Three","ownerUserId":"owner-http-lifecycle","revision":3}`, true)
	assertApplied(`{"name":"Delayed Revision Two","ownerUserId":"owner-http-lifecycle","revision":2}`, false)

	memberPath := "/internal/orgs/" + orgID + "/members/reconcile"
	member := request(nethttp.MethodPost, memberPath,
		`{"userId":"member-http-lifecycle","role":"admin","action":"upsert","revision":5}`,
		"auth-core", authToken)
	if member.Code != nethttp.StatusOK || !bytes.Contains(member.Body.Bytes(), []byte(`"applied":true`)) {
		t.Fatalf("membership projection status=%d body=%s", member.Code, member.Body.String())
	}
	delayedRemoval := request(nethttp.MethodPost, memberPath,
		`{"userId":"member-http-lifecycle","role":"member","action":"remove","revision":4}`,
		"auth-core", authToken)
	if delayedRemoval.Code != nethttp.StatusOK || !bytes.Contains(delayedRemoval.Body.Bytes(), []byte(`"applied":false`)) {
		t.Fatalf("delayed membership status=%d body=%s", delayedRemoval.Code, delayedRemoval.Body.String())
	}
	conflictingRole := request(nethttp.MethodPost, memberPath,
		`{"userId":"member-http-lifecycle","role":"viewer","action":"upsert","revision":5}`,
		"auth-core", authToken)
	if conflictingRole.Code != nethttp.StatusConflict {
		t.Fatalf("same-revision role conflict status=%d body=%s", conflictingRole.Code, conflictingRole.Body.String())
	}

	deletionPath := "/internal/orgs/" + orgID + "/reconcile-delete"
	for _, invalidBody := range []string{
		``,
		`{}`,
		`{"revision":0}`,
		`{"revision":-1}`,
		`{"revision":1.5}`,
		`{"revision":"4"}`,
		`{"revision":9007199254740992}`,
		`{`,
	} {
		response := request(nethttp.MethodPost, deletionPath, invalidBody, "auth-core", authToken)
		if response.Code != nethttp.StatusBadRequest {
			t.Fatalf("invalid deletion body %q status=%d body=%s", invalidBody, response.Code, response.Body.String())
		}
	}
	var tombstonesBeforeDelete int
	if err := db.Pool.QueryRow(ctx,
		`SELECT COUNT(*)::INT FROM auth_organization_tombstones WHERE org_id = $1`, orgID,
	).Scan(&tombstonesBeforeDelete); err != nil {
		t.Fatalf("count tombstones after invalid deletion requests: %v", err)
	}
	if tombstonesBeforeDelete != 0 {
		t.Fatalf("invalid deletion request persisted %d tombstone(s)", tombstonesBeforeDelete)
	}
	for _, conflictingBody := range []string{
		`{"revision":2}`,
		`{"revision":3}`,
	} {
		response := request(nethttp.MethodPost, deletionPath, conflictingBody, "auth-core", authToken)
		if response.Code != nethttp.StatusConflict {
			t.Fatalf("conflicting deletion body %s status=%d body=%s", conflictingBody, response.Code, response.Body.String())
		}
	}

	assertDeletionApplied := func(body string, want bool) {
		t.Helper()
		response := request(nethttp.MethodPost, deletionPath, body, "auth-core", authToken)
		if response.Code != nethttp.StatusOK {
			t.Fatalf("deletion body %s status=%d body=%s", body, response.Code, response.Body.String())
		}
		var payload struct {
			Applied bool `json:"applied"`
		}
		if err := json.Unmarshal(response.Body.Bytes(), &payload); err != nil {
			t.Fatalf("decode deletion response: %v", err)
		}
		if payload.Applied != want {
			t.Fatalf("deletion applied=%t want=%t body=%s", payload.Applied, want, response.Body.String())
		}
	}
	assertDeletionApplied(`{"revision":4}`, true)
	assertDeletionApplied(`{"revision":4}`, false)
	newerReuse := request(
		nethttp.MethodPost,
		deletionPath,
		`{"revision":5}`,
		"auth-core",
		authToken,
	)
	if newerReuse.Code != nethttp.StatusConflict {
		t.Fatalf("post-tombstone newer deletion status=%d body=%s", newerReuse.Code, newerReuse.Body.String())
	}
	delayedProjection := request(nethttp.MethodPost, projectionPath,
		`{"name":"Must Stay Deleted","ownerUserId":"owner-http-lifecycle","revision":4}`,
		"auth-core", authToken)
	if delayedProjection.Code != nethttp.StatusConflict {
		t.Fatalf("post-delete projection status=%d body=%s", delayedProjection.Code, delayedProjection.Body.String())
	}

	var name string
	var organizations, tombstones int
	var tombstoneRevision int64
	var erasureCompleted bool
	if err := db.Pool.QueryRow(ctx, `SELECT COALESCE(MAX(name), '') FROM organizations WHERE id = $1`, orgID).Scan(&name); err != nil {
		t.Fatalf("read final organization: %v", err)
	}
	if err := db.Pool.QueryRow(ctx, `SELECT COUNT(*)::INT FROM organizations WHERE id = $1`, orgID).Scan(&organizations); err != nil {
		t.Fatalf("count final organization: %v", err)
	}
	if err := db.Pool.QueryRow(ctx, `
		SELECT COUNT(*)::INT, COALESCE(MAX(revision), 0),
		       BOOL_AND(erasure_completed_at IS NOT NULL AND deletion_receipt IS NOT NULL)
		FROM auth_organization_tombstones WHERE org_id = $1`, orgID,
	).Scan(&tombstones, &tombstoneRevision, &erasureCompleted); err != nil {
		t.Fatalf("read final tombstone: %v", err)
	}
	if name != "" || organizations != 0 || tombstones != 1 || tombstoneRevision != 4 || !erasureCompleted {
		t.Fatalf("final lifecycle state name=%q organizations=%d tombstones=%d revision=%d completed=%t", name, organizations, tombstones, tombstoneRevision, erasureCompleted)
	}
}
