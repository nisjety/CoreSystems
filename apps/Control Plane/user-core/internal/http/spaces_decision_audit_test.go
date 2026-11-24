package http

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS05-S5: the group middleware that turns issuer outcomes into audit rows.

type capturedAudit struct {
	eventID string
	subject string
	payload string
}

type auditRecorder struct {
	mu   sync.Mutex
	rows []capturedAudit
	err  error
}

func (r *auditRecorder) EnqueueAudit(_ context.Context, eventID, subject string, payload []byte) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.err != nil {
		return r.err
	}
	r.rows = append(r.rows, capturedAudit{eventID, subject, string(payload)})
	return nil
}

func (r *auditRecorder) count() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.rows)
}

// auditHarness mounts the real middleware on a group with one probe route per
// path. The handler answers whatever status/body the test set on the request
// through headers, and echoes the request body it received.
type auditHarness struct {
	engine   *gin.Engine
	recorder *auditRecorder
	gotBody  string
}

func newAuditHarness(t *testing.T, cfg spaces.DecisionAuditConfig, wireOutbox bool) *auditHarness {
	t.Helper()
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	h := &auditHarness{recorder: &auditRecorder{}}
	if wireOutbox {
		server.SetSpaceDecisionAudit(cfg, h.recorder)
	} else {
		server.SetSpaceDecisionAudit(cfg, nil)
	}
	h.engine = gin.New()
	group := h.engine.Group("/api/v1/internal/spaces")
	// Stand-in for service auth: the probe header picks the verified caller.
	group.Use(func(c *gin.Context) {
		switch c.GetHeader("X-Probe-Auth") {
		case "delegated":
			c.Set("auth_method", "service_principal")
			c.Set("service_id", "verevon-gateway")
			c.Set("delegation_verified", true)
			c.Set("org_id", "org-d")
			c.Set("user_id", "user-d")
		case "service":
			c.Set("auth_method", "service_principal")
			c.Set("service_id", "capability-core")
		}
		c.Next()
	})
	group.Use(server.spaceDecisionAuditMiddleware())
	probe := func(c *gin.Context) {
		raw, _ := io.ReadAll(c.Request.Body)
		h.gotBody = string(raw)
		if c.GetHeader("X-Probe-Abort") == "1" {
			c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "scope guard"})
			return
		}
		status := http.StatusOK
		switch c.GetHeader("X-Probe-Status") {
		case "403":
			status = http.StatusForbidden
		case "400":
			status = http.StatusBadRequest
		case "409":
			status = http.StatusConflict
		case "503":
			status = http.StatusServiceUnavailable
		}
		c.Data(status, "application/json", []byte(c.GetHeader("X-Probe-Body")))
	}
	for _, path := range []string{
		"/thread-decision", "/thread-read-decision", "/retrieval-decision", "/thread-append-decision",
		"/watch-observe-decision", "/run-control-decision", "/import-decision", "/schedule-create-decision", "/membership",
		"/thread-read-decisions",
	} {
		group.POST(path, probe)
	}
	group.GET("/thread-decision", probe)
	return h
}

func (h *auditHarness) post(path, body string, headers map[string]string) *httptest.ResponseRecorder {
	req := httptest.NewRequest(http.MethodPost, "/api/v1/internal/spaces"+path, strings.NewReader(body))
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	rec := httptest.NewRecorder()
	h.engine.ServeHTTP(rec, req)
	return rec
}

const auditRequestBody = `{"org_id":"org-1","subject_id":"user-1","space_ref":"space-1","action_id":"model.run.create","idempotency_key":"idem-secret","payload_digest":"sha256:digestsecret"}`

const issuedBody = `{"data":{"decision":{"decision_ref":"dec-1","org_id":"org-1","space_ref":"space-1","subject_id":"user-1","service_audience":"model-plane","action_id":"model.run.control","authority_revision":3,"expires_at":"2030-01-01T00:00:00Z","zero_data_retention":true},"token":"tok-secret-value"}}`

const deniedBody = `{"code":"space_role_insufficient","resolvable_by":"space_manager","error":"nope"}`

