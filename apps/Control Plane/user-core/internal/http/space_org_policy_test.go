package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS03-S3: the org-admin Space policy routes. A fake store and a fake role
// resolver stand in for Postgres and auth-core; the real routes (registered
// through the P-UC1 seam) and the real guard run.

type fakeOrgPolicyStore struct {
	view      spaces.OrgPolicyView
	viewErr   error
	applyRes  spaces.ApplyOrgEntitlementsResult
	applyErr  error
	events    []spaces.OrgPolicyEvent
	next      int64
	eventsErr error
	denyErr   error

	calls       int
	applied     []spaces.ApplyOrgEntitlementsRequest
	denied      []string
	deniedActor []spaces.EffectPolicyActor
	listed      []struct {
		limit  int
		cursor int64
	}
}

func (f *fakeOrgPolicyStore) GetOrgPolicy(_ context.Context, orgID string) (spaces.OrgPolicyView, error) {
	f.calls++
	view := f.view
	view.OrgID = orgID
	return view, f.viewErr
}

func (f *fakeOrgPolicyStore) ApplyOrgEntitlements(_ context.Context, req spaces.ApplyOrgEntitlementsRequest) (spaces.ApplyOrgEntitlementsResult, error) {
	f.calls++
	f.applied = append(f.applied, req)
	return f.applyRes, f.applyErr
}

func (f *fakeOrgPolicyStore) ListOrgPolicyEvents(_ context.Context, _ string, limit int, cursor int64) ([]spaces.OrgPolicyEvent, int64, error) {
	f.calls++
	f.listed = append(f.listed, struct {
		limit  int
		cursor int64
	}{limit, cursor})
	return f.events, f.next, f.eventsErr
}

func (f *fakeOrgPolicyStore) RecordOrgPolicyWriteDenied(_ context.Context, _ string, actor spaces.EffectPolicyActor, reason string) error {
	f.denied = append(f.denied, reason)
	f.deniedActor = append(f.deniedActor, actor)
	return f.denyErr
}

type orgPolicyCaller struct {
	authMethod  string
	serviceID   string
	delegated   bool
	scopes      []string
	userID      string
	orgID       string
	omitContext bool
}

func gatewayAdmin(scopes ...string) orgPolicyCaller {
	return orgPolicyCaller{
		authMethod: "service_principal", serviceID: verevonGatewayPrincipal, delegated: true,
		scopes: scopes, userID: "user-1", orgID: "org-1",
	}
}

type orgPolicyRig struct {
	server    *Server
	store     *fakeOrgPolicyStore
	role      string
	roleErr   error
	roleCalls int
}

func newOrgPolicyRig(writes bool) *orgPolicyRig {
	rig := &orgPolicyRig{store: &fakeOrgPolicyStore{}, role: "admin"}
	rig.server = &Server{}
	rig.server.spaceOrgPolicy = spaceOrgPolicyConfig{
		store: rig.store, writesEnabled: writes,
		roleResolver: func(context.Context, string, string) (string, error) {
			rig.roleCalls++
			return rig.role, rig.roleErr
		},
	}
	return rig
}

func (r *orgPolicyRig) do(caller orgPolicyCaller, method, target, body string) *httptest.ResponseRecorder {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	engine.Use(func(c *gin.Context) {
		if caller.omitContext {
			return
		}
		c.Set("auth_method", caller.authMethod)
		c.Set("service_id", caller.serviceID)
		c.Set("delegation_verified", caller.delegated)
		c.Set("service_scopes", caller.scopes)
		c.Set("user_id", caller.userID)
		c.Set("org_id", caller.orgID)
	})
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(r.server, group)
	}
	request := httptest.NewRequest(method, target, bytes.NewBufferString(body))
	request.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	engine.ServeHTTP(w, request)
	return w
}

