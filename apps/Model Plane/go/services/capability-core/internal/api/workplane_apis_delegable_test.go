package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// delegableRow is one capability_tools row the delegable tool list reads.
type delegableRow struct {
	tool, capability, class, floor, refinement string
}

// delegableDB answers the delegable list's one statement with rows and records its arguments. Any other statement
// fails the test: the list writes nothing and reads only the registry.
type delegableDB struct {
	t       *testing.T
	rows    []delegableRow
	queries []databaseCall
}

func (db *delegableDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	db.t.Fatal("the delegable tool list writes nothing")
	return pgconn.CommandTag{}, nil
}

func (db *delegableDB) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	db.queries = append(db.queries, databaseCall{query: query, args: append([]any(nil), args...)})
	return &delegableRows{rows: db.rows, next: -1}, nil
}

func (db *delegableDB) QueryRow(context.Context, string, ...any) pgx.Row {
	db.t.Fatal("the delegable tool list reads one statement")
	return errorRow{err: pgx.ErrNoRows}
}

// delegableRows yields the rows in the order the fake was given them, which is not the order the list sorts by.
type delegableRows struct {
	rows []delegableRow
	next int
}

func (rows *delegableRows) Next() bool {
	rows.next++
	return rows.next < len(rows.rows)
}

func (rows *delegableRows) Scan(dest ...any) error {
	row := rows.rows[rows.next]
	values := []string{row.tool, row.capability, row.class, row.floor, row.refinement}
	for i, target := range dest {
		*target.(*string) = values[i]
	}
	return nil
}

func (rows *delegableRows) Err() error                                   { return nil }
func (rows *delegableRows) Close()                                       {}
func (rows *delegableRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (rows *delegableRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (rows *delegableRows) Values() ([]any, error)                       { return nil, nil }
func (rows *delegableRows) RawValues() [][]byte                          { return nil }
func (rows *delegableRows) Conn() *pgx.Conn                              { return nil }

// delegableMemberPrincipal is a non-ZDR member of org-a with a stated retention posture.
func delegableMemberPrincipal() authctx.Principal {
	return authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user", RetentionPolicyPresent: true}
}

// delegableErrorCode reads the code of a typed error envelope {error:{code,message,details}}.
func delegableErrorCode(t *testing.T, response *httptest.ResponseRecorder) string {
	t.Helper()
	var reply struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &reply); err != nil {
		t.Fatalf("error body is not the typed envelope: %s", response.Body.String())
	}
	return reply.Error.Code
}

// serveDelegableAt answers one request at path on the cron routes, as protectedMux mounts them, for principal.
func serveDelegableAt(t *testing.T, db *delegableDB, scheduleContract bool, method, path string, principal authctx.Principal) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewCronHandler(db).WithScheduleContract(scheduleContract).Register(mux)
	request := httptest.NewRequest(method, path, nil)
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, request)
	return response
}

// serveDelegable answers one request on the list path for a member of org-a.
func serveDelegable(t *testing.T, db *delegableDB, scheduleContract bool, method string) *httptest.ResponseRecorder {
	t.Helper()
	return serveDelegableAt(t, db, scheduleContract, method, "/api/v1/cron/delegable-tools", delegableMemberPrincipal())
}