var (
	enabledDeniedOnly = spaces.DecisionAuditConfig{Enabled: true, Denied: true}
	enabledBoth       = spaces.DecisionAuditConfig{Enabled: true, Denied: true, Issued: true}
)

func TestAuditMiddlewareIsAPassThroughWhenOffOrUnwired(t *testing.T) {
	cases := []struct {
		name   string
		cfg    spaces.DecisionAuditConfig
		wire   bool
		method string
	}{
		{"flag off", spaces.DecisionAuditConfig{}, true, http.MethodPost},
		{"outbox nil", enabledBoth, false, http.MethodPost},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newAuditHarness(t, tc.cfg, tc.wire)
			rec := h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
			if rec.Code != http.StatusForbidden || rec.Body.String() != deniedBody {
				t.Fatalf("response changed: %d %s", rec.Code, rec.Body.String())
			}
			if h.recorder.count() != 0 {
				t.Fatalf("a pass-through wrote %d rows", h.recorder.count())
			}
			if h.gotBody != auditRequestBody {
				t.Fatalf("handler body = %q", h.gotBody)
			}
		})
	}
}

func TestAuditMiddlewareWritesOneDeniedRowForAHandlerRefusal(t *testing.T) {
	h := newAuditHarness(t, enabledDeniedOnly, true)
	rec := h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	if rec.Code != http.StatusForbidden || rec.Body.String() != deniedBody {
		t.Fatalf("response must be unchanged: %d %s", rec.Code, rec.Body.String())
	}
	if h.recorder.count() != 1 {
		t.Fatalf("rows = %d, want 1", h.recorder.count())
	}
	row := h.recorder.rows[0]
	if !strings.HasPrefix(row.eventID, "space-denial:") || row.subject != spaces.AuditSubjectSpaceDecisionDenied {
		t.Fatalf("row = %+v", row)
	}
	for _, want := range []string{`"org_id":"org-1"`, `"user_id":"user-1"`, `"space:space-1"`, `"reason":"space_role_insufficient"`, `"resolvable_by":"space_manager"`, `"decision_kind":"thread_create"`, `"outcome":"denied"`} {
		if !strings.Contains(row.payload, want) {
			t.Errorf("payload lacks %s: %s", want, row.payload)
		}
	}
	for _, secret := range []string{"idem-secret", "digestsecret", "tok-secret"} {
		if strings.Contains(row.payload, secret) {
			t.Errorf("payload leaks %q", secret)
		}
	}
	if h.gotBody != auditRequestBody {
		t.Fatalf("request body must be restored for the handler, got %q", h.gotBody)
	}
}