func orgPolicyError(t *testing.T, w *httptest.ResponseRecorder) (code, message string, details map[string]any) {
	t.Helper()
	var body struct {
		Error struct {
			Code    string         `json:"code"`
			Message string         `json:"message"`
			Details map[string]any `json:"details"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("not a typed error envelope: %v (%s)", err, w.Body.String())
	}
	if body.Error.Details == nil {
		t.Fatalf("details must be an object, never null: %s", w.Body.String())
	}
	return body.Error.Code, body.Error.Message, body.Error.Details
}

const (
	orgPolicyGet    = "/api/v1/internal/spaces/org-policy"
	orgPolicyEvents = "/api/v1/internal/spaces/org-policy/events"
	orgPolicyPut    = "/api/v1/internal/spaces/org-policy/entitlements"
	validPutBody    = `{"idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation"}`
)

func TestOrgPolicyRoutesRegisterThroughTheSeam(t *testing.T) {
	gin.SetMode(gin.TestMode)
	engine := gin.New()
	group := engine.Group("/api/v1/internal/spaces")
	for _, register := range spaceFeatureRouteRegistrars {
		register(&Server{}, group)
	}
	counts := routeCounts(engine.Routes())
	for _, route := range []string{"GET " + orgPolicyGet, "GET " + orgPolicyEvents, "PUT " + orgPolicyPut} {
		if counts[route] != 1 {
			t.Errorf("route %q registered %d times", route, counts[route])
		}
	}
}

func TestOrgPolicyMemberAndViewerAreRefusedAndTheStoreIsNeverCalled(t *testing.T) {
	for _, role := range []string{"member", "viewer", "", "guest", "Admin", "owner "} {
		rig := newOrgPolicyRig(true)
		rig.role = role
		for _, call := range []struct{ method, target, body string }{
			{http.MethodGet, orgPolicyGet, ""},
			{http.MethodGet, orgPolicyEvents, ""},
			{http.MethodPut, orgPolicyPut, validPutBody},
		} {
			w := rig.do(gatewayAdmin("spaces:policy:admin:read", "spaces:policy:admin:write"), call.method, call.target, call.body)
			if w.Code != http.StatusForbidden {
				t.Fatalf("role %q %s %s: want 403, got %d (%s)", role, call.method, call.target, w.Code, w.Body.String())
			}
			if code, _, _ := orgPolicyError(t, w); code != "space_org_admin_required" {
				t.Fatalf("role %q: code %q", role, code)
			}
		}
		if rig.store.calls != 0 || len(rig.store.denied) != 0 {
			t.Fatalf("role %q reached the store (%d calls, %d denied audits)", role, rig.store.calls, len(rig.store.denied))
		}
	}
	// Owner and admin pass.
	for _, role := range []string{"owner", "admin"} {
		rig := newOrgPolicyRig(true)
		rig.role = role
		if w := rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyGet, ""); w.Code != http.StatusOK {
			t.Fatalf("role %q: want 200, got %d (%s)", role, w.Code, w.Body.String())
		}
	}
}

func TestOrgPolicyRequiresGatewayDelegationScopeAndIdentity(t *testing.T) {
	read := gatewayAdmin("spaces:policy:admin:read")
	write := gatewayAdmin("spaces:policy:admin:write")
	cases := []struct {
		name   string
		caller orgPolicyCaller
		method string
		target string
		body   string
	}{
		{"no delegation", func() orgPolicyCaller { c := read; c.delegated = false; return c }(), http.MethodGet, orgPolicyGet, ""},
		{"another service principal", func() orgPolicyCaller { c := read; c.serviceID = "integration-core"; return c }(), http.MethodGet, orgPolicyGet, ""},
		{"not a service principal", func() orgPolicyCaller { c := read; c.authMethod = "user"; return c }(), http.MethodGet, orgPolicyGet, ""},
		{"missing read scope", gatewayAdmin("spaces:policy:write"), http.MethodGet, orgPolicyGet, ""},
		{"write scope does not open the read route", write, http.MethodGet, orgPolicyEvents, ""},
		{"read scope does not open the write route", read, http.MethodPut, orgPolicyPut, validPutBody},
		{"no user", func() orgPolicyCaller { c := read; c.userID = ""; return c }(), http.MethodGet, orgPolicyGet, ""},
		{"no org", func() orgPolicyCaller { c := read; c.orgID = " "; return c }(), http.MethodGet, orgPolicyGet, ""},
		{"no auth context at all", orgPolicyCaller{omitContext: true}, http.MethodGet, orgPolicyGet, ""},
	}
	for _, tc := range cases {
		rig := newOrgPolicyRig(true)
		w := rig.do(tc.caller, tc.method, tc.target, tc.body)
		if w.Code != http.StatusForbidden {
			t.Errorf("%s: want 403, got %d (%s)", tc.name, w.Code, w.Body.String())
			continue
		}
		if code, _, _ := orgPolicyError(t, w); code != "space_org_admin_required" {
			t.Errorf("%s: code %q", tc.name, code)
		}
		if rig.store.calls != 0 || rig.roleCalls != 0 {
			t.Errorf("%s: a refused caller reached the role resolver (%d) or store (%d)", tc.name, rig.roleCalls, rig.store.calls)
		}
	}
}

func TestOrgPolicyMembershipAuthorityFailuresFailClosed(t *testing.T) {
	// auth-core down: 503, no fallback to a local projection, store never called.
	rig := newOrgPolicyRig(true)
	rig.roleErr = errors.New("auth-core unreachable")
	w := rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyGet, "")
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("auth-core down: want 503, got %d (%s)", w.Code, w.Body.String())
	}
	if code, _, _ := orgPolicyError(t, w); code != "space_policy_membership_authority_unavailable" {
		t.Fatalf("code = %q", code)
	}
	if rig.store.calls != 0 {
		t.Fatal("the store was reached while the membership authority was down")
	}

	// Not a member: 403 space_org_membership_required.
	rig = newOrgPolicyRig(true)
	rig.roleErr = ErrMembershipNotFound
	w = rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyGet, "")
	if w.Code != http.StatusForbidden {
		t.Fatalf("not a member: want 403, got %d", w.Code)
	}
	if code, _, _ := orgPolicyError(t, w); code != "space_org_membership_required" {
		t.Fatalf("code = %q", code)
	}

	// No store wired: 503 space_policy_unavailable.
	rig = newOrgPolicyRig(true)
	rig.server.spaceOrgPolicy.store = nil
	w = rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyGet, "")
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("no store: want 503, got %d", w.Code)
	}
	if code, _, _ := orgPolicyError(t, w); code != "space_policy_unavailable" {
		t.Fatalf("code = %q", code)
	}
}

func TestOrgPolicyWriteSwitchOffIsForbiddenAndAuditedButReadsStayAvailable(t *testing.T) {
	rig := newOrgPolicyRig(false)
	rig.role = "owner"
	w := rig.do(gatewayAdmin("spaces:policy:admin:write"), http.MethodPut, orgPolicyPut, validPutBody)
	if w.Code != http.StatusForbidden {
		t.Fatalf("switch off: want 403, got %d (%s)", w.Code, w.Body.String())
	}
	if code, _, _ := orgPolicyError(t, w); code != "space_policy_admin_writes_disabled" {
		t.Fatalf("code = %q", code)
	}
	if len(rig.store.applied) != 0 {
		t.Fatal("a write reached the store with the switch off")
	}
	if len(rig.store.denied) != 1 || rig.store.denied[0] != "space_policy_admin_writes_disabled" {
		t.Fatalf("denied audit calls = %v, want exactly one", rig.store.denied)
	}
	if actor := rig.store.deniedActor[0]; actor.Type != "user" || actor.ID != "user-1" || actor.OrgRole != "owner" {
		t.Fatalf("denied audit actor = %+v", actor)
	}

	// A failing denial audit must not turn the refusal into something else.
	rig.store.denyErr = errors.New("outbox down")
	if w := rig.do(gatewayAdmin("spaces:policy:admin:write"), http.MethodPut, orgPolicyPut, validPutBody); w.Code != http.StatusForbidden {
		t.Fatalf("with a failing audit: want 403, got %d", w.Code)
	}

	// Reads and history work with the switch off, and say why writing is not possible.
	w = rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyGet, "")
	if w.Code != http.StatusOK {
		t.Fatalf("read with the switch off: %d (%s)", w.Code, w.Body.String())
	}
	var body struct {
		Data spaces.OrgPolicyView `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if body.Data.Capabilities.Write || body.Data.Capabilities.WriteDeniedReason == nil || *body.Data.Capabilities.WriteDeniedReason != "writes_disabled" {
		t.Fatalf("capabilities = %+v", body.Data.Capabilities)
	}
	if w := rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyEvents, ""); w.Code != http.StatusOK {
		t.Fatalf("history with the switch off: %d", w.Code)
	}
}

func TestOrgPolicyCapabilitiesReflectProvisioningAndTheSwitch(t *testing.T) {
	cases := []struct {
		name       string
		writes     bool
		state      string
		wantWrite  bool
		wantReason string
	}{
		{"switch off", false, spaces.OrgPolicyStateProvisioned, false, "writes_disabled"},
		{"not provisioned", true, spaces.OrgPolicyStateNotProvisioned, false, "not_provisioned"},
		{"writable", true, spaces.OrgPolicyStateProvisioned, true, ""},
	}
	for _, tc := range cases {
		rig := newOrgPolicyRig(tc.writes)
		rig.store.view = spaces.OrgPolicyView{State: tc.state}
		w := rig.do(gatewayAdmin("spaces:policy:admin:read"), http.MethodGet, orgPolicyGet, "")
		var body struct {
			Data struct {
				OrgID        string                       `json:"org_id"`
				Capabilities spaces.OrgPolicyCapabilities `json:"capabilities"`
			} `json:"data"`
		}
		if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
			t.Fatalf("%s: %v (%s)", tc.name, err, w.Body.String())
		}
		if body.Data.OrgID != "org-1" {
			t.Errorf("%s: the org must come from the delegation, got %q", tc.name, body.Data.OrgID)
		}
		got := body.Data.Capabilities
		if got.Write != tc.wantWrite {
			t.Errorf("%s: write = %v", tc.name, got.Write)
		}
		switch {
		case tc.wantReason == "" && got.WriteDeniedReason != nil:
			t.Errorf("%s: unexpected reason %q", tc.name, *got.WriteDeniedReason)
		case tc.wantReason != "" && (got.WriteDeniedReason == nil || *got.WriteDeniedReason != tc.wantReason):
			t.Errorf("%s: reason = %v, want %q", tc.name, got.WriteDeniedReason, tc.wantReason)
		}
	}
}

