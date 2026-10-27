package http

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS03-S2: gateway-called Space routes answer a refusal with the status and
// `error` string they always had, plus the additive typed body
// {code, resolvable_by, details}. Service-to-service routes keep plain bodies.

func denialTestContext(method, body string) (*httptest.ResponseRecorder, *gin.Context) {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(method, "/x", bytes.NewBufferString(body))
	c.Request.Header.Set("Content-Type", "application/json")
	c.Params = gin.Params{{Key: "space_ref", Value: "s"}}
	c.Set("org_id", "o")
	c.Set("user_id", "u")
	return w, c
}

func decodeDenialBody(t *testing.T, w *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("body is not JSON: %v (%s)", err, w.Body.String())
	}
	return body
}

func TestSpaceDenialBodyHasTheUnchangedErrorPlusTypedFields(t *testing.T) {
	body := spaceDenialBody("current Space membership required", spaces.AuthorityUnavailableDenial("x"))
	if body["error"] != "current Space membership required" || body["code"] != "space_authority_unavailable" || body["resolvable_by"] != "retry" {
		t.Fatalf("typed body drifted: %v", body)
	}
	if details, ok := body["details"].(map[string]any); !ok || len(details) != 0 {
		t.Fatalf("details must be an (empty) object, got %#v", body["details"])
	}
}

// The ten gateway-called routes (plus thread decision resolvers), against a
// repository with no database, take their fail-closed path: each keeps its
// status and `error` string and gains the typed code.
func TestGatewayCalledRoutesKeepStatusAndErrorAndAddTheCode(t *testing.T) {
	routes := []struct {
		name       string
		handler    func(*Server, *gin.Context)
		body       string
		wantStatus int
		wantError  string
		wantCode   string
	}{
		{"personal thread decision", (*Server).issuePersonalThreadDecision, `{"space_ref":"s","session_key":"k","idempotency_key":"i"}`, 503, "personal Space authority unavailable", "space_authority_unavailable"},
		{"thread decision", (*Server).issueThreadDecision, `{"space_ref":"s","session_key":"k","idempotency_key":"i"}`, 503, "Space authority unavailable", "space_authority_unavailable"},
		{"thread append decision", (*Server).issueThreadAppendDecision, `{"space_ref":"s","thread_id":"t","content_digest":"sha256:d","idempotency_key":"i"}`, 503, "Space authority unavailable", "space_authority_unavailable"},
		{"personal retrieval decision", (*Server).issuePersonalRetrievalDecision, `{"space_ref":"s","idempotency_key":"i"}`, 503, "personal Space retrieval authority unavailable", "space_authority_unavailable"},
		{"retrieval decision", (*Server).issueRetrievalDecision, `{"space_ref":"s","idempotency_key":"i"}`, 503, "Space authority unavailable", "space_authority_unavailable"},
		{"personal import decision", (*Server).issuePersonalImportDecision, `{"space_ref":"s","source_type":"upload","idempotency_key":"i"}`, 503, "personal Space import authority unavailable", "space_authority_unavailable"},
		{"schedule create decision", (*Server).issueScheduleCreateDecision, `{"space_ref":"s","schedule_id":"sc","template_digest":"sha256:t","idempotency_key":"i"}`, 503, "personal Space schedule authority unavailable", "space_authority_unavailable"},
		{"watch create decision", (*Server).issueWatchCreateDecision, `{"space_ref":"s","watch_id":"w","source_kind":"k","source_ref":"r","predicate_digest":"sha256:p","idempotency_key":"i"}`, 503, "Space watch authority unavailable", "space_authority_unavailable"},
	}
	for _, tc := range routes {
		t.Run(tc.name, func(t *testing.T) {
			w, c := denialTestContext(http.MethodPost, tc.body)
			tc.handler(&Server{spaceRepo: &spaces.Repository{}}, c)
			if w.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d (%s)", w.Code, tc.wantStatus, w.Body.String())
			}
			body := decodeDenialBody(t, w)
			if body["error"] != tc.wantError {
				t.Fatalf("error string changed: %v, want %q", body["error"], tc.wantError)
			}
			if body["code"] != tc.wantCode || body["resolvable_by"] != "retry" {
				t.Fatalf("typed fields missing or wrong: %v", body)
			}
			if _, ok := body["details"].(map[string]any); !ok {
				t.Fatalf("details missing: %v", body)
			}
		})
	}
}

