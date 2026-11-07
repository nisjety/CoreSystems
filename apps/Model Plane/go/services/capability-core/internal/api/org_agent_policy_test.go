package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

// orgAgentFakeDB fails the test on any read or write unless the case expects one. Its
// QueryRow answers with row, so a stale write or a missing row can be staged.
type orgAgentFakeDB struct {
	t   *testing.T
	row pgx.Row
}

func (d orgAgentFakeDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Helper()
	d.t.Fatal("the registry was written for a request that had to be refused")
	return pgconn.CommandTag{}, nil
}

func (d orgAgentFakeDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Helper()
	d.t.Fatal("the registry was listed for a request that had to be refused")
	return nil, nil
}

func (d orgAgentFakeDB) QueryRow(context.Context, string, ...any) pgx.Row {
	if d.row == nil {
		d.t.Helper()
		d.t.Fatal("the registry was read or written for a request that had to be refused")
	}
	return d.row
}

type orgAgentErrRow struct{ err error }

func (r orgAgentErrRow) Scan(...any) error { return r.err }

func orgAgentAdmin() authctx.Principal {
	return authctx.Principal{OrganizationID: "org-a", ActorID: "user-admin", PrincipalType: "user", Scopes: []string{authz.WriteScope, "org:admin"}, RetentionPolicyPresent: true}
}

func orgAgentMember() authctx.Principal {
	return authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true}
}

func serveOrgAgentPolicy(t *testing.T, enabled bool, db registryDatabase, principal *authctx.Principal, method, target, ifMatch, body string) (*httptest.ResponseRecorder, *OrgAgentPolicyHandler) {
	t.Helper()
	handler := NewOrgAgentPolicyHandler(db, enabled)
	mux := http.NewServeMux()
	handler.Register(mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	if ifMatch != "" {
		request.Header.Set("If-Match", ifMatch)
	}
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response, handler
}

func orgAgentErrorCode(t *testing.T, response *httptest.ResponseRecorder) string {
	t.Helper()
	var reply map[string]string
	if err := json.Unmarshal(response.Body.Bytes(), &reply); err != nil {
		t.Fatalf("error body is not JSON: %s", response.Body.String())
	}
	return reply["error"]
}

func TestOrgAgentPolicyAPIIsOnlyTheLiteralTrue(t *testing.T) {
	for raw, want := range map[string]bool{"true": true, "1": false, "TRUE": false, " true": false, "yes": false, "": false} {
		got := OrgAgentPolicyAPIFromLookup(func(string) string { return raw })
		if got != want {
			t.Errorf("ORG_AGENT_POLICY_API=%q enabled = %v, want %v", raw, got, want)
		}
	}
}

// While the switch is off every route is a 404 before anything is read or written.
func TestOrgAgentPolicyRoutesAreNotFoundWhileTheAPIIsOff(t *testing.T) {
	admin := orgAgentAdmin()
	tests := []struct{ method, target, body string }{
		{http.MethodGet, "/api/v1/org-agent-policy", ""},
		{http.MethodPut, "/api/v1/org-agent-policy", `{"settings":{}}`},
		{http.MethodGet, "/api/v1/org-agent-policy/effective?user=me", ""},
		{http.MethodGet, "/api/v1/org-agent-policy/history", ""},
	}
	for _, tc := range tests {
		response, _ := serveOrgAgentPolicy(t, false, orgAgentFakeDB{t: t}, &admin, tc.method, tc.target, "0", tc.body)
		if response.Code != http.StatusNotFound {
			t.Fatalf("%s %s with the API off = %d, want 404", tc.method, tc.target, response.Code)
		}
	}
}

func TestOrgAgentPolicyWritesAreRefusedBeforeTheRegistryIsTouched(t *testing.T) {
	zdr := orgAgentAdmin()
	zdr.ZeroDataRetention = true
	unstated := orgAgentAdmin()
	unstated.RetentionPolicyPresent = false
	workload := orgAgentAdmin()
	workload.PrincipalType = "service"
	noWrite := orgAgentAdmin()
	noWrite.Scopes = []string{"org:admin"}
	member := orgAgentMember()
	memberWriter := orgAgentMember()
	memberWriter.Scopes = []string{authz.WriteScope}
	admin := orgAgentAdmin()
	body := `{"settings":{"mode_cap":"read_only"}}`

	tests := []struct {
		name      string
		principal *authctx.Principal
		ifMatch   string
		body      string
		status    int
		code      string
	}{
		{name: "no principal", principal: nil, ifMatch: "0", body: body, status: http.StatusUnauthorized, code: "authentication required"},
		{name: "a ZDR admin may not write", principal: &zdr, ifMatch: "0", body: body, status: http.StatusForbidden, code: "org_agent_policy_write_under_zdr_refused"},
		{name: "an unstated posture may not write", principal: &unstated, ifMatch: "0", body: body, status: http.StatusForbidden, code: "org_agent_policy_write_under_zdr_refused"},
		{name: "a workload may not write", principal: &workload, ifMatch: "0", body: body, status: http.StatusForbidden, code: "only a person may set the org agent policy"},
		{name: "an admin without the write scope may not write", principal: &noWrite, ifMatch: "0", body: body, status: http.StatusForbidden, code: "capability write scope required"},
		{name: "a member without the write scope may not write", principal: &member, ifMatch: "0", body: body, status: http.StatusForbidden, code: "capability write scope required"},
		{name: "a member with the write scope may not write", principal: &memberWriter, ifMatch: "0", body: body, status: http.StatusForbidden, code: "only an admin may set the org agent policy"},
		{name: "a write without If-Match is refused", principal: &admin, ifMatch: "", body: body, status: http.StatusPreconditionRequired, code: "if_match_required"},
		{name: "a malformed If-Match is refused", principal: &admin, ifMatch: "latest", body: body, status: http.StatusBadRequest, code: "invalid_if_match"},
		{name: "a body without settings is refused", principal: &admin, ifMatch: "0", body: `{"mode_cap":"read_only"}`, status: http.StatusBadRequest, code: "invalid request body"},
		{name: "trailing JSON after the body is refused", principal: &admin, ifMatch: "0", body: `{"settings":{}} {}`, status: http.StatusBadRequest, code: "invalid request body"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			response, _ := serveOrgAgentPolicy(t, true, orgAgentFakeDB{t: t}, tc.principal, http.MethodPut, "/api/v1/org-agent-policy", tc.ifMatch, tc.body)
			if response.Code != tc.status {
				t.Fatalf("status = %d, want %d (%s)", response.Code, tc.status, response.Body.String())
			}
			if got := orgAgentErrorCode(t, response); got != tc.code {
				t.Fatalf("error = %q, want %q", got, tc.code)
			}
		})
	}
}

