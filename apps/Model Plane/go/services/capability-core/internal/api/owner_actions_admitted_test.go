package api

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"reflect"
	"sort"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// W3-EP-6-S11-route: GET /api/v1/capabilities/owner-actions/admitted answers the ids of the owner
// actions the org's run view can offer, in both admission modes, and nothing else.

// listingAdmissions answers both ListEnabled (the run view) and ListAllEnabled (the listing). It
// deliberately returns every row whatever its status, so the handler's own enabled check is what
// these tests pin.
type listingAdmissions struct {
	rows []*registry.AgentActionAdmission
	err  error
}

func (l *listingAdmissions) ListEnabled(context.Context, []string) ([]*registry.AgentActionAdmission, error) {
	return l.rows, l.err
}

func (l *listingAdmissions) ListAllEnabled(context.Context) ([]*registry.AgentActionAdmission, error) {
	return l.rows, l.err
}

// runViewOnlyAdmissions has no ListAllEnabled, as an older backend would.
type runViewOnlyAdmissions struct{}

func (runViewOnlyAdmissions) ListEnabled(context.Context, []string) ([]*registry.AgentActionAdmission, error) {
	return nil, nil
}

func ownerActionsVerifier(t *testing.T) *ControlModelActionViewVerifier {
	t.Helper()
	public, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	return verifier
}

func ownerActionsUser() authctx.Principal {
	return authctx.Principal{
		OrganizationID:         "org-1",
		ActorID:                "user-1",
		PrincipalType:          "user",
		RetentionPolicyPresent: true,
	}
}

func getAdmittedOwnerActions(handler *CapabilitiesHandler, principal *authctx.Principal, method string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, ownerActionsAdmittedPath, nil)
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	recorder := httptest.NewRecorder()
	mux := http.NewServeMux()
	handler.Register(mux)
	mux.ServeHTTP(recorder, request)
	return recorder
}

