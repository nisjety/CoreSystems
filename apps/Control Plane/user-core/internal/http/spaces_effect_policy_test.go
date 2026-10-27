package http

import (
	"bytes"
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/jackc/pgx/v5"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS03-S1: the operator PUT /api/v1/internal/spaces/effect-policy handler.
// A key the caller did not state (absent or JSON null) keeps its stored value,
// and every write is attributed to the control-space-policy service principal.

func putEffectPolicy(s *Server, body string) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodPut, "/api/v1/internal/spaces/effect-policy", bytes.NewBufferString(body))
	c.Request.Header.Set("Content-Type", "application/json")
	s.upsertSpaceEffectPolicy(c)
	return w
}

func TestUpsertSpaceEffectPolicyFailsClosedWithoutARepository(t *testing.T) {
	w := putEffectPolicy(&Server{}, `{"org_id":"o"}`)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired repository: want 503, got %d (%s)", w.Code, w.Body.String())
	}
}

func TestUpsertSpaceEffectPolicyRejectsMalformedBodiesBeforeTheRepository(t *testing.T) {
	s := &Server{spaceRepo: &spaces.Repository{}}
	for name, body := range map[string]string{
		"not json":               `nope`,
		"json array":             `[1,2]`,
		"wrong entitlement type": `{"org_id":"o","thread_read_entitled":"yes"}`,
		"entitlement as number":  `{"org_id":"o","import_write_entitled":1}`,
	} {
		w := putEffectPolicy(s, body)
		if w.Code != http.StatusBadRequest {
			t.Errorf("%s: want 400, got %d (%s)", name, w.Code, w.Body.String())
		}
	}
}

func TestControlSpacePolicyPrincipalNameIsStable(t *testing.T) {
	// The actor recorded on every operator policy change, and the service
	// principal the scope table and authz facade name.
	if controlSpacePolicyPrincipal != "control-space-policy" {
		t.Fatalf("operator principal renamed to %q; the audit trail and plane-service-principals key on it", controlSpacePolicyPrincipal)
	}
	if scope := serviceScopeFor(http.MethodPut, "/api/v1/internal/spaces/effect-policy"); scope != "spaces:policy:write" {
		t.Fatalf("effect-policy scope = %q, want spaces:policy:write", scope)
	}
}

func serviceScopeFor(method, path string) string {
	return serviceScopeForRequest(httptest.NewRequest(method, path, nil))
}

type httpTxAuditOutbox struct{}

func (httpTxAuditOutbox) EnqueueAuditTx(ctx context.Context, tx pgx.Tx, eventID, subject string, payload []byte) error {
	_, err := tx.Exec(ctx, `
		INSERT INTO user_audit_outbox (event_id, subject, payload)
		VALUES ($1, $2, $3::jsonb) ON CONFLICT (event_id) DO NOTHING`, eventID, subject, payload)
	return err
}

// Live: drives the real handler against a real repository. Gated on
// TEST_DATABASE_URL via the shared d12Pool helper.
func TestUpsertSpaceEffectPolicyTreatsAbsentAndNullEntitlementsAsNotStatedLive(t *testing.T) {
	pool := d12Pool(t)
	ctx := context.Background()
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	repo.SetPolicyAuditOutbox(httpTxAuditOutbox{})
	s := &Server{}
	s.SetSpaceRepository(repo)

	org := fmt.Sprintf("ws03s1-http-%d", time.Now().UnixNano())
	t.Cleanup(func() {
		_, _ = pool.Exec(ctx, `DELETE FROM space_effect_policies WHERE org_id = $1`, org)
		_, _ = pool.Exec(ctx, `DELETE FROM user_audit_outbox WHERE payload->>'org_id' = $1`, org)
	})
	privacy := `"org_id":"` + org + `","privacy_policy_ref":"privacy:` + org + `:1","purpose":"assistant_collaboration",` +
		`"lawful_basis":"contract","privacy_class":"internal","retention_class":"standard","residency":"swedencentral","deletion_scope":"space"`

	// An incomplete policy (no privacy_policy_ref) is a 400, never stored.
	if w := putEffectPolicy(s, `{"org_id":"`+org+`"}`); w.Code != http.StatusBadRequest {
		t.Fatalf("incomplete policy: want 400, got %d %s", w.Code, w.Body.String())
	}

	// Create with two bits on.
	if w := putEffectPolicy(s, `{`+privacy+`,"thread_create_entitled":true,"retrieval_read_entitled":true}`); w.Code != http.StatusOK {
		t.Fatalf("create: %d %s", w.Code, w.Body.String())
	}
	read := func() map[string]bool {
		resolved, err := repo.ResolveOrgEffectPolicy(ctx, org)
		if err != nil {
			t.Fatalf("resolve policy: %v", err)
		}
		return spaces.EntitlementValues(resolved)
	}
	if got := read(); !got["thread_create"] || !got["retrieval_read"] {
		t.Fatalf("created policy lost its bits: %v", got)
	}

	// A re-run that states only thread_read: absent keys keep their value...
	if w := putEffectPolicy(s, `{`+privacy+`,"thread_read_entitled":true}`); w.Code != http.StatusOK {
		t.Fatalf("absent keys: %d %s", w.Code, w.Body.String())
	}
	if got := read(); !got["thread_create"] || !got["retrieval_read"] || !got["thread_read"] {
		t.Fatalf("an absent key was cleared: %v", got)
	}
	// ...and so do keys sent as null.
	if w := putEffectPolicy(s, `{`+privacy+`,"thread_create_entitled":null,"retrieval_read_entitled":null,"import_write_entitled":true}`); w.Code != http.StatusOK {
		t.Fatalf("null keys: %d %s", w.Code, w.Body.String())
	}
	if got := read(); !got["thread_create"] || !got["retrieval_read"] || !got["import_write"] {
		t.Fatalf("a null key was cleared: %v", got)
	}
	// An explicit false clears.
	if w := putEffectPolicy(s, `{`+privacy+`,"thread_create_entitled":false}`); w.Code != http.StatusOK {
		t.Fatalf("explicit false: %d %s", w.Code, w.Body.String())
	}
	if read()["thread_create"] {
		t.Fatal("an explicit false did not clear the bit")
	}

	// Every change is attributed to the operator service principal.
	rows, err := pool.Query(ctx, `SELECT DISTINCT actor_subject_type, actor_subject_id FROM space_effect_policy_events WHERE org_id = $1`, org)
	if err != nil {
		t.Fatalf("read events: %v", err)
	}
	defer rows.Close()
	seen := 0
	for rows.Next() {
		var actorType, actorID string
		if err := rows.Scan(&actorType, &actorID); err != nil {
			t.Fatal(err)
		}
		if actorType != "service" || actorID != "control-space-policy" {
			t.Fatalf("event actor = %s/%s, want service/control-space-policy", actorType, actorID)
		}
		seen++
	}
	if seen != 1 {
		t.Fatalf("expected a single distinct actor, saw %d", seen)
	}
}