func TestOrgPolicyPutRejectsOrgActorAndRoleFieldsAndBadShapes(t *testing.T) {
	caller := gatewayAdmin("spaces:policy:admin:write")
	reject := map[string]string{
		"org_id in the body":               `{"org_id":"org-9","idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation"}`,
		"actor in the body":                `{"actor":"user-9","idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation"}`,
		"role in the body":                 `{"role":"owner","idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation"}`,
		"unknown field":                    `{"idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation","extra":1}`,
		"not json":                         `nope`,
		"trailing data":                    `{"idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation"} {}`,
		"missing revision":                 `{"idempotency_key":"idem-0001","preset_id":"conversation"}`,
		"neither preset nor entitlements":  `{"idempotency_key":"idem-0001","expected_entitlement_revision":3}`,
		"both preset and entitlements":     `{"idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"conversation","entitlements":{"thread_create":true}}`,
		"blank preset and no entitlements": `{"idempotency_key":"idem-0001","expected_entitlement_revision":3,"preset_id":"  "}`,
		"oversized body":                   `{"idempotency_key":"` + strings.Repeat("k", 17<<10) + `"}`,
	}
	for name, body := range reject {
		rig := newOrgPolicyRig(true)
		w := rig.do(caller, http.MethodPut, orgPolicyPut, body)
		if w.Code != http.StatusBadRequest {
			t.Errorf("%s: want 400, got %d (%s)", name, w.Code, w.Body.String())
			continue
		}
		if code, _, _ := orgPolicyError(t, w); code != spaces.OrgPolicyCodeInvalidRequest {
			t.Errorf("%s: code %q", name, code)
		}
		if len(rig.store.applied) != 0 {
			t.Errorf("%s: reached the store", name)
		}
	}
}

