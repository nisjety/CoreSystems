package http

import (
	"net/http"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

// D1-214 follow-on: the service-to-service resolvers (import-execution,
// schedule-fire, scheduled-run, scheduled-run-execution, sandbox-capability,
// watch-observe) answer a TYPED refusal with the 403 the denial vocabulary
// assigns it and the typed body, not the 503 they sent before. A caller that
// retries a 5xx (capability-core's watch sweeper, Temporal activities) must be
// able to tell a refusal from an outage. 503 stays for real unavailability.
// The subject comes from the intent, not from the request context. Gated on
// TEST_DATABASE_URL via the shared d12 helpers.

var serviceResolverRoutes = []struct {
	name    string
	handler func(*Server, *gin.Context)
	body    func(spaceRef, subject string) string
}{
	{"import execution decision", (*Server).issuePersonalImportExecutionDecision, func(ref, subject string) string {
		return `{"intent":{"org_id":"` + d12Org + `","space_ref":"` + ref + `","subject_id":"` + subject + `","action_schema_hash":"sha256:ingestion-import-v1","payload_digest":"sha256:d","idempotency_key":"i","source_type":"upload"}}`
	}},
	{"schedule fire decision", (*Server).issueScheduleFireDecision, func(ref, subject string) string {
		return `{"intent":{"org_id":"` + d12Org + `","space_ref":"` + ref + `","subject_id":"` + subject + `","schedule_id":"sc","fire_key":"f","template_digest":"sha256:` + strings.Repeat("a", 64) + `","idempotency_key":"i"}}`
	}},
	{"scheduled run decision", (*Server).issueScheduledRunDecision, func(ref, subject string) string {
		return `{"intent":{"org_id":"` + d12Org + `","space_ref":"` + ref + `","subject_id":"` + subject + `","schedule_id":"sc","fire_key":"f","task_id":"t","template_digest":"sha256:` + strings.Repeat("a", 64) + `","idempotency_key":"i"}}`
	}},
	{"scheduled run execution decision", (*Server).issueScheduledRunExecutionDecision, func(ref, subject string) string {
		return `{"intent":{"org_id":"` + d12Org + `","space_ref":"` + ref + `","subject_id":"` + subject + `","schedule_id":"sc","fire_key":"f","task_id":"t","thread_id":"th","template_digest":"sha256:` + strings.Repeat("a", 64) + `","idempotency_key":"i"}}`
	}},
	{"sandbox capability decision", (*Server).issueSpaceCapabilityDecision, func(ref, subject string) string {
		return `{"intent":{"org_id":"` + d12Org + `","space_ref":"` + ref + `","subject_id":"` + subject + `","backend_id":"b","profile_digest":"sha256:` + strings.Repeat("a", 64) + `","persistence":"ephemeral","processes":"bounded_oneshot","egress":"disabled_by_default","credential_mode":"none","idempotency_key":"i"}}`
	}},
	{"watch observe decision", (*Server).issueWatchObserveDecision, func(ref, subject string) string {
		return `{"intent":{"org_id":"` + d12Org + `","space_ref":"` + ref + `","subject_id":"` + subject + `","watch_id":"w","source_kind":"k","source_ref":"r","predicate_digest":"sha256:p","idempotency_key":"i"}}`
	}},
}

func TestServiceResolverPathsAnswerTypedRefusalsWith403Live(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, false) // no effect entitlement is on
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)

	for _, route := range serviceResolverRoutes {
		t.Run(route.name, func(t *testing.T) {
			// The owner of their own personal Space, but the org's entitlement is off.
			w := driveSpaceRoute(s, route.handler, route.body(d12Personal, d12Owner), "", "")
			body := decodeDenialBody(t, w)
			if w.Code != http.StatusForbidden {
				t.Fatalf("entitlement off: status = %d, want 403 (%s)", w.Code, w.Body.String())
			}
			if body["code"] != "space_entitlement_off" || body["resolvable_by"] != "org_admin" {
				t.Fatalf("entitlement off: the typed body is missing or changed: %v", body)
			}

			// A room is the wrong kind for a personal-Space route.
			w = driveSpaceRoute(s, route.handler, route.body(d12Room, d12Owner), "", "")
			body = decodeDenialBody(t, w)
			if w.Code != http.StatusForbidden || body["code"] != "space_kind_unsupported" {
				t.Fatalf("kind unsupported: %d %v", w.Code, body)
			}

			// A subject who is not in the personal Space keeps the plain membership 403.
			w = driveSpaceRoute(s, route.handler, route.body(d12Personal, d12Peer), "", "")
			if w.Code != http.StatusForbidden {
				t.Fatalf("non-member: status = %d, want 403 (%s)", w.Code, w.Body.String())
			}
		})
	}
}

// Real unavailability is still a 503: a repository that cannot reach its
// database is not a refusal, and a caller must keep retrying it.
func TestServiceResolverPathsKeep503ForRealUnavailabilityLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, false)
	d12SigningEnv(t)
	s := &Server{}
	s.SetSpaceRepository(repo)
	pool.Close()

	for _, route := range serviceResolverRoutes {
		t.Run(route.name, func(t *testing.T) {
			w := driveSpaceRoute(s, route.handler, route.body(d12Personal, d12Owner), "", "")
			if w.Code != http.StatusServiceUnavailable {
				t.Fatalf("unavailable: status = %d, want 503 (%s)", w.Code, w.Body.String())
			}
			if body := decodeDenialBody(t, w); body["error"] == nil {
				t.Fatalf("unavailable: the body lost its error string: %v", body)
			}
		})
	}
}