// The list names exactly the tools a schedule's contract may name, sorted by tool name whatever order the registry
// returns, with a tool declared by both the seed and the org listed once. The answer carries the tools and the two
// window bounds and nothing else.
func TestDelegableToolsListsOnlyWhatAContractMayName(t *testing.T) {
	db := &delegableDB{t: t, rows: []delegableRow{
		{tool: "zeta_read", capability: "cap.zeta", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "alpha_read", capability: "cap.alpha", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "web_search", capability: "cap.tool.web_search", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "web_search", capability: "cap.tool.web_search", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		// Not a read tool.
		{tool: "sandbox_run", capability: "cap.sandbox", class: models.SideEffectSandboxCompute, floor: models.ApprovalFloorNone},
		// An approval floor, and a refinement, each hide a tool.
		{tool: "floor_read", capability: "cap.floor", class: models.SideEffectRead, floor: models.ApprovalFloorAsk},
		{tool: "refined_read", capability: "cap.refined", class: models.SideEffectRead, floor: models.ApprovalFloorNone, refinement: "domain=example.com"},
		// The non-delegable families and names hide a tool whatever their rows say.
		{tool: "browser.open", capability: "cap.browser", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "process_list", capability: "cap.process", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "mcp__github__list", capability: "cap.mcp", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "memory.write", capability: "cap.memory", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		// A stricter org row hides the tool behind a permissive global one.
		{tool: "org_stricter", capability: "cap.org", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "org_stricter", capability: "cap.org", class: models.SideEffectRead, floor: models.ApprovalFloorAsk},
		// Rows that name two capabilities hide the tool, since a contract could name only one.
		{tool: "two_caps", capability: "cap.one", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
		{tool: "two_caps", capability: "cap.two", class: models.SideEffectRead, floor: models.ApprovalFloorNone},
	}}

	response := serveDelegable(t, db, true, http.MethodGet)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200 (%s)", response.Code, response.Body.String())
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(response.Body.Bytes(), &raw); err != nil {
		t.Fatalf("response is not an object: %v (%s)", err, response.Body.String())
	}
	if len(raw) != 3 || raw["tools"] == nil || raw["max_days"] == nil || raw["default_days"] == nil {
		t.Fatalf("response keys are not exactly tools, max_days and default_days: %s", response.Body.String())
	}
	var body struct {
		Tools []struct {
			Tool       string `json:"tool"`
			Capability string `json:"capability"`
		} `json:"tools"`
		MaxDays     int `json:"max_days"`
		DefaultDays int `json:"default_days"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatalf("response is not the tools answer: %v (%s)", err, response.Body.String())
	}
	want := []string{"alpha_read", "web_search", "zeta_read"}
	if len(body.Tools) != len(want) {
		t.Fatalf("listed %+v, want exactly %v", body.Tools, want)
	}
	for i, tool := range want {
		if body.Tools[i].Tool != tool {
			t.Fatalf("entry %d = %q, want %q (sorted by tool name)", i, body.Tools[i].Tool, tool)
		}
	}
	if body.Tools[1].Capability != "cap.tool.web_search" {
		t.Fatalf("web_search capability = %q", body.Tools[1].Capability)
	}
	if body.MaxDays != cron.ContractMaxDays || body.DefaultDays != cron.ContractDefaultDays {
		t.Fatalf("window bounds = %d and %d, want %d and %d", body.MaxDays, body.DefaultDays, cron.ContractMaxDays, cron.ContractDefaultDays)
	}
	if len(db.queries) != 1 || db.queries[0].args[0] != "org-a" {
		t.Fatalf("registry reads = %+v, want one read for the verified org", db.queries)
	}
}

// With no declared rows the answer is an empty list, not null, so the client reads it as an empty list.
func TestDelegableToolsAnswersAnEmptyListWithoutRows(t *testing.T) {
	db := &delegableDB{t: t}
	response := serveDelegable(t, db, true, http.MethodGet)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200 (%s)", response.Code, response.Body.String())
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(response.Body.Bytes(), &raw); err != nil {
		t.Fatalf("response is not an object: %v", err)
	}
	if string(raw["tools"]) != "[]" {
		t.Fatalf("tools = %s, want an empty list", raw["tools"])
	}
}

// While SCHEDULE_CONTRACT_ENABLED is off the list is a typed not found and the registry is not read.
func TestDelegableToolsIsNotFoundWhileContractsAreOff(t *testing.T) {
	db := &delegableDB{t: t, rows: []delegableRow{{tool: "web_search", capability: "cap.tool.web_search", class: models.SideEffectRead, floor: models.ApprovalFloorNone}}}
	response := serveDelegable(t, db, false, http.MethodGet)
	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 (%s)", response.Code, response.Body.String())
	}
	if got := delegableErrorCode(t, response); got != "not_found" {
		t.Fatalf("error code = %q, want not_found", got)
	}
	if len(db.queries) != 0 {
		t.Fatalf("the registry was read while contracts are off: %+v", db.queries)
	}
}

// The list is a GET. Any other method answers 405 and names GET as the one it takes, whether or not contracts are on,
// and the registry is not read.
func TestDelegableToolsAcceptsOnlyGET(t *testing.T) {
	for _, method := range []string{http.MethodPost, http.MethodPut, http.MethodPatch, http.MethodDelete} {
		for _, scheduleContract := range []bool{true, false} {
			db := &delegableDB{t: t}
			response := serveDelegable(t, db, scheduleContract, method)
			if response.Code != http.StatusMethodNotAllowed {
				t.Fatalf("%s (contracts %v) status = %d, want 405", method, scheduleContract, response.Code)
			}
			if got := response.Header().Get("Allow"); got != http.MethodGet {
				t.Fatalf("%s Allow = %q, want GET", method, got)
			}
			if len(db.queries) != 0 {
				t.Fatalf("%s read the registry: %+v", method, db.queries)
			}
		}
	}
}

// The schedule subtree never reads the list path as a schedule id: neither the bare path under the subtree nor its
// /fires or /run forms reach a schedule read or a fire, whatever the method, and the registry is not read.
func TestDelegableToolsCatchAllNeverReadsTheListPathAsAScheduleID(t *testing.T) {
	db := &delegableDB{t: t, rows: []delegableRow{{tool: "web_search", capability: "cap.tool.web_search", class: models.SideEffectRead, floor: models.ApprovalFloorNone}}}
	cases := []struct{ method, path string }{
		{http.MethodGet, "/api/v1/cron/delegable-tools/"},
		{http.MethodGet, "/api/v1/cron/delegable-tools/fires"},
		{http.MethodGet, "/api/v1/cron/delegable-tools/x"},
		{http.MethodPost, "/api/v1/cron/delegable-tools/run"},
		{http.MethodPatch, "/api/v1/cron/delegable-tools/x"},
	}
	for _, c := range cases {
		response := serveDelegableAt(t, db, true, c.method, c.path, delegableMemberPrincipal())
		if response.Code != http.StatusNotFound {
			t.Fatalf("%s %s status = %d, want 404 (%s)", c.method, c.path, response.Code, response.Body.String())
		}
	}
	if len(db.queries) != 0 {
		t.Fatalf("a path under the list read the registry or a schedule: %+v", db.queries)
	}
}

// A ZDR token can create no contract, so the list answers 200 with an empty tools list, not a refusal, and the registry
// is not read. The window bounds are still named.
func TestDelegableToolsAnswersAZDRTokenAnEmptyList(t *testing.T) {
	db := &delegableDB{t: t, rows: []delegableRow{{tool: "web_search", capability: "cap.tool.web_search", class: models.SideEffectRead, floor: models.ApprovalFloorNone}}}
	zdr := delegableMemberPrincipal()
	zdr.ZeroDataRetention = true
	response := serveDelegableAt(t, db, true, http.MethodGet, "/api/v1/cron/delegable-tools", zdr)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200 (%s)", response.Code, response.Body.String())
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(response.Body.Bytes(), &raw); err != nil {
		t.Fatalf("response is not an object: %v", err)
	}
	if string(raw["tools"]) != "[]" {
		t.Fatalf("tools = %s, want an empty list", raw["tools"])
	}
	if string(raw["max_days"]) != "90" || string(raw["default_days"]) != "30" {
		t.Fatalf("window bounds = %s and %s, want 90 and 30", raw["max_days"], raw["default_days"])
	}
	if len(db.queries) != 0 {
		t.Fatalf("a ZDR token read the registry: %+v", db.queries)
	}
}
