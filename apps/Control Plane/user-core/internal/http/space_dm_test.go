package http

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS12-S1: the direct-message eligibility route (POST .../dm-eligibility) and
// the registration refusal codes. Pure tier; the database legs are at the end
// of the file, gated on TEST_DATABASE_URL.

const dmEligibilityPath = "/api/v1/internal/spaces/dm-eligibility"

func TestDMEligibilityRouteScopeAndRegistration(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+dmEligibilityPath] != 1 {
		t.Fatalf("dm-eligibility registered %d times, want 1", counts["POST "+dmEligibilityPath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, dmEligibilityPath, nil))
	if scope != "spaces:resolve" {
		t.Fatalf("scope = %q, want spaces:resolve", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:resolve must require a verified gateway delegation: the acting user is never a body field")
	}
	// The rule is exact: another method or path is not claimed by it.
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, dmEligibilityPath}, {http.MethodPost, dmEligibilityPath + "/x"}, {http.MethodPut, dmEligibilityPath},
	} {
		for _, rule := range spaceFeatureScopeRules {
			if scope, ok := rule(tc.method, tc.path); ok && scope == "spaces:resolve" {
				t.Errorf("%s %s unexpectedly claimed by a feature rule", tc.method, tc.path)
			}
		}
	}
}

func TestDMEligibilityRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"execution core":      {"auth_method": "service_principal", "service_id": executionCorePrincipal, "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			rec := postJSON(engine, dmEligibilityPath, map[string]any{"participant_ids": []string{"u2"}})
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestDMEligibilityFailsClosedAndIsStrict(t *testing.T) {
	// No repository.
	rec := postJSON(spaceIssuerHarness(t, nil, gatewayContext()), dmEligibilityPath, map[string]any{"participant_ids": []string{"u2"}})
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("no repository: status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for name, body := range map[string]any{
		"unknown field": map[string]any{"participant_ids": []string{"u2"}, "caller_id": "someone-else"},
		"not json":      "{nope",
		"empty body":    "",
	} {
		t.Run(name, func(t *testing.T) {
			wantApprovalError(t, postJSON(engine, dmEligibilityPath, body), http.StatusBadRequest, spaces.CodeInvalidRequest)
		})
	}
}

func TestDMEligibilityDenials(t *testing.T) {
	gin.SetMode(gin.TestMode)
	write := func(err error) *httptest.ResponseRecorder {
		rec := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(rec)
		c.Request = httptest.NewRequest(http.MethodPost, dmEligibilityPath, nil)
		writeDMRefusal(c, err)
		return rec
	}

	wantApprovalError(t, write(spaces.ErrDMDisabled), http.StatusNotFound, "dm_disabled")

	body := wantApprovalError(t, write(&spaces.DMNotEntitledError{Missing: []string{"dm_create_entitled", "thread_read_entitled"}}),
		http.StatusForbidden, "dm_not_entitled")
	missing, _ := body.Error.Details["missing"].([]any)
	if len(missing) != 2 || missing[0] != "dm_create_entitled" || missing[1] != "thread_read_entitled" {
		t.Fatalf("details.missing = %v", body.Error.Details["missing"])
	}

	for _, reason := range []string{
		spaces.DMReasonCount, spaces.DMReasonDuplicate, spaces.DMReasonSelf, spaces.DMReasonNotInOrg, spaces.DMReasonInactive,
	} {
		body := wantApprovalError(t, write(&spaces.DMParticipantInvalidError{Reason: reason}),
			http.StatusUnprocessableEntity, "dm_participants_invalid")
		if body.Error.Details["reason"] != reason {
			t.Errorf("details.reason = %v, want %s", body.Error.Details["reason"], reason)
		}
	}
	// A wrapped sentinel still maps, and an unknown failure is a 503 that
	// leaks nothing.
	wantApprovalError(t, write(fmt.Errorf("register: %w", spaces.ErrDMDisabled)), http.StatusNotFound, "dm_disabled")
	rec := write(errors.New("pq: connection refused at 10.0.0.5"))
	wantApprovalError(t, rec, http.StatusServiceUnavailable, spaces.CodeSpaceAuthorityUnavailable)
	if body := rec.Body.String(); len(body) == 0 || strings.Contains(body, "10.0.0.5") {
		t.Fatalf("the database error leaked: %s", body)
	}
}

func TestRegisterSpaceMapsDMRefusalsToTheirStableCode(t *testing.T) {
	for _, tc := range []struct {
		err  error
		code string
	}{
		{spaces.ErrDMDisabled, "dm_disabled"},
		{&spaces.DMNotEntitledError{Missing: []string{"dm_create_entitled"}}, "dm_not_entitled"},
		{&spaces.DMParticipantInvalidError{Reason: spaces.DMReasonNotInOrg}, "dm_participants_invalid"},
		{spaces.ErrDMExists, "dm_exists"},
		{fmt.Errorf("register: %w", spaces.ErrDMExists), "dm_exists"},
	} {
		got, ok := dmRegistrationRefusalCode(tc.err)
		if !ok || got != tc.code {
			t.Errorf("%v -> %q ok=%v, want %q", tc.err, got, ok, tc.code)
		}
	}
	for _, other := range []error{errors.New("boom"), spaces.ErrFixedRoster, spaces.ErrInactiveOwnerMembership} {
		if code, ok := dmRegistrationRefusalCode(other); ok {
			t.Errorf("%v must keep its existing mapping, got code %q", other, code)
		}
	}
}

// Live: the real route against a real repository.
func TestDMEligibilityRouteLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	room := newW2Room("ws12s1")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) {
		p.DMCreateEntitled, p.ThreadCreateEntitled, p.ThreadReadEntitled = true, true, true
	})
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = room.Org, room.Owner
	engine := spaceIssuerHarness(t, repo, ctx)
	body := func(ids ...string) map[string]any { return map[string]any{"participant_ids": ids} }

	// Switch off (default): 404 dm_disabled.
	wantApprovalError(t, postJSON(engine, dmEligibilityPath, body(room.Manager)), http.StatusNotFound, "dm_disabled")
	repo.SetDMRegistrationEnabled(true)
	t.Cleanup(func() { repo.SetDMRegistrationEnabled(false) })

	rec := postJSON(engine, dmEligibilityPath, body(room.Manager))
	if rec.Code != http.StatusOK {
		t.Fatalf("eligible pair: %d %s", rec.Code, rec.Body.String())
	}
	wantReason := func(rec *httptest.ResponseRecorder, reason string) {
		t.Helper()
		got := wantApprovalError(t, rec, http.StatusUnprocessableEntity, "dm_participants_invalid")
		if got.Error.Details["reason"] != reason {
			t.Fatalf("reason = %v, want %s", got.Error.Details["reason"], reason)
		}
	}
	wantReason(postJSON(engine, dmEligibilityPath, body()), spaces.DMReasonCount)
	wantReason(postJSON(engine, dmEligibilityPath, body(room.Manager, room.Manager)), spaces.DMReasonDuplicate)
	wantReason(postJSON(engine, dmEligibilityPath, body(room.Owner)), spaces.DMReasonSelf)
	wantReason(postJSON(engine, dmEligibilityPath, body("ws12s1-nobody")), spaces.DMReasonNotInOrg)

	// Entitlement off: 403 with details.missing.
	if _, err := pool.Exec(context.Background(), `UPDATE space_effect_policies SET dm_create_entitled=FALSE WHERE org_id=$1`, room.Org); err != nil {
		t.Fatal(err)
	}
	denied := wantApprovalError(t, postJSON(engine, dmEligibilityPath, body(room.Manager)), http.StatusForbidden, "dm_not_entitled")
	if missing, _ := denied.Error.Details["missing"].([]any); len(missing) != 1 || missing[0] != "dm_create_entitled" {
		t.Fatalf("details.missing = %v", denied.Error.Details["missing"])
	}
}