func decodeAdmitted(t *testing.T, recorder *httptest.ResponseRecorder) (string, []string, map[string]json.RawMessage) {
	t.Helper()
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	var envelope struct {
		Data map[string]json.RawMessage `json:"data"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &envelope); err != nil {
		t.Fatalf("decode: %v (%s)", err, recorder.Body.String())
	}
	var mode string
	var ids []string
	if err := json.Unmarshal(envelope.Data["mode"], &mode); err != nil {
		t.Fatalf("mode: %v", err)
	}
	if err := json.Unmarshal(envelope.Data["action_ids"], &ids); err != nil {
		t.Fatalf("action_ids: %v", err)
	}
	return mode, ids, envelope.Data
}

func TestAdmittedOwnerActionsTicketsOnlyWhileTheTableIsOff(t *testing.T) {
	// The table store holds more enabled rows, but with the switch off only tickets.create is offered.
	store := &listingAdmissions{rows: []*registry.AgentActionAdmission{
		{ActionID: "zeta.do", Status: registry.AgentActionAdmissionEnabled},
		{ActionID: "tickets.create", Status: registry.AgentActionAdmissionEnabled},
	}}
	handler := (&CapabilitiesHandler{}).WithModelActionViewVerifier(ownerActionsVerifier(t).WithAgentActionAdmissions(store, false))
	user := ownerActionsUser()

	mode, ids, data := decodeAdmitted(t, getAdmittedOwnerActions(handler, &user, http.MethodGet))
	if mode != "tickets_only" || !reflect.DeepEqual(ids, []string{"tickets.create"}) {
		t.Fatalf("mode=%q ids=%v; want tickets_only [tickets.create]", mode, ids)
	}
	if len(data) != 2 {
		t.Fatalf("data carries %d fields, want only mode and action_ids: %v", len(data), data)
	}
}

func TestAdmittedOwnerActionsListsEveryEnabledRowSortedInTableMode(t *testing.T) {
	store := &listingAdmissions{rows: []*registry.AgentActionAdmission{
		{ActionID: "zeta.do", Status: registry.AgentActionAdmissionEnabled, ParametersJSON: `{"secret":"schema"}`, Description: "describes zeta"},
		{ActionID: "declared.only", Status: "declared"},
		{ActionID: "alpha.do", Status: registry.AgentActionAdmissionEnabled},
		{ActionID: "retired.one", Status: "retired"},
		{ActionID: "  ", Status: registry.AgentActionAdmissionEnabled},
		nil,
		{ActionID: "tickets.create", Status: registry.AgentActionAdmissionEnabled},
	}}
	handler := (&CapabilitiesHandler{}).WithModelActionViewVerifier(ownerActionsVerifier(t).WithAgentActionAdmissions(store, true))
	user := ownerActionsUser()

	recorder := getAdmittedOwnerActions(handler, &user, http.MethodGet)
	mode, ids, data := decodeAdmitted(t, recorder)
	want := []string{"alpha.do", "tickets.create", "zeta.do"}
	if mode != "table" || !reflect.DeepEqual(ids, want) {
		t.Fatalf("mode=%q ids=%v; want table %v", mode, ids, want)
	}
	if !sort.StringsAreSorted(ids) {
		t.Fatalf("ids not sorted: %v", ids)
	}
	if len(data) != 2 {
		t.Fatalf("data carries %d fields, want only mode and action_ids", len(data))
	}
	body := recorder.Body.String()
	for _, leaked := range []string{"secret", "schema", "describes zeta", "declared.only", "retired.one"} {
		if strings.Contains(body, leaked) {
			t.Fatalf("listing leaks %q: %s", leaked, body)
		}
	}
}

func TestAdmittedOwnerActionsTableModeWithNoEnabledRowIsAnEmptyList(t *testing.T) {
	handler := (&CapabilitiesHandler{}).WithModelActionViewVerifier(ownerActionsVerifier(t).WithAgentActionAdmissions(&listingAdmissions{}, true))
	user := ownerActionsUser()
	recorder := getAdmittedOwnerActions(handler, &user, http.MethodGet)
	mode, ids, _ := decodeAdmitted(t, recorder)
	if mode != "table" || len(ids) != 0 {
		t.Fatalf("mode=%q ids=%v; want table and no ids", mode, ids)
	}
	// An empty list is a JSON array, never null, so a client can iterate it.
	if !strings.Contains(recorder.Body.String(), `"action_ids":[]`) {
		t.Fatalf("empty list is not []: %s", recorder.Body.String())
	}
}

func TestAdmittedOwnerActionsFailsClosed(t *testing.T) {
	user := ownerActionsUser()
	zdr := ownerActionsUser()
	zdr.ZeroDataRetention = true
	noOrg := ownerActionsUser()
	noOrg.OrganizationID = ""

	tableOn := func(backend agentActionAdmissionBackend) *CapabilitiesHandler {
		return (&CapabilitiesHandler{}).WithModelActionViewVerifier(ownerActionsVerifier(t).WithAgentActionAdmissions(backend, true))
	}
	tableOff := (&CapabilitiesHandler{}).WithModelActionViewVerifier(ownerActionsVerifier(t))

	cases := []struct {
		name      string
		handler   *CapabilitiesHandler
		principal *authctx.Principal
		method    string
		want      int
	}{
		{"no principal", tableOff, nil, http.MethodGet, http.StatusForbidden},
		{"principal without org", tableOff, &noOrg, http.MethodGet, http.StatusForbidden},
		{"zdr principal, table off", tableOff, &zdr, http.MethodGet, http.StatusForbidden},
		{"zdr principal, table on", tableOn(&listingAdmissions{}), &zdr, http.MethodGet, http.StatusForbidden},
		{"no Control verifier", &CapabilitiesHandler{}, &user, http.MethodGet, http.StatusServiceUnavailable},
		{"table on, backend cannot list", tableOn(runViewOnlyAdmissions{}), &user, http.MethodGet, http.StatusServiceUnavailable},
		{"table on, store error", tableOn(&listingAdmissions{err: errors.New("db down")}), &user, http.MethodGet, http.StatusServiceUnavailable},
		{"post is not allowed", tableOff, &user, http.MethodPost, http.StatusMethodNotAllowed},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			recorder := getAdmittedOwnerActions(tc.handler, tc.principal, tc.method)
			if recorder.Code != tc.want {
				t.Fatalf("status = %d, want %d, body = %s", recorder.Code, tc.want, recorder.Body.String())
			}
			if strings.Contains(recorder.Body.String(), "tickets.create") {
				t.Fatalf("refusal names an action: %s", recorder.Body.String())
			}
		})
	}
}

// The route is a read: the HTTP authorizer lets an org member read it and keeps a
// conflicting org query out, like every other capability read.
func TestAdmittedOwnerActionsPassesTheHTTPAuthorizerForAMember(t *testing.T) {
	user := ownerActionsUser()
	if err := authz.AuthorizeHTTP(user, httptest.NewRequest(http.MethodGet, ownerActionsAdmittedPath, nil)); err != nil {
		t.Fatalf("org member denied: %v", err)
	}
	if err := authz.AuthorizeHTTP(user, httptest.NewRequest(http.MethodGet, ownerActionsAdmittedPath+"?org_id=other-org", nil)); err == nil {
		t.Fatal("a conflicting org_id query was admitted")
	}
}