func TestAuditMiddlewareReadsIdsNestedUnderIntent(t *testing.T) {
	h := newAuditHarness(t, enabledDeniedOnly, true)
	body := `{"intent":{"org_id":"org-9","subject_id":"user-9","space_ref":"space-9","action_id":"a.b"}}`
	h.post("/thread-decision", body, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	if h.recorder.count() != 1 || !strings.Contains(h.recorder.rows[0].payload, `"org_id":"org-9"`) {
		t.Fatalf("rows = %+v", h.recorder.rows)
	}
}

func TestAuditMiddlewareIgnoresScopeGuardAbortsAndNonRefusals(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Abort": "1"})
	h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Status": "400", "X-Probe-Body": `{"error":"bad"}`})
	h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Abort": "1"})
	h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Status": "400", "X-Probe-Body": `{"error":"bad"}`})
	if h.recorder.count() != 0 {
		t.Fatalf("aborted chain and 400 wrote %d rows: %+v", h.recorder.count(), h.recorder.rows)
	}
}

// B-REC-03: availability (503) and conflict (409) refusals are audited too,
// in either refusal body shape, and the response is never changed.
func TestAuditMiddlewareAuditsAvailabilityAndConflictRefusals(t *testing.T) {
	h := newAuditHarness(t, enabledDeniedOnly, true)
	unavailable := `{"error":"Space authority repository unavailable"}`
	rec := h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Status": "503", "X-Probe-Body": unavailable})
	if rec.Code != http.StatusServiceUnavailable || rec.Body.String() != unavailable {
		t.Fatalf("503 must be unchanged: %d %s", rec.Code, rec.Body.String())
	}
	conflict := `{"error":{"code":"space_ref_conflict","message":"x","details":{"resolvable_by":"space_owner"}}}`
	rec = h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Status": "409", "X-Probe-Body": conflict})
	if rec.Code != http.StatusConflict || rec.Body.String() != conflict {
		t.Fatalf("409 must be unchanged: %d %s", rec.Code, rec.Body.String())
	}
	if h.recorder.count() != 2 {
		t.Fatalf("rows = %d, want 2: %+v", h.recorder.count(), h.recorder.rows)
	}
	first, second := h.recorder.rows[0].payload, h.recorder.rows[1].payload
	for _, want := range []string{`"status":503`, `"outcome":"error"`, `"decision_kind":"thread_create"`} {
		if !strings.Contains(first, want) {
			t.Errorf("503 row lacks %s: %s", want, first)
		}
	}
	for _, want := range []string{`"status":409`, `"outcome":"denied"`, `"reason":"space_ref_conflict"`, `"resolvable_by":"space_owner"`, `"decision_kind":"run_control"`} {
		if !strings.Contains(second, want) {
			t.Errorf("409 row lacks %s: %s", want, second)
		}
	}
	if h.recorder.rows[0].subject != spaces.AuditSubjectSpaceDecisionDenied || h.recorder.rows[1].subject != spaces.AuditSubjectSpaceDecisionDenied {
		t.Fatalf("subjects = %+v", h.recorder.rows)
	}
}

// B-REC-03: each denied row of a 200 batch answer is one denial row; issued
// rows (thread reads) and their tokens are never audited.
func TestAuditMiddlewareAuditsBatchRowDenials(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	batch := `{"data":{"results":[` +
		`{"space_ref":"space-a","status":"issued","decision":{"decision_ref":"d-a"},"token":"tok-secret-a"},` +
		`{"space_ref":"space-b","status":"denied","reason":"not_member"},` +
		`{"space_ref":"space-c","status":"denied","reason":"unavailable"}]}}`
	rec := h.post("/thread-read-decisions", `{"items":[]}`, map[string]string{"X-Probe-Body": batch, "X-Probe-Auth": "delegated"})
	if rec.Code != http.StatusOK || rec.Body.String() != batch {
		t.Fatalf("batch answer must be unchanged: %d %s", rec.Code, rec.Body.String())
	}
	if h.recorder.count() != 2 {
		t.Fatalf("rows = %d, want 2 (one per denied row): %+v", h.recorder.count(), h.recorder.rows)
	}
	b, c := h.recorder.rows[0], h.recorder.rows[1]
	if b.eventID == c.eventID {
		t.Fatal("two denied rows must be two events")
	}
	for _, want := range []string{`"space:space-b"`, `"reason":"not_member"`, `"outcome":"denied"`, `"batch_row":true`, `"org_id":"org-d"`, `"user_id":"user-d"`, `"actor_role":"delegated_user"`, `"decision_kind":"thread_read"`} {
		if !strings.Contains(b.payload, want) {
			t.Errorf("row b lacks %s: %s", want, b.payload)
		}
	}
	if !strings.Contains(c.payload, `"reason":"unavailable"`) || !strings.Contains(c.payload, `"outcome":"error"`) {
		t.Errorf("row c = %s", c.payload)
	}
	for _, row := range h.recorder.rows {
		if strings.Contains(row.payload, "tok-secret") || strings.Contains(row.payload, "space-a") {
			t.Errorf("a batch row leaks the issued row: %s", row.payload)
		}
	}

	// The batch route's own whole-batch refusals are audited like any route.
	h.recorder.rows = nil
	h.post("/thread-read-decisions", `{"items":[]}`, map[string]string{"X-Probe-Status": "503", "X-Probe-Body": `{"error":"down"}`, "X-Probe-Auth": "delegated"})
	if h.recorder.count() != 1 || !strings.Contains(h.recorder.rows[0].payload, `"status":503`) {
		t.Fatalf("batch 503 rows = %+v", h.recorder.rows)
	}
}

// B-REC-03: every row names who the call spoke for.
func TestAuditMiddlewareSetsActorRole(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody, "X-Probe-Auth": "delegated"})
	h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": issuedBody, "X-Probe-Auth": "service"})
	if h.recorder.count() != 2 {
		t.Fatalf("rows = %+v", h.recorder.rows)
	}
	if !strings.Contains(h.recorder.rows[0].payload, `"actor_role":"delegated_user"`) {
		t.Errorf("denied row = %s", h.recorder.rows[0].payload)
	}
	if !strings.Contains(h.recorder.rows[1].payload, `"actor_role":"service"`) {
		t.Errorf("issued row = %s", h.recorder.rows[1].payload)
	}
}