func TestOrgPolicyPutTakesOrgAndActorFromTheDelegationOnly(t *testing.T) {
	rig := newOrgPolicyRig(true)
	rig.role = "admin"
	rig.store.applyRes = spaces.ApplyOrgEntitlementsResult{Changed: true, SpacesFenced: 2, Policy: spaces.OrgPolicyView{State: spaces.OrgPolicyStateProvisioned}}
	w := rig.do(gatewayAdmin("spaces:policy:admin:write"), http.MethodPut, orgPolicyPut, validPutBody)
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d (%s)", w.Code, w.Body.String())
	}
	if len(rig.store.applied) != 1 {
		t.Fatalf("store calls = %d", len(rig.store.applied))
	}
	req := rig.store.applied[0]
	if req.OrgID != "org-1" || req.Actor.Type != "user" || req.Actor.ID != "user-1" || req.Actor.OrgRole != "admin" {
		t.Fatalf("org/actor not taken from the delegation: %+v", req)
	}
	if req.IdempotencyKey != "idem-0001" || req.ExpectedEntitlementRevision != 3 || req.PresetID != "conversation" || req.Entitlements != nil {
		t.Fatalf("request drifted: %+v", req)
	}
	var body struct {
		Data struct {
			Changed      bool `json:"changed"`
			SpacesFenced int  `json:"spaces_fenced"`
			Policy       struct {
				Capabilities spaces.OrgPolicyCapabilities `json:"capabilities"`
			} `json:"policy"`
		} `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if !body.Data.Changed || body.Data.SpacesFenced != 2 || !body.Data.Policy.Capabilities.Write {
		t.Fatalf("envelope = %+v", body.Data)
	}

	// The explicit-map form passes the map through and no preset.
	rig.store.applied = nil
	explicit := `{"idempotency_key":"idem-0002","expected_entitlement_revision":4,"entitlements":{"thread_create":true,"thread_read":false}}`
	if w := rig.do(gatewayAdmin("spaces:policy:admin:write"), http.MethodPut, orgPolicyPut, explicit); w.Code != http.StatusOK {
		t.Fatalf("explicit: %d %s", w.Code, w.Body.String())
	}
	if got := rig.store.applied[0]; got.PresetID != "" || len(got.Entitlements) != 2 || !got.Entitlements["thread_create"] || got.Entitlements["thread_read"] {
		t.Fatalf("explicit request drifted: %+v", got)
	}
}

func TestOrgPolicyErrorCodesMapToStatuses(t *testing.T) {
	cases := []struct {
		name       string
		err        error
		wantStatus int
		wantCode   string
		wantDetail string
	}{
		{"invalid request", &spaces.OrgPolicyError{Code: spaces.OrgPolicyCodeInvalidRequest, Message: "bad", Details: map[string]any{"field": "x"}}, 400, "space_policy_invalid_request", "field"},
		{"revision conflict", &spaces.OrgPolicyError{Code: spaces.OrgPolicyCodeRevisionConflict, Message: "stale", Details: map[string]any{"current_entitlement_revision": 9}}, 409, "space_policy_revision_conflict", "current_entitlement_revision"},
		{"idempotency conflict", &spaces.OrgPolicyError{Code: spaces.OrgPolicyCodeIdempotencyConflict, Message: "reused"}, 409, "space_policy_idempotency_conflict", ""},
		{"not provisioned error", &spaces.OrgPolicyError{Code: spaces.OrgPolicyCodeNotProvisioned, Message: "none"}, 409, "space_policy_not_provisioned", ""},
		{"missing row", spaces.ErrNoOrgEffectPolicy, 409, "space_policy_not_provisioned", ""},
		{"wrapped missing row", errors.Join(errors.New("ctx"), spaces.ErrNoOrgEffectPolicy), 409, "space_policy_not_provisioned", ""},
		{"zdr conflict", &spaces.OrgPolicyError{Code: spaces.OrgPolicyCodeZDRConflict, Message: "zdr", Details: map[string]any{"keys": []string{"import_write"}}}, 422, "space_policy_zdr_conflict", "keys"},
		{"unknown preset", &spaces.OrgPolicyError{Code: spaces.OrgPolicyCodeUnknownPreset, Message: "unknown", Details: map[string]any{"preset_id": "x"}}, 422, "space_policy_unknown_preset", "preset_id"},
		{"store failure", errors.New("db exploded"), 503, "space_policy_unavailable", ""},
	}
	for _, tc := range cases {
		rig := newOrgPolicyRig(true)
		rig.store.applyErr = tc.err
		w := rig.do(gatewayAdmin("spaces:policy:admin:write"), http.MethodPut, orgPolicyPut, validPutBody)
		if w.Code != tc.wantStatus {
			t.Errorf("%s: status %d, want %d (%s)", tc.name, w.Code, tc.wantStatus, w.Body.String())
			continue
		}
		code, message, details := orgPolicyError(t, w)
		if code != tc.wantCode {
			t.Errorf("%s: code %q, want %q", tc.name, code, tc.wantCode)
		}
		if tc.wantDetail != "" {
			if _, ok := details[tc.wantDetail]; !ok {
				t.Errorf("%s: details %v lack %q", tc.name, details, tc.wantDetail)
			}
		}
		if tc.name == "store failure" && strings.Contains(message, "exploded") {
			t.Errorf("a raw store error leaked into the response: %q", message)
		}
	}
}

func TestOrgPolicyEventsRouteValidatesPagingAndShapesTheEnvelope(t *testing.T) {
	caller := gatewayAdmin("spaces:policy:admin:read")
	for _, target := range []string{
		orgPolicyEvents + "?limit=0", orgPolicyEvents + "?limit=101", orgPolicyEvents + "?limit=abc",
		orgPolicyEvents + "?cursor=0", orgPolicyEvents + "?cursor=-3", orgPolicyEvents + "?cursor=x",
	} {
		rig := newOrgPolicyRig(true)
		w := rig.do(caller, http.MethodGet, target, "")
		if w.Code != http.StatusBadRequest {
			t.Errorf("%s: want 400, got %d", target, w.Code)
		}
		if len(rig.store.listed) != 0 {
			t.Errorf("%s: reached the store", target)
		}
	}

	rig := newOrgPolicyRig(true)
	rig.store.next = 41
	rig.store.events = []spaces.OrgPolicyEvent{{EventID: 42, ChangeKind: spaces.PolicyChangePresetApplied}}
	w := rig.do(caller, http.MethodGet, orgPolicyEvents+"?limit=5&cursor=50", "")
	if w.Code != http.StatusOK {
		t.Fatalf("%d %s", w.Code, w.Body.String())
	}
	if got := rig.store.listed[0]; got.limit != 5 || got.cursor != 50 {
		t.Fatalf("paging passed to the store = %+v", got)
	}
	var body struct {
		Data []spaces.OrgPolicyEvent `json:"data"`
		Meta struct {
			Limit      int     `json:"limit"`
			NextCursor *string `json:"next_cursor"`
		} `json:"meta"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatal(err)
	}
	if len(body.Data) != 1 || body.Meta.Limit != 5 || body.Meta.NextCursor == nil || *body.Meta.NextCursor != "41" {
		t.Fatalf("envelope = %+v", body)
	}

	// Defaults, an empty page is [] and the last page has a null cursor.
	rig = newOrgPolicyRig(true)
	w = rig.do(caller, http.MethodGet, orgPolicyEvents, "")
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), `"data":[]`) || !strings.Contains(w.Body.String(), `"next_cursor":null`) {
		t.Fatalf("empty page: %d %s", w.Code, w.Body.String())
	}
	if got := rig.store.listed[0]; got.limit != spaces.OrgPolicyEventsDefaultLimit || got.cursor != 0 {
		t.Fatalf("defaults = %+v", got)
	}
}
