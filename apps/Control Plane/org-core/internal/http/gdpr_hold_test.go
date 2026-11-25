package http

import (
	"bytes"
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/gin-gonic/gin"
)

// failingHolds is a retention service whose hold list cannot be read.
type failingHolds struct{ retentionService }

func (failingHolds) Holds(context.Context, string) (retention.HoldList, error) {
	return retention.HoldList{}, errors.New("database unavailable")
}

func holdTestContext(method, path string) (*gin.Context, *httptest.ResponseRecorder) {
	gin.SetMode(gin.TestMode)
	recorder := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(recorder)
	c.Request = httptest.NewRequest(method, path, nil)
	return c, recorder
}

func placeHold(t *testing.T, service *retention.Service, req retention.PlaceHoldRequest) retention.Hold {
	t.Helper()
	result, err := service.PlaceHold(context.Background(), modelPolicyOrg, retention.Actor{ID: "u-comp", Role: "compliance"}, req)
	if err != nil {
		t.Fatalf("place hold: %v", err)
	}
	return result.Hold
}

func TestErasureHoldCheckSwitchIsStrict(t *testing.T) {
	store := &memoryRetentionStore{}
	service := retention.NewService(store, nil, nil)
	placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeOrg, HoldRef: "Matter"})
	server := &Server{}
	server.SetRetention(service)

	for _, value := range []string{"", "1", "TRUE", "yes"} {
		t.Setenv(envErasureHoldCheck, value)
		c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
		if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) {
			t.Fatalf("switch=%q must leave erasure unchecked: %s", value, recorder.Body.String())
		}
	}
}

func TestOrgHoldBlocksErasureUntilReleased(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	store := &memoryRetentionStore{}
	service := retention.NewService(store, nil, nil)
	server := &Server{}
	server.SetRetention(service)

	c, _ := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
	if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) {
		t.Fatal("an org without holds may be erased")
	}

	hold := placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeOrg, HoldRef: "Matter 2026-17"})
	c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
	if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) {
		t.Fatal("an org hold must block erasure")
	}
	if recorder.Code != http.StatusConflict || errorCode(t, recorder) != codeErasureBlockedLegalHold {
		t.Fatalf("held: status=%d body=%s", recorder.Code, recorder.Body.String())
	}
	details := decodeBody(t, recorder)["error"].(map[string]any)["details"].(map[string]any)
	if details["org_holds"].(float64) != 1 || details["thread_holds"].(float64) != 0 || details["retryable"] != false {
		t.Fatalf("details: %v", details)
	}
	held := map[string]bool{}
	for _, name := range details["held_content"].([]any) {
		held[name.(string)] = true
	}
	if !held["thread_goals"] || !held["artifact_revisions"] {
		t.Fatalf("the blocked scope must name goals and revisions: %v", details["held_content"])
	}

	if _, err := service.ReleaseHold(context.Background(), modelPolicyOrg, hold.ID, retention.Actor{ID: "u-owner", Role: "owner"}); err != nil {
		t.Fatalf("release: %v", err)
	}
	c, recorder = holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
	if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) {
		t.Fatalf("a released hold must not block erasure: %s", recorder.Body.String())
	}
}

func TestThreadHoldBlocksOrgErasure(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	store := &memoryRetentionStore{}
	service := retention.NewService(store, nil, nil)
	placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeThread, ThreadID: "thr-1", HoldRef: "Matter"})
	server := &Server{}
	server.SetRetention(service)

	c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/soft-delete")
	if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpSoftDelete) {
		t.Fatal("a thread hold must block the org erasure that would purge the thread")
	}
	details := decodeBody(t, recorder)["error"].(map[string]any)["details"].(map[string]any)
	if recorder.Code != http.StatusConflict || details["thread_holds"].(float64) != 1 {
		t.Fatalf("thread hold: status=%d body=%s", recorder.Code, recorder.Body.String())
	}
}

func TestUnreadableHoldsRefuseErasureAsRetryable(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	for name, server := range map[string]*Server{
		"no service":    {},
		"store failure": {retention: failingHolds{}},
	} {
		c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
		if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) {
			t.Fatalf("%s: unreadable holds must refuse erasure", name)
		}
		if recorder.Code != http.StatusServiceUnavailable || errorCode(t, recorder) != codeErasureHoldCheckUnavailable {
			t.Fatalf("%s: status=%d body=%s", name, recorder.Code, recorder.Body.String())
		}
		details := decodeBody(t, recorder)["error"].(map[string]any)["details"].(map[string]any)
		if details["retryable"] != true {
			t.Fatalf("%s: the outage refusal is retryable: %v", name, details)
		}
	}
}