func TestAuditMiddlewareIssuedRowsAreEffectBoundOnly(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	headers := map[string]string{"X-Probe-Body": issuedBody}

	rec := h.post("/run-control-decision", auditRequestBody, headers)
	if rec.Code != http.StatusOK || rec.Body.String() != issuedBody {
		t.Fatalf("response must be unchanged: %d %s", rec.Code, rec.Body.String())
	}
	if h.recorder.count() != 1 {
		t.Fatalf("effect-bound 200 must write one row, got %d", h.recorder.count())
	}
	row := h.recorder.rows[0]
	if row.eventID != "space-decision:dec-1" || row.subject != spaces.AuditSubjectSpaceDecisionIssued {
		t.Fatalf("row = %+v", row)
	}
	if strings.Contains(row.payload, "tok-secret-value") || strings.Contains(row.payload, "token") {
		t.Fatalf("issued payload must not carry the token: %s", row.payload)
	}
	if !strings.Contains(row.payload, `"service_audience":"model-plane"`) || !strings.Contains(row.payload, `"outcome":"ok"`) {
		t.Fatalf("payload = %s", row.payload)
	}

	h.recorder.rows = nil
	for _, path := range []string{"/thread-read-decision", "/retrieval-decision", "/thread-append-decision", "/watch-observe-decision", "/thread-decision"} {
		h.post(path, auditRequestBody, headers)
	}
	if h.recorder.count() != 0 {
		t.Fatalf("read/retrieval/append/observe/create 200s must write nothing, wrote %+v", h.recorder.rows)
	}

	// import and schedule-create are effect-bound too.
	h.post("/import-decision", auditRequestBody, headers)
	h.post("/schedule-create-decision", auditRequestBody, headers)
	if h.recorder.count() != 2 {
		t.Fatalf("import + schedule-create issued rows = %d, want 2", h.recorder.count())
	}
}

func TestAuditMiddlewareKindsSelectWhatIsWritten(t *testing.T) {
	issuedOnly := spaces.DecisionAuditConfig{Enabled: true, Issued: true}
	h := newAuditHarness(t, issuedOnly, true)
	h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	if h.recorder.count() != 0 {
		t.Fatal("issued-only must not write a denial row")
	}
	h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": issuedBody})
	if h.recorder.count() != 1 {
		t.Fatalf("issued-only must write the issued row, got %d", h.recorder.count())
	}

	h = newAuditHarness(t, enabledDeniedOnly, true)
	h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": issuedBody})
	if h.recorder.count() != 0 {
		t.Fatal("denied-only must not write an issued row")
	}
}