func TestMembershipRouteKeepsItsStatusWhenResolutionFails(t *testing.T) {
	// The membership resolver's fail-closed status is a 500, not a 503; a
	// non-refusal failure there stays a plain 500 with its message.
	w, c := denialTestContext(http.MethodGet, "")
	(&Server{spaceRepo: &spaces.Repository{}}).resolveCurrentSpaceMembership(c)
	if w.Code != http.StatusInternalServerError {
		t.Fatalf("status = %d (%s)", w.Code, w.Body.String())
	}
	body := decodeDenialBody(t, w)
	if body["error"] != "Space membership resolution failed" {
		t.Fatalf("error string changed: %v", body)
	}
	if _, typed := body["code"]; typed {
		t.Fatalf("a non-refusal 500 must stay a plain body: %v", body)
	}
}

func TestServiceToServiceRoutesKeepPlainBodies(t *testing.T) {
	// import-execution is called by Imports Core with a service credential, not
	// by a delegated user: its refusals keep the plain {error} body.
	body := `{"intent":{"org_id":"o","space_ref":"s","subject_id":"u","action_schema_hash":"sha256:ingestion-import-v1","payload_digest":"sha256:d","idempotency_key":"i","source_type":"upload"}}`
	w, c := denialTestContext(http.MethodPost, body)
	(&Server{spaceRepo: &spaces.Repository{}}).issuePersonalImportExecutionDecision(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d (%s)", w.Code, w.Body.String())
	}
	got := decodeDenialBody(t, w)
	if got["error"] != "personal Space import authority unavailable" {
		t.Fatalf("error string changed: %v", got)
	}
	for _, field := range []string{"code", "resolvable_by", "details"} {
		if _, ok := got[field]; ok {
			t.Fatalf("service route grew a typed field %q: %v", field, got)
		}
	}
}

func TestWriteSpaceDenialFallsBackToThePlain403WhenTheDiagnosisFails(t *testing.T) {
	// ErrNoCurrentMembership is labelled by DiagnoseDenial; with no database the
	// diagnosis fails, and the plain refusal (still a 403) must stand.
	s := &Server{spaceRepo: &spaces.Repository{}}
	w, c := denialTestContext(http.MethodPost, "")
	if !s.writeSpaceDenial(c, fmt.Errorf("resolve: %w", spaces.ErrNoCurrentMembership), "s", spaces.ExpectAny,
		"current Space authority required", http.StatusServiceUnavailable, "Space authority unavailable") {
		t.Fatal("writeSpaceDenial did not report the refusal as handled")
	}
	if w.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403", w.Code)
	}
	body := decodeDenialBody(t, w)
	if body["error"] != "current Space authority required" {
		t.Fatalf("error = %v", body)
	}
	if _, typed := body["code"]; typed {
		t.Fatalf("a failed diagnosis must not invent a code: %v", body)
	}
}