func TestOrgAgentPolicyValidationRefusesBeforeTheRegistryIsTouched(t *testing.T) {
	admin := orgAgentAdmin()
	tests := []struct {
		name   string
		body   string
		status int
		code   string
	}{
		{name: "an unknown key", body: `{"settings":{"nope":1}}`, status: http.StatusBadRequest, code: "unknown_key"},
		{name: "an enum outside its order", body: `{"settings":{"mode_cap":"full_access"}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a bool written as text", body: `{"settings":{"lockdown":"yes"}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a reserved plugins key with a non-default value", body: `{"settings":{"ext.plugins":{"mode":"none","ids":[]}}}`, status: http.StatusUnprocessableEntity, code: "not_enforced"},
		{name: "a reserved hooks key with a non-default value", body: `{"settings":{"user_hooks":"denied"}}`, status: http.StatusUnprocessableEntity, code: "not_enforced"},
		{name: "a reserved reference uploads key with a non-default value", body: `{"settings":{"reference_uploads":"on"}}`, status: http.StatusUnprocessableEntity, code: "not_enforced"},
		{name: "ids under a mode that names none", body: `{"settings":{"ext.mcp_servers":{"mode":"any","ids":["srv.one"]}}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "an id that is not an identifier", body: `{"settings":{"ext.mcp_servers":{"mode":"approved_only","ids":["has space"]}}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "an allow-list with an extra field", body: `{"settings":{"ext.mcp_servers":{"mode":"approved_only","ids":["srv.one"],"extra":1}}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a fan-out cap written as text", body: `{"settings":{"research.max_fan_out_items":"8"}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a fan-out cap that is a fraction", body: `{"settings":{"research.max_fan_out_items":8.5}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a negative fan-out cap", body: `{"settings":{"research.max_fan_out_items":-1}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a fan-out cap past sixteen", body: `{"settings":{"research.max_fan_out_items":17}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a reserved web mode", body: `{"settings":{"web.mode":"cached_only"}}`, status: http.StatusUnprocessableEntity, code: "not_enforced"},
		{name: "a web mode outside its order", body: `{"settings":{"web.mode":"full"}}`, status: http.StatusBadRequest, code: "invalid_value"},
		{name: "a paid switch written as a bool", body: `{"settings":{"web.paid_providers":true}}`, status: http.StatusBadRequest, code: "invalid_value"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			response, _ := serveOrgAgentPolicy(t, true, orgAgentFakeDB{t: t}, &admin, http.MethodPut, "/api/v1/org-agent-policy", "0", tc.body)
			if response.Code != tc.status {
				t.Fatalf("status = %d, want %d (%s)", response.Code, tc.status, response.Body.String())
			}
			if got := orgAgentErrorCode(t, response); got != tc.code {
				t.Fatalf("error = %q, want %q", got, tc.code)
			}
		})
	}
}

// A value looser than the platform ceiling is refused; a stricter one is not.
func TestOrgAgentPolicyLooseningBeyondTheCeilingIsRefused(t *testing.T) {
	admin := orgAgentAdmin()
	handler := NewOrgAgentPolicyHandler(orgAgentFakeDB{t: t}, true)
	handler.ceiling["mode_cap"] = "ask_first"
	mux := http.NewServeMux()
	handler.Register(mux)

	loosen := httptest.NewRequest(http.MethodPut, "/api/v1/org-agent-policy", strings.NewReader(`{"settings":{"mode_cap":"auto"}}`))
	loosen.Header.Set("If-Match", "0")
	loosen = loosen.WithContext(authctx.ContextWithPrincipal(loosen.Context(), admin))
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, loosen)
	if response.Code != http.StatusUnprocessableEntity || orgAgentErrorCode(t, response) != "exceeds_platform_ceiling" {
		t.Fatalf("loosening = %d %s, want 422 exceeds_platform_ceiling", response.Code, response.Body.String())
	}
}

// A stale or raced revision is answered 409 and nothing is replaced.
func TestOrgAgentPolicyStaleRevisionIsAConflict(t *testing.T) {
	admin := orgAgentAdmin()
	body := `{"settings":{"mode_cap":"read_only"}}`
	db := orgAgentFakeDB{t: t, row: orgAgentErrRow{err: pgx.ErrNoRows}}
	response, _ := serveOrgAgentPolicy(t, true, db, &admin, http.MethodPut, "/api/v1/org-agent-policy", "3", body)
	if response.Code != http.StatusConflict || orgAgentErrorCode(t, response) != "stale_revision" {
		t.Fatalf("stale write = %d %s, want 409 stale_revision", response.Code, response.Body.String())
	}
}

// An org with no row reads the platform defaults at revision 0 with no updated_at.
func TestOrgAgentPolicyReadsTheDefaultsWhenNoRowExists(t *testing.T) {
	member := orgAgentMember()
	db := orgAgentFakeDB{t: t, row: orgAgentErrRow{err: pgx.ErrNoRows}}
	response, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, "/api/v1/org-agent-policy", "", "")
	if response.Code != http.StatusOK {
		t.Fatalf("GET = %d %s", response.Code, response.Body.String())
	}
	var view struct {
		Revision  int64          `json:"revision"`
		Settings  map[string]any `json:"settings"`
		UpdatedAt *string        `json:"updated_at"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if view.Revision != 0 || view.UpdatedAt != nil {
		t.Fatalf("revision %d, updated_at %v; want 0 and null", view.Revision, view.UpdatedAt)
	}
	if !reflect.DeepEqual(view.Settings["mode_cap"], "auto") || len(view.Settings) != len(orgAgentPolicySettings) {
		t.Fatalf("settings = %v", view.Settings)
	}
	if !reflect.DeepEqual(view.Settings["reference_uploads"], "off") {
		t.Fatalf("reference_uploads = %v, want the default off", view.Settings["reference_uploads"])
	}
}

func TestOrgAgentPolicyEffectiveNamesTheLayerAndTheStatus(t *testing.T) {
	member := orgAgentMember()
	db := orgAgentFakeDB{t: t, row: orgAgentErrRow{err: pgx.ErrNoRows}}
	response, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, "/api/v1/org-agent-policy/effective?user=me", "", "")
	if response.Code != http.StatusOK {
		t.Fatalf("effective = %d %s", response.Code, response.Body.String())
	}
	var view struct {
		Settings []struct {
			Key    string `json:"key"`
			Status string `json:"status"`
			Source string `json:"source"`
		} `json:"settings"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if len(view.Settings) != len(orgAgentPolicySettings) {
		t.Fatalf("keys = %d, want %d", len(view.Settings), len(orgAgentPolicySettings))
	}
	for _, row := range view.Settings {
		if row.Source != "platform_default" {
			t.Errorf("%s source = %q at revision 0", row.Key, row.Source)
		}
		if row.Key == "ext.skills" && row.Status != "reserved" {
			t.Errorf("ext.skills status = %q, want reserved", row.Status)
		}
	}
	other, _ := serveOrgAgentPolicy(t, true, db, &member, http.MethodGet, "/api/v1/org-agent-policy/effective?user=someone-else", "", "")
	if other.Code != http.StatusBadRequest {
		t.Fatalf("effective for another user = %d, want 400", other.Code)
	}
}

// The validator accepts the defaults, and writes an allow-list in canonical form: ids
// sorted and de-duplicated, an omitted key taking its default.
func TestOrgAgentPolicyValidationCanonicalForm(t *testing.T) {
	ceiling := loosestOrgAgentCeiling()
	settings, refusal := validateOrgAgentSettings(map[string]any{}, ceiling)
	if refusal != nil {
		t.Fatalf("defaults refused: %s", refusal.code)
	}
	if !reflect.DeepEqual(settings, orgAgentPolicyDefaults()) {
		t.Fatalf("empty document is not the defaults: %v", settings)
	}
	written := map[string]any{"ext.mcp_servers": map[string]any{"mode": "approved_only", "ids": []any{"zeta", "alpha", "alpha"}}}
	settings, refusal = validateOrgAgentSettings(written, ceiling)
	if refusal != nil {
		t.Fatalf("allow-list refused: %s", refusal.code)
	}
	want := map[string]any{"mode": "approved_only", "ids": []string{"alpha", "zeta"}}
	if !reflect.DeepEqual(settings["ext.mcp_servers"], want) {
		t.Fatalf("allow-list = %v, want %v", settings["ext.mcp_servers"], want)
	}
}

func TestOrgAgentPolicyDigestFollowsTheDocument(t *testing.T) {
	defaults := orgAgentPolicyDefaults()
	first, _, err := orgAgentPolicyDigest(defaults)
	if err != nil {
		t.Fatal(err)
	}
	again, _, _ := orgAgentPolicyDigest(orgAgentPolicyDefaults())
	if first != again {
		t.Fatal("the same document has two digests")
	}
	defaults["lockdown"] = true
	changed, _, _ := orgAgentPolicyDigest(defaults)
	if changed == first {
		t.Fatal("a changed document kept its digest")
	}
}

// The wide-research item cap (EP-2 S8) is a whole number: its default is 8, a stored value reads
// back as the same token, 0 turns wide research off, and a value above the ceiling is refused.
func TestOrgAgentPolicyFanOutItemsIsATightenOnlyCount(t *testing.T) {
	const key = "research.max_fan_out_items"
	if got := orgAgentPolicyDefaults()[key]; !reflect.DeepEqual(got, 8) {
		t.Fatalf("default = %#v, want 8", got)
	}
	ceiling := loosestOrgAgentCeiling()
	if ceiling[key] != "16" {
		t.Fatalf("loosest = %q, want 16", ceiling[key])
	}
	// encoding/json hands the handler a float64, as a PUT body decodes.
	settings, refusal := validateOrgAgentSettings(map[string]any{key: float64(0)}, ceiling)
	if refusal != nil || !reflect.DeepEqual(settings[key], 0) {
		t.Fatalf("0 = %#v %v, want the stored count 0", settings[key], refusal)
	}
	if token := orgAgentTokenOf(float64(5)); token != "5" {
		t.Fatalf("a stored 5 reads back as %q", token)
	}
	ceiling[key] = "4"
	if _, refusal := validateOrgAgentSettings(map[string]any{key: float64(10)}, ceiling); refusal == nil || refusal.code != "exceeds_platform_ceiling" {
		t.Fatalf("10 under a ceiling of 4 = %v, want exceeds_platform_ceiling", refusal)
	}
	if _, refusal := validateOrgAgentSettings(map[string]any{key: float64(3)}, ceiling); refusal != nil {
		t.Fatalf("3 under a ceiling of 4 refused: %s", refusal.code)
	}
}

// The org's web switches (EP-9 W2) are two enforced keys: web access takes off or live, paid search
// takes off or on, and cached_only is refused until Quarry ships it. A write looser than the ceiling is refused.
func TestOrgAgentPolicyWebSwitchesTakeOffOrLive(t *testing.T) {
	ceiling := loosestOrgAgentCeiling()
	for _, body := range []map[string]any{
		{"web.mode": "off"},
		{"web.mode": "live", "web.paid_providers": "off"},
	} {
		if _, refusal := validateOrgAgentSettings(body, ceiling); refusal != nil {
			t.Fatalf("%v refused: %s", body, refusal.code)
		}
	}
	settings, refusal := validateOrgAgentSettings(map[string]any{"web.mode": "off"}, ceiling)
	if refusal != nil || settings["web.paid_providers"] != "on" {
		t.Fatalf("settings = %v, refusal = %v; paid search should take its default on", settings, refusal)
	}
	strict := loosestOrgAgentCeiling()
	strict["web.mode"] = "off"
	if _, refusal := validateOrgAgentSettings(map[string]any{"web.mode": "live"}, strict); refusal == nil || refusal.code != "exceeds_platform_ceiling" {
		t.Fatalf("live under an off ceiling = %v, want exceeds_platform_ceiling", refusal)
	}
}

// The golden holds the schema as data. This table must match it key by key, in order,
// with the same kinds, orders, defaults, fail-closed values, status and reserved values.
func TestOrgAgentPolicySchemaMatchesTheGolden(t *testing.T) {
	raw, err := os.ReadFile("../../../../../api/contracts/org-agent-policy-schema.json")
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden struct {
		Settings []struct {
			Key             string   `json:"key"`
			Kind            string   `json:"kind"`
			StrictnessOrder []string `json:"strictness_order"`
			Default         string   `json:"default"`
			FailClosedValue string   `json:"fail_closed_value"`
			Status          string   `json:"status"`
			ReservedValues  []string `json:"reserved_values"`
		} `json:"settings"`
		Refusals []string `json:"refusals"`
	}
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("golden is not JSON: %v", err)
	}
	if len(golden.Settings) != len(orgAgentPolicySettings) {
		t.Fatalf("golden has %d keys, the table %d", len(golden.Settings), len(orgAgentPolicySettings))
	}
	for i, row := range golden.Settings {
		def := orgAgentPolicySettings[i]
		status := "enforced"
		if def.reserved {
			status = "reserved"
		}
		if row.Key != def.key || row.Kind != def.kind || !reflect.DeepEqual(row.StrictnessOrder, def.order) ||
			row.Default != def.def || row.FailClosedValue != def.failClose || row.Status != status {
			t.Fatalf("key %s differs from the golden row %+v", def.key, row)
		}
		if !slices.Equal(row.ReservedValues, def.reservedValues) {
			t.Fatalf("reserved values of %s differ from the golden: %v", def.key, row.ReservedValues)
		}
	}
	codes := []string{orgAgentRefuseUnknownKey.code, orgAgentRefuseInvalidValue.code, orgAgentRefuseNotEnforced.code, orgAgentRefuseExceedsCeiling.code}
	if !reflect.DeepEqual(golden.Refusals, codes) {
		t.Fatalf("refusals = %v, golden %v", codes, golden.Refusals)
	}
}