func TestAuditMiddlewareIgnoresNonIssuerPathsAndMethods(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	h.post("/membership", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	req := httptest.NewRequest(http.MethodGet, "/api/v1/internal/spaces/thread-decision", nil)
	req.Header.Set("X-Probe-Status", "403")
	h.engine.ServeHTTP(httptest.NewRecorder(), req)
	if h.recorder.count() != 0 {
		t.Fatalf("a non-issuer path or a GET wrote %+v", h.recorder.rows)
	}
}

// B-REC-03: an outbox failure never changes a refusal, but an effect-bound
// issue fails closed: the decision and token are withheld behind a typed 503.
func TestAuditMiddlewareOutboxFailureFailsAnEffectBoundIssueClosed(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	h.recorder.err = errors.New("outbox down")

	rec := h.post("/thread-decision", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	if rec.Code != http.StatusForbidden || rec.Body.String() != deniedBody {
		t.Fatalf("a refusal must stay a refusal: %d %s", rec.Code, rec.Body.String())
	}
	rec = h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	if rec.Code != http.StatusForbidden || rec.Body.String() != deniedBody {
		t.Fatalf("an effect-bound refusal must stay a refusal: %d %s", rec.Code, rec.Body.String())
	}
	rec = h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": issuedBody})
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("an unaudited effect-bound issue must fail closed, got %d %s", rec.Code, rec.Body.String())
	}
	if strings.Contains(rec.Body.String(), "tok-secret-value") || strings.Contains(rec.Body.String(), "dec-1") {
		t.Fatalf("the withheld decision leaked: %s", rec.Body.String())
	}
	if !strings.Contains(rec.Body.String(), `"code":"space_decision_audit_unavailable"`) || !strings.Contains(rec.Body.String(), `"details":{}`) {
		t.Fatalf("refusal must be the typed envelope: %s", rec.Body.String())
	}

	// A non-effect-bound 200 is not held and passes through.
	rec = h.post("/thread-read-decision", auditRequestBody, map[string]string{"X-Probe-Body": issuedBody})
	if rec.Code != http.StatusOK || rec.Body.String() != issuedBody {
		t.Fatalf("a read issue must pass: %d %s", rec.Code, rec.Body.String())
	}
}

// B-REC-03: a decision whose row cannot even be built (no decision_ref) fails
// closed too; a 200 that carries no decision is released untouched.
func TestAuditMiddlewareUnbuildableIssuedRowFailsClosed(t *testing.T) {
	h := newAuditHarness(t, enabledBoth, true)
	noRef := `{"data":{"decision":{"org_id":"org-1","space_ref":"space-1"},"token":"tok-secret-value"}}`
	rec := h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": noRef})
	if rec.Code != http.StatusServiceUnavailable || strings.Contains(rec.Body.String(), "tok-secret-value") {
		t.Fatalf("unbuildable issued row must fail closed: %d %s", rec.Code, rec.Body.String())
	}
	other := `{"data":{"status":"ok"}}`
	rec = h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": other})
	if rec.Code != http.StatusOK || rec.Body.String() != other {
		t.Fatalf("a 200 without a decision must pass: %d %s", rec.Code, rec.Body.String())
	}
	if h.recorder.count() != 0 {
		t.Fatalf("rows = %+v", h.recorder.rows)
	}
}

// With the issued kind off, an effect-bound route is not held: an outbox
// failure cannot affect the issue (the rollout switch is never authority).
func TestAuditMiddlewareDeniedOnlyNeverHoldsAnIssue(t *testing.T) {
	h := newAuditHarness(t, enabledDeniedOnly, true)
	h.recorder.err = errors.New("outbox down")
	rec := h.post("/run-control-decision", auditRequestBody, map[string]string{"X-Probe-Body": issuedBody})
	if rec.Code != http.StatusOK || rec.Body.String() != issuedBody {
		t.Fatalf("denied-only must not hold an issue: %d %s", rec.Code, rec.Body.String())
	}
}

func TestAuditMiddlewareSkipsARefusalWithoutAnOrg(t *testing.T) {
	h := newAuditHarness(t, enabledDeniedOnly, true)
	rec := h.post("/thread-decision", `{"space_ref":"space-1"}`, map[string]string{"X-Probe-Status": "403", "X-Probe-Body": deniedBody})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d", rec.Code)
	}
	if h.recorder.count() != 0 {
		t.Fatal("a denial with no org cannot be built and must be dropped, not written")
	}
}

// The group middleware registers no route: the route table is the same with the
// producer wired or not (WS05-S5; the golden in routes_spaces_test.go pins the
// table itself).
func TestAuditMiddlewareAddsNoRoutes(t *testing.T) {
	gin.SetMode(gin.TestMode)
	plain := NewServer(nil, nil, nil, nil, "0")
	wired := NewServer(nil, nil, nil, nil, "0")
	wired.SetSpaceDecisionAudit(enabledBoth, &auditRecorder{})
	a, b := routeCounts(plain.router.Routes()), routeCounts(wired.router.Routes())
	if len(a) == 0 || len(a) != len(b) {
		t.Fatalf("route tables differ: %d vs %d", len(a), len(b))
	}
	for route, n := range a {
		if b[route] != n {
			t.Errorf("%s: %d vs %d", route, n, b[route])
		}
	}
}