// The real route refuses a held org before the irreversible procedure runs: the
// org service is nil here, so reaching HardDelete would panic.
func TestHardDeleteRouteRefusesAHeldOrgBeforeErasing(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	store := &memoryRetentionStore{}
	service := retention.NewService(store, nil, nil)
	placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeOrg, HoldRef: "Matter"})
	server := &Server{}
	server.SetRetention(service)

	gin.SetMode(gin.TestMode)
	router := gin.New()
	router.DELETE("/orgs/:id/gdpr/erase", server.hardDeleteOrganization)
	router.DELETE("/orgs/:id/gdpr/soft-delete", server.softDeleteOrganization)
	for path, body := range map[string]string{
		"/orgs/" + modelPolicyOrg + "/gdpr/erase":       `{"confirm":true}`,
		"/orgs/" + modelPolicyOrg + "/gdpr/soft-delete": `{"confirm":true,"org_name":"Acme"}`,
	} {
		request := httptest.NewRequest(http.MethodDelete, path, bytes.NewReader([]byte(body)))
		request.Header.Set("Content-Type", "application/json")
		request.Header.Set("x-user-id", "u-platform")
		request.Header.Set("X-User-Role", "admin")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusConflict || errorCode(t, response) != codeErasureBlockedLegalHold {
			t.Fatalf("%s: status=%d body=%s", path, response.Code, response.Body.String())
		}
	}
}

// auditingRetentionStore is the in-memory store with the refusal audit the real
// repository has (F-f10-06).
type auditingRetentionStore struct {
	*memoryRetentionStore
	refusals []map[string]any
	fail     error
}

func (s *auditingRetentionStore) EnqueueErasureRefusal(_ context.Context, orgID string, actor retention.Actor, details map[string]any) error {
	if s.fail != nil {
		return s.fail
	}
	s.refusals = append(s.refusals, map[string]any{"org_id": orgID, "actor": actor, "details": details})
	return nil
}

// failingHoldsAuditor cannot read holds but counts refusal audits.
type failingHoldsAuditor struct {
	failingHolds
	audits *int
}

func (a failingHoldsAuditor) AuditRefusedErasure(context.Context, string, retention.Actor, string, int, int) error {
	*a.audits++
	return nil
}

// F-f10-06 (decision D-C1): a held refusal leaves exactly one audit row naming the
// actor, the operation and the counts, never the hold reference.
func TestHeldErasureRefusalIsAudited(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	store := &auditingRetentionStore{memoryRetentionStore: &memoryRetentionStore{}}
	service := retention.NewService(store, nil, nil)
	placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeThread, ThreadID: "thr-1", HoldRef: "Matter 2026-17"})
	server := &Server{}
	server.SetRetention(service)

	c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/soft-delete")
	if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpSoftDelete) {
		t.Fatal("a held org must be refused")
	}
	if recorder.Code != http.StatusConflict || len(store.refusals) != 1 {
		t.Fatalf("status=%d refusals=%d; want 409 and exactly one audit row", recorder.Code, len(store.refusals))
	}
	row := store.refusals[0]
	details := row["details"].(map[string]any)
	if row["org_id"] != modelPolicyOrg || row["actor"] != (retention.Actor{ID: "u-owner", Role: "owner"}) ||
		details["kind"] != "erasure_refused" || details["op"] != retention.ErasureOpSoftDelete ||
		details["org_holds"] != 0 || details["thread_holds"] != 1 {
		t.Fatalf("refusal row = %v", row)
	}
	if _, ok := details["hold_ref"]; ok {
		t.Fatalf("the refusal row must not carry the hold reference: %v", details)
	}

	// No hold, no refusal, no row.
	other := &auditingRetentionStore{memoryRetentionStore: &memoryRetentionStore{}}
	server.SetRetention(retention.NewService(other, nil, nil))
	c, _ = holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
	if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) || len(other.refusals) != 0 {
		t.Fatalf("an unheld org is not refused and leaves no row: %d", len(other.refusals))
	}
}

// An audit that cannot be written never changes the refusal.
func TestHeldErasureRefusalStandsWhenTheAuditFails(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	for name, store := range map[string]retention.Store{
		"audit write fails":   &auditingRetentionStore{memoryRetentionStore: &memoryRetentionStore{}, fail: errors.New("outbox down")},
		"store cannot record": &memoryRetentionStore{},
	} {
		service := retention.NewService(store, nil, nil)
		placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeOrg, HoldRef: "Matter"})
		server := &Server{}
		server.SetRetention(service)
		c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
		if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) ||
			recorder.Code != http.StatusConflict || errorCode(t, recorder) != codeErasureBlockedLegalHold {
			t.Fatalf("%s: status=%d body=%s", name, recorder.Code, recorder.Body.String())
		}
	}
}

// The unavailable refusal (503) records nothing: there is no hold count to record.
func TestUnavailableErasureRefusalWritesNoAudit(t *testing.T) {
	t.Setenv(envErasureHoldCheck, "true")
	audits := 0
	server := &Server{retention: failingHoldsAuditor{audits: &audits}}
	c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
	if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "owner", retention.ErasureOpHardDelete) ||
		recorder.Code != http.StatusServiceUnavailable {
		t.Fatalf("status=%d body=%s", recorder.Code, recorder.Body.String())
	}
	if audits != 0 {
		t.Fatalf("audits = %d, want 0 for the unavailable refusal", audits)
	}
}
