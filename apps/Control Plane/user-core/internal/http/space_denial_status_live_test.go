package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// D1-214: a gateway-called personal-Space resolver path answers a TYPED
// refusal (entitlement off, role floor, kind unsupported) with the 403 the
// denial vocabulary assigns it, not the 503 the route used before the
// refusals were typed. 503 stays for real unavailability. The body code is
// the same either way. Gated on TEST_DATABASE_URL via the shared d12 helpers.

var personalResolverRoutes = []struct {
	name    string
	handler func(*Server, *gin.Context)
	body    func(spaceRef string) string
}{
	{"personal thread decision", (*Server).issuePersonalThreadDecision, func(ref string) string {
		return `{"space_ref":"` + ref + `","session_key":"k","idempotency_key":"i"}`
	}},
	{"personal retrieval decision", (*Server).issuePersonalRetrievalDecision, func(ref string) string {
		return `{"space_ref":"` + ref + `","idempotency_key":"i"}`
	}},
	{"personal import decision", (*Server).issuePersonalImportDecision, func(ref string) string {
		return `{"space_ref":"` + ref + `","source_type":"upload","idempotency_key":"i"}`
	}},
	{"schedule create decision", (*Server).issueScheduleCreateDecision, func(ref string) string {
		return `{"space_ref":"` + ref + `","schedule_id":"sc","template_digest":"sha256:t","idempotency_key":"i"}`
	}},
	{"watch create decision", (*Server).issueWatchCreateDecision, func(ref string) string {
		return `{"space_ref":"` + ref + `","watch_id":"w","source_kind":"k","source_ref":"r","predicate_digest":"sha256:p","idempotency_key":"i"}`
	}},
}

func driveSpaceRoute(s *Server, handler func(*Server, *gin.Context), body, org, user string) *httptest.ResponseRecorder {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodPost, "/x", bytes.NewBufferString(body))
	c.Request.Header.Set("Content-Type", "application/json")
	c.Set("org_id", org)
	c.Set("user_id", user)
	handler(s, c)
	return w
}

func TestPersonalResolverPathsAnswerTypedRefusalsWith403Live(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, false) // no effect entitlement is on
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)

	for _, route := range personalResolverRoutes {
		t.Run(route.name, func(t *testing.T) {
			// The owner of their own personal Space, but the org's entitlement is off.
			w := driveSpaceRoute(s, route.handler, route.body(d12Personal), d12Org, d12Owner)
			body := decodeDenialBody(t, w)
			if w.Code != http.StatusForbidden {
				t.Fatalf("entitlement off: status = %d, want 403 (%s)", w.Code, w.Body.String())
			}
			if body["code"] != "space_entitlement_off" || body["resolvable_by"] != "org_admin" {
				t.Fatalf("entitlement off: the body code changed: %v", body)
			}

			// A room is the wrong kind for a personal-Space route.
			w = driveSpaceRoute(s, route.handler, route.body(d12Room), d12Org, d12Owner)
			body = decodeDenialBody(t, w)
			if w.Code != http.StatusForbidden || body["code"] != "space_kind_unsupported" {
				t.Fatalf("kind unsupported: %d %v", w.Code, body)
			}

			// A peer who is not in the personal Space is a plain non-member.
			w = driveSpaceRoute(s, route.handler, route.body(d12Personal), d12Org, d12Peer)
			body = decodeDenialBody(t, w)
			if w.Code != http.StatusForbidden || body["code"] != "space_membership_required" {
				t.Fatalf("non-member: %d %v", w.Code, body)
			}
		})
	}
}

// Real unavailability is still a 503 with the retryable code: a repository
// that cannot reach its database is not a refusal.
func TestPersonalResolverPathsKeep503ForRealUnavailabilityLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, false)
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)
	pool.Close()

	for _, route := range personalResolverRoutes {
		t.Run(route.name, func(t *testing.T) {
			w := driveSpaceRoute(s, route.handler, route.body(d12Personal), d12Org, d12Owner)
			body := decodeDenialBody(t, w)
			if w.Code != http.StatusServiceUnavailable || body["code"] != string(spaces.DenialAuthorityUnavailable) || body["resolvable_by"] != "retry" {
				t.Fatalf("unavailable: %d %v", w.Code, body)
			}
		})
	}
}