func TestWriteSpaceDenialMapsTypedAndUntypedFailures(t *testing.T) {
	s := &Server{spaceRepo: &spaces.Repository{}}
	denialErr := fmt.Errorf("evidence: %w", spaces.AuthorityUnavailableDenial("typed message"))

	// A nil error is not a refusal.
	_, c := denialTestContext(http.MethodPost, "")
	if s.writeSpaceDenial(c, nil, "s", spaces.ExpectAny, "m", http.StatusServiceUnavailable, "o") {
		t.Fatal("a nil error was reported as a refusal")
	}

	// A typed denial in the chain keeps otherStatus and the route's own message.
	w, c := denialTestContext(http.MethodPost, "")
	s.writeSpaceDenial(c, denialErr, "s", spaces.ExpectAny, "m", http.StatusForbidden, "route message")
	body := decodeDenialBody(t, w)
	if w.Code != http.StatusForbidden || body["error"] != "route message" || body["code"] != "space_authority_unavailable" {
		t.Fatalf("typed denial: %d %v", w.Code, body)
	}

	// An untyped failure on a 503 path becomes space_authority_unavailable...
	w, c = denialTestContext(http.MethodPost, "")
	s.writeSpaceDenial(c, errors.New("db down"), "s", spaces.ExpectAny, "m", http.StatusServiceUnavailable, "route unavailable")
	body = decodeDenialBody(t, w)
	if w.Code != http.StatusServiceUnavailable || body["error"] != "route unavailable" || body["code"] != "space_authority_unavailable" || body["resolvable_by"] != "retry" {
		t.Fatalf("503 path: %d %v", w.Code, body)
	}
	// ...and on any other status it stays a plain body.
	w, c = denialTestContext(http.MethodPost, "")
	s.writeSpaceDenial(c, errors.New("db down"), "s", spaces.ExpectAny, "m", http.StatusInternalServerError, "route failed")
	body = decodeDenialBody(t, w)
	if w.Code != http.StatusInternalServerError || body["error"] != "route failed" {
		t.Fatalf("500 path: %d %v", w.Code, body)
	}
	if _, typed := body["code"]; typed {
		t.Fatalf("a plain 500 grew a code: %v", body)
	}
}

func TestWriteSpaceIssuerDenialAddsTheCodeOnlyForTypedRefusals(t *testing.T) {
	w, c := denialTestContext(http.MethodPost, "")
	writeSpaceIssuerDenial(c, fmt.Errorf("issue: %w", spaces.AuthorityUnavailableDenial("x")), "Space thread append is not authorized")
	body := decodeDenialBody(t, w)
	if w.Code != http.StatusForbidden || body["error"] != "Space thread append is not authorized" || body["code"] != "space_authority_unavailable" {
		t.Fatalf("typed issuer refusal: %d %v", w.Code, body)
	}

	w, c = denialTestContext(http.MethodPost, "")
	writeSpaceIssuerDenial(c, errors.New("decision_ref is required"), "Space thread append is not authorized")
	body = decodeDenialBody(t, w)
	if w.Code != http.StatusForbidden || body["error"] != "Space thread append is not authorized" {
		t.Fatalf("untyped issuer refusal: %d %v", w.Code, body)
	}
	if _, typed := body["code"]; typed {
		t.Fatalf("an invalid request was labelled as a Space denial: %v", body)
	}
}

// Live: the thread-read own-scheduled fallback. Both resolvers refuse, and a
// typed refusal from the own-scheduled resolver is reported under the
// UNCHANGED membership message and status. Gated on TEST_DATABASE_URL via the
// shared d12 seed helpers.
func TestThreadReadOwnScheduledFallbackReportsThePersonalSpaceDenialLive(t *testing.T) {
	pool := d12Pool(t)
	repo := spaces.NewRepository(&database.DB{Pool: pool})
	d12Seed(t, pool, repo, false) // thread_read_entitled off
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)

	w := d12ReadDecision(t, s, d12Org, d12Owner, d12Personal)
	if w.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want the unchanged 403 (%s)", w.Code, w.Body.String())
	}
	body := decodeDenialBody(t, w)
	if body["error"] != "current shared Space authority required" {
		t.Fatalf("the unchanged message was replaced: %v", body)
	}
	if body["code"] != "space_entitlement_off" || body["resolvable_by"] != "org_admin" {
		t.Fatalf("personal Space entitlement denial not reported: %v", body)
	}
	if details, _ := body["details"].(map[string]any); details["entitlement"] != "thread_read" {
		t.Fatalf("details = %v", body["details"])
	}

	// A non-member peer of the personal Space is just a non-member.
	w = d12ReadDecision(t, s, d12Org, d12Peer, d12Personal)
	body = decodeDenialBody(t, w)
	if w.Code != http.StatusForbidden || body["code"] != "space_membership_required" {
		t.Fatalf("non-member: %d %v", w.Code, body)
	}
}
