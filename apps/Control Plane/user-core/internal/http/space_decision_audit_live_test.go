package http

import (
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// WS05-S5 live proof (DoD S5): a viewer's denied thread decision and an issued
// schedule-create decision both reach user_audit_outbox through the real
// middleware + real repository + real SpaceAuditEnqueuer.
//
// Gated on TEST_DATABASE_URL (disposable Postgres with user-core migrations).
// The second leg, delivery into audit-core's audit_events within 10 s, needs the
// full stack (NATS + audit-core) and is gated additionally on
// TEST_AUDIT_EVENTS_DATABASE_URL (the audit-core database); it skips without it.

func auditLiveEngine(t *testing.T, pool *pgxpool.Pool, repo *spaces.Repository) *gin.Engine {
	t.Helper()
	gin.SetMode(gin.TestMode)
	s := NewServer(nil, nil, nil, nil, "0")
	s.SetSpaceRepository(repo)
	s.SetSpaceDecisionAudit(
		spaces.DecisionAuditConfig{Enabled: true, Denied: true, Issued: true},
		users.NewSpaceAuditEnqueuer(users.NewRepository(&database.DB{Pool: pool})),
	)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	group.Use(func(c *gin.Context) {
		c.Set("org_id", d12Org)
		c.Set("user_id", c.GetHeader("X-Test-User"))
		c.Next()
	})
	group.Use(s.spaceDecisionAuditMiddleware())
	group.POST("/thread-decision", s.issueThreadDecision)
	group.POST("/schedule-create-decision", s.issueScheduleCreateDecision)
	return engine
}

func auditLivePost(engine *gin.Engine, path, user, body string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces"+path, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Test-User", user)
	rec := httptest.NewRecorder()
	engine.ServeHTTP(rec, req)
	return rec
}

func TestSpaceDecisionAuditReachesTheOutboxLive(t *testing.T) {
	pool := d12Pool(t)
	ctx := context.Background()
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, false)
	d12SigningEnv(t)
	// Schedule creation needs its own entitlement; thread creation too so the
	// viewer is refused on role, not on a missing entitlement.
	if _, err := repo.UpsertEffectPolicy(ctx, spaces.EffectPolicy{
		OrgID: d12Org, PrivacyPolicyRef: "privacy:d12-org:1", Purpose: "assistant_collaboration",
		LawfulBasis: "contract", PrivacyClass: "internal", RetentionClass: "standard",
		Residency: "swedencentral", DeletionScope: "space",
		ThreadCreateEntitled: true, ScheduleFireEntitled: true,
	}, spaces.EffectPolicyActor{Type: "service", ID: "d12-test"}); err != nil {
		t.Fatalf("seed entitlements: %v", err)
	}
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM user_audit_outbox WHERE payload->>'org_id' = $1 AND subject LIKE '%space_decision_%'`, d12Org)
	})
	_, _ = pool.Exec(ctx, `DELETE FROM user_audit_outbox WHERE payload->>'org_id' = $1 AND subject LIKE '%space_decision_%'`, d12Org)

	engine := auditLiveEngine(t, pool, repo)

	// A viewer's thread decision in the room is refused and audited as denied.
	rec := auditLivePost(engine, "/thread-decision", d12Peer,
		`{"space_ref":"`+d12Room+`","session_key":"k","idempotency_key":"s5-deny-1"}`)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("viewer thread decision: status = %d, want 403 (%s)", rec.Code, rec.Body.String())
	}
	var denied int
	if err := pool.QueryRow(ctx, `
		SELECT count(*) FROM user_audit_outbox
		WHERE payload->>'org_id' = $1 AND subject = $2 AND payload->>'user_id' = $3`,
		d12Org, spaces.AuditSubjectSpaceDecisionDenied, d12Peer).Scan(&denied); err != nil {
		t.Fatalf("count denied rows: %v", err)
	}
	if denied != 1 {
		t.Fatalf("denied outbox rows = %d, want 1", denied)
	}

	// The same denial inside the same minute collapses to the same event id.
	auditLivePost(engine, "/thread-decision", d12Peer,
		`{"space_ref":"`+d12Room+`","session_key":"k","idempotency_key":"s5-deny-2"}`)
	if err := pool.QueryRow(ctx, `
		SELECT count(*) FROM user_audit_outbox
		WHERE payload->>'org_id' = $1 AND subject = $2`, d12Org, spaces.AuditSubjectSpaceDecisionDenied).Scan(&denied); err != nil {
		t.Fatal(err)
	}
	if denied != 1 {
		t.Fatalf("a retry in the same minute made %d rows, want 1", denied)
	}

	// The owner's schedule-create decision in their personal Space is issued and
	// audited as issued, with no token in the row.
	rec = auditLivePost(engine, "/schedule-create-decision", d12Owner,
		`{"space_ref":"`+d12Personal+`","schedule_id":"sch_s5","template_digest":"sha256:`+strings.Repeat("a", 64)+`","idempotency_key":"s5-issue-1"}`)
	if rec.Code != http.StatusOK {
		t.Fatalf("owner schedule-create: status = %d, want 200 (%s)", rec.Code, rec.Body.String())
	}
	var payload string
	if err := pool.QueryRow(ctx, `
		SELECT payload::text FROM user_audit_outbox
		WHERE payload->>'org_id' = $1 AND subject = $2`, d12Org, spaces.AuditSubjectSpaceDecisionIssued).Scan(&payload); err != nil {
		t.Fatalf("issued outbox row: %v", err)
	}
	if strings.Contains(payload, "token") || strings.Contains(payload, "template_digest") {
		t.Fatalf("issued row must stay content-free: %s", payload)
	}

	// Leg two: audit-core received both within 10 s (full stack only).
	auditDSN := os.Getenv("TEST_AUDIT_EVENTS_DATABASE_URL")
	if auditDSN == "" {
		t.Log("TEST_AUDIT_EVENTS_DATABASE_URL not set; skipping the audit_events delivery leg")
		return
	}
	auditPool, err := pgxpool.New(ctx, auditDSN)
	if err != nil {
		t.Fatalf("connect audit db: %v", err)
	}
	defer auditPool.Close()
	deadline := time.Now().Add(10 * time.Second)
	for {
		var n int
		if err := auditPool.QueryRow(ctx, `
			SELECT count(*) FROM audit_events
			WHERE org_id = $1 AND event IN ('space_decision_denied','space_decision_issued')`, d12Org).Scan(&n); err == nil && n >= 2 {
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("audit_events did not receive both Space decision rows within 10s")
		}
		time.Sleep(500 * time.Millisecond)
	}
}
