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
		if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "hard delete") {
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
	if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "hard delete") {
		t.Fatal("an org without holds may be erased")
	}

	hold := placeHold(t, service, retention.PlaceHoldRequest{Scope: retention.ScopeOrg, HoldRef: "Matter 2026-17"})
	c, recorder := holdTestContext(http.MethodDelete, "/orgs/"+modelPolicyOrg+"/gdpr/erase")
	if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "hard delete") {
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
	if server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "hard delete") {
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
	if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "soft delete") {
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
		if !server.refuseHeldErasure(c, modelPolicyOrg, "u-owner", "hard delete") {
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
