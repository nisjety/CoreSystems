package http

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

const resourceGrantsDDL = `
CREATE TABLE IF NOT EXISTS resource_grants (
    grant_id      TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id   TEXT NOT NULL,
    subject_type  TEXT NOT NULL,
    subject_id    TEXT NOT NULL,
    role          TEXT NOT NULL,
    granted_by    TEXT NOT NULL DEFAULT '',
    granted_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT resource_grants_subject_type_chk CHECK (subject_type IN ('user','team')),
    CONSTRAINT resource_grants_role_chk         CHECK (role IN ('view','edit')),
    CONSTRAINT resource_grants_unique           UNIQUE (org_id, resource_type, resource_id, subject_type, subject_id)
);`

// End-to-end facade test: HTTP handler → AclRepository → resource_grants. This
// is the exact cross-plane path documents-api/retrieval take, so it is worth
// proving live. Gated on TEST_DATABASE_URL.
func TestAuthzVisibleLive(t *testing.T) {
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping live facade test")
	}
	ctx := context.Background()
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	if _, err := pool.Exec(ctx, resourceGrantsDDL); err != nil {
		t.Fatalf("ensure resource_grants: %v", err)
	}

	repo := users.NewAclRepository(&database.DB{Pool: pool})
	org := "facade-live-" + t.Name()
	defer func() { _, _ = pool.Exec(context.Background(), `DELETE FROM resource_grants WHERE org_id=$1`, org) }()

	// Seed: user-a granted doc-1; user-b granted nothing.
	if _, err := repo.Grant(ctx, &users.ResourceGrant{
		OrgID: org, ResourceType: "document", ResourceID: "doc-1",
		SubjectType: "user", SubjectID: "user-a", Role: "view",
	}); err != nil {
		t.Fatalf("seed grant: %v", err)
	}

	s := &Server{aclRepo: repo}

	// user-a sees doc-1.
	c, w := newGinCtx("/api/v1/internal/authz/visible?org_id=" + org + "&subject_id=user-a&resource_type=document")
	s.authzVisible(c)
	if w.Code != http.StatusOK {
		t.Fatalf("authzVisible(user-a): want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var resp struct {
		IDs    []string `json:"ids"`
		AllOrg bool     `json:"all_org"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(resp.IDs) != 1 || resp.IDs[0] != "doc-1" || resp.AllOrg {
		t.Fatalf("user-a visible = %+v; want ids=[doc-1] all_org=false", resp)
	}

	// user-b sees nothing.
	c, w = newGinCtx("/api/v1/internal/authz/visible?org_id=" + org + "&subject_id=user-b&resource_type=document")
	s.authzVisible(c)
	var respB struct {
		IDs []string `json:"ids"`
	}
	_ = json.Unmarshal(w.Body.Bytes(), &respB)
	if len(respB.IDs) != 0 {
		t.Fatalf("user-b visible = %+v; want empty", respB.IDs)
	}
}
