package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// bindingRow is the one mcp_servers row the credential-binding tests drive.
type bindingRow struct {
	id, orgID, name, description, endpointURL, transport, authKind string
	config                                                         []byte
	scope, rolloutState, riskLevel                                 string
	enabled                                                        bool
	createdAt, updatedAt                                           time.Time
}

// bindingDatabase holds one mcp_servers row and answers the statements the MCP
// handler runs against it. Like the row lock in Postgres, a transaction holds the lock
// from Begin until Commit or Rollback, and a statement outside a transaction takes it
// for its own duration, so a registration queued behind a sharing update waits for
// that update to finish. It answers only the statements these tests drive.
type bindingDatabase struct {
	row  *bindingRow
	lock chan struct{}
	// waiting receives a value when a statement outside a transaction starts waiting
	// for the row lock, so a test can know a write is queued without sleeping.
	waiting chan struct{}
	// beforeSharingRead runs once inside the sharing update's read, with the row lock
	// held and before the stored config is returned.
	beforeSharingRead func()
}

func newBindingDatabase() *bindingDatabase {
	return &bindingDatabase{lock: make(chan struct{}, 1), waiting: make(chan struct{}, 1)}
}

func (db *bindingDatabase) Begin(context.Context) (pgx.Tx, error) {
	db.lock <- struct{}{}
	return &bindingTx{db: db}, nil
}

func (db *bindingDatabase) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	db.acquireStatementLock()
	defer db.releaseStatementLock()
	return db.exec(query, args...)
}

func (db *bindingDatabase) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return nil, fmt.Errorf("bindingDatabase: no list statement is driven by these tests")
}

func (db *bindingDatabase) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	db.acquireStatementLock()
	defer db.releaseStatementLock()
	return db.queryRow(query, args...)
}

func (db *bindingDatabase) acquireStatementLock() {
	select {
	case db.waiting <- struct{}{}:
	default:
	}
	db.lock <- struct{}{}
}

func (db *bindingDatabase) releaseStatementLock() { <-db.lock }

// bindingTx is a transaction on bindingDatabase. It holds the row lock from Begin until
// Commit or Rollback, whichever comes first. Only the methods the handler calls are
// implemented; the embedded pgx.Tx is nil and panics on any other.
type bindingTx struct {
	pgx.Tx
	db       *bindingDatabase
	released bool
}

func (tx *bindingTx) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	return tx.db.exec(query, args...)
}

func (tx *bindingTx) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	return tx.db.queryRow(query, args...)
}

func (tx *bindingTx) Commit(context.Context) error {
	tx.end()
	return nil
}

func (tx *bindingTx) Rollback(context.Context) error {
	tx.end()
	return nil
}

func (tx *bindingTx) end() {
	if !tx.released {
		tx.released = true
		tx.db.releaseStatementLock()
	}
}

// exec applies the one UPDATE of the enable and sharing paths. A row the statement does
// not match is "UPDATE 0", as Postgres reports it.
func (db *bindingDatabase) exec(query string, args ...any) (pgconn.CommandTag, error) {
	if !strings.Contains(query, "UPDATE mcp_servers") {
		return pgconn.CommandTag{}, fmt.Errorf("bindingDatabase: unexpected exec: %.60s", query)
	}
	// args: enabled, rollout_state, config_json, updated_at, id, org_id.
	if db.row == nil || db.row.id != args[4] || db.row.orgID != args[5] {
		return pgconn.NewCommandTag("UPDATE 0"), nil
	}
	if enabled, ok := args[0].(bool); ok {
		db.row.enabled = enabled
	}
	if rollout, _ := args[1].(string); rollout != "" {
		db.row.rolloutState = rollout
	}
	if config, ok := args[2].([]byte); ok {
		db.row.config = config
	}
	db.row.updatedAt = args[3].(time.Time)
	return pgconn.NewCommandTag("UPDATE 1"), nil
}

func (db *bindingDatabase) queryRow(query string, args ...any) pgx.Row {
	switch {
	case strings.Contains(query, "WITH registered AS ("):
		return db.upsert(args)
	case strings.Contains(query, "SELECT config_json, scope, auth_kind"):
		if hook := db.beforeSharingRead; hook != nil {
			db.beforeSharingRead = nil
			hook()
		}
		if db.row == nil || db.row.id != args[0] || db.row.orgID != args[1] {
			return errorRow{err: pgx.ErrNoRows}
		}
		config, err := decodeBindingConfig(db.row.config)
		if err != nil {
			return errorRow{err: err}
		}
		return bindingFieldsRow{values: []any{config, db.row.scope, db.row.authKind}}
	case strings.Contains(query, "SELECT id, org_id, name, description, endpoint_url"):
		if db.row == nil || db.row.id != args[0] || db.row.orgID != args[1] {
			return errorRow{err: pgx.ErrNoRows}
		}
		config, err := decodeBindingConfig(db.row.config)
		if err != nil {
			return errorRow{err: err}
		}
		row := db.row
		return bindingFieldsRow{values: []any{
			row.id, row.orgID, row.name, row.description, row.endpointURL, row.transport, row.authKind,
			config, row.scope, row.enabled, row.rolloutState, row.riskLevel, row.createdAt, row.updatedAt,
		}}
	}
	return errorRow{err: fmt.Errorf("bindingDatabase: unexpected query: %.60s", query)}
}

// upsert stores a registration as the statement does: a new row, or, on the same
// (org, name), the existing row keeps its id and takes the new fields.
func (db *bindingDatabase) upsert(args []any) pgx.Row {
	id, orgID, name := args[0].(string), args[1].(string), args[2].(string)
	config, _ := args[7].([]byte)
	if db.row == nil || db.row.orgID != orgID || db.row.name != name {
		db.row = &bindingRow{id: id, orgID: orgID, name: name, createdAt: args[12].(time.Time)}
	}
	db.row.description = args[3].(string)
	db.row.endpointURL = args[4].(string)
	db.row.transport = args[5].(string)
	db.row.authKind = args[6].(string)
	db.row.config = config
	db.row.scope = args[8].(string)
	db.row.enabled = args[9].(bool)
	db.row.rolloutState = args[10].(string)
	db.row.riskLevel = args[11].(string)
	db.row.updatedAt = args[13].(time.Time)
	return stringRow{value: db.row.id}
}

func decodeBindingConfig(raw []byte) (any, error) {
	var config any
	if err := json.Unmarshal(raw, &config); err != nil {
		return nil, err
	}
	return config, nil
}

// bindingFieldsRow scans the values of one statement's row into its destinations.
type bindingFieldsRow struct{ values []any }

func (row bindingFieldsRow) Scan(dest ...any) error {
	if len(dest) != len(row.values) {
		return fmt.Errorf("bindingFieldsRow: %d destinations for %d values", len(dest), len(row.values))
	}
	for i, target := range dest {
		switch out := target.(type) {
		case *string:
			*out = row.values[i].(string)
		case *bool:
			*out = row.values[i].(bool)
		case *any:
			*out = row.values[i]
		case *time.Time:
			*out = row.values[i].(time.Time)
		default:
			return fmt.Errorf("bindingFieldsRow: destination %d is %T", i, target)
		}
	}
	return nil
}

// bindingRegistration is the body the gateway's write-through sends for a static-bearer
// server whose secret reference is the given credential binding.
func bindingRegistration(bindingID string) string {
	return `{"org_id":"org-a","name":"tenant-mcp","endpoint_url":"https://mcp.example.test","transport":"http","auth_kind":"bearer","config_json":{"tool_allowlist":["records.search"],"secret_ref":"secret://integration-corev2/credential-bindings/` + bindingID + `"}}`
}

// newBindingTestHandler mounts the MCP handler over db behind the test's signed tokens.
func newBindingTestHandler(t *testing.T, db *bindingDatabase) (http.Handler, string, string) {
	t.Helper()
	mcp := NewMCPHandler(nil).WithPublisher(nil)
	mcp.pool = db
	mcp.resolver = publicMCPResolver{}
	mux := http.NewServeMux()
	mcp.Register(mux)
	return mcpAuthenticatedHandler(t, mux)
}

// bindingServe sends one request and returns the recorder. It does not touch t, so a
// goroutine may call it.
func bindingServe(handler http.Handler, method, target, token, body string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

// bindingRegister registers a server bound to bindingID and returns the id it persisted.
func bindingRegister(t *testing.T, handler http.Handler, token, bindingID string) string {
	t.Helper()
	response := bindingServe(handler, http.MethodPost, "/api/v1/mcp", token, bindingRegistration(bindingID))
	if response.Code != http.StatusCreated {
		t.Fatalf("registration of %s: status %d, body=%s", bindingID, response.Code, response.Body.String())
	}
	var created struct {
		ID string `json:"id"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &created); err != nil || created.ID == "" {
		t.Fatalf("registration response %q: %v", response.Body.String(), err)
	}
	return created.ID
}

// bindingView reads the server and returns the auth_binding_id its view carries.
func bindingView(t *testing.T, handler http.Handler, token, id string) string {
	t.Helper()
	response := bindingServe(handler, http.MethodGet, "/api/v1/mcp/"+id, token, "")
	if response.Code != http.StatusOK {
		t.Fatalf("read of %s: status %d, body=%s", id, response.Code, response.Body.String())
	}
	var view struct {
		AuthBindingID string `json:"auth_binding_id"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &view); err != nil {
		t.Fatalf("read of %s: %v", id, err)
	}
	return view.AuthBindingID
}

// FU-14: the view names only a credential binding, and only one that the stored
// secret reference spells out. Any other reference names no binding.
func TestAuthBindingIDNamesOnlyACredentialBinding(t *testing.T) {
	t.Parallel()
	cases := map[string]string{
		"": "",
		"secret://integration-corev2/credential-bindings/cb_A":                "cb_A",
		"secret://integration-corev2/credential-bindings/cb_A.v2:rotated-1_x": "cb_A.v2:rotated-1_x",
		"secret://integration-corev2/credential-bindings/":                    "",
		"secret://integration-corev2/credential-bindings/A":                   "",
		"secret://integration-corev2/credential-bindings/cb_A/extra":          "",
		"secret://integration-corev2/credential-bindings/cb_has space":        "",
		"vault://integration-corev2/credential-bindings/cb_A":                 "",
		"secret://vault/credential":                                           "",
		"https://vault.example.test/credential":                               "",
	}
	for ref, want := range cases {
		if got := authBindingIDFromSecretRef(ref); got != want {
			t.Fatalf("authBindingIDFromSecretRef(%q) = %q, want %q", ref, got, want)
		}
	}
}

// FU-14: set binding A, then B, then re-read. The view shows B, and a sharing change
// made afterwards keeps B.
func TestRebindingShowsTheLatestCredentialBinding(t *testing.T) {
	db := newBindingDatabase()
	handler, readToken, writeToken := newBindingTestHandler(t, db)

	id := bindingRegister(t, handler, writeToken, "cb_A")
	if got := bindingView(t, handler, readToken, id); got != "cb_A" {
		t.Fatalf("after binding A the view shows %q, want cb_A", got)
	}
	if again := bindingRegister(t, handler, writeToken, "cb_B"); again != id {
		t.Fatalf("re-registering the same name persisted id %q, want the original %q", again, id)
	}
	if got := bindingView(t, handler, readToken, id); got != "cb_B" {
		t.Fatalf("after binding B the view shows %q, want cb_B", got)
	}

	shared := bindingServe(handler, http.MethodPatch, "/api/v1/mcp/"+id, writeToken, `{"shared_with":["user-b"]}`)
	if shared.Code != http.StatusOK {
		t.Fatalf("sharing update: status %d, body=%s", shared.Code, shared.Body.String())
	}
	if got := bindingView(t, handler, readToken, id); got != "cb_B" {
		t.Fatalf("after a sharing update the view shows %q, want cb_B", got)
	}
}

// FU-14 (the restore): a sharing update reads binding A, a registration of binding B is
// queued behind the update's transaction, and the update then writes its config back.
// The registration commits after the update, so the row must end on B. Without the
// row lock the update would write A over B, and the view would show the previous
// binding until the next registration.
func TestSharingUpdateNeverRestoresTheBindingAReRegistrationReplaced(t *testing.T) {
	db := newBindingDatabase()
	handler, readToken, writeToken := newBindingTestHandler(t, db)
	id := bindingRegister(t, handler, writeToken, "cb_A")

	registered := make(chan *httptest.ResponseRecorder, 1)
	db.beforeSharingRead = func() {
		// Forget any signal left by an earlier statement, so the wait below sees only
		// the registration started here.
		select {
		case <-db.waiting:
		default:
		}
		go func() {
			registered <- bindingServe(handler, http.MethodPost, "/api/v1/mcp", writeToken, bindingRegistration("cb_B"))
		}()
		// The registration reaches the row lock and waits there, behind the update.
		select {
		case <-db.waiting:
		case <-time.After(10 * time.Second):
			panic("the registration never reached the row lock")
		}
	}
	shared := bindingServe(handler, http.MethodPatch, "/api/v1/mcp/"+id, writeToken, `{"shared_with":["user-b"]}`)
	if shared.Code != http.StatusOK {
		t.Fatalf("sharing update: status %d, body=%s", shared.Code, shared.Body.String())
	}
	var response *httptest.ResponseRecorder
	select {
	case response = <-registered:
	case <-time.After(10 * time.Second):
		t.Fatal("the registration never completed after the sharing update committed")
	}
	if response.Code != http.StatusCreated {
		t.Fatalf("registration of cb_B: status %d, body=%s", response.Code, response.Body.String())
	}
	if got := bindingView(t, handler, readToken, id); got != "cb_B" {
		t.Fatalf("the row holds %q after the registration of cb_B committed, want cb_B", got)
	}
}

// A sharing update that finds no row still releases the row lock it took, so the next
// write is not blocked behind it.
func TestAFailedSharingUpdateReleasesTheRowLock(t *testing.T) {
	db := newBindingDatabase()
	handler, _, writeToken := newBindingTestHandler(t, db)

	missing := bindingServe(handler, http.MethodPatch, "/api/v1/mcp/mcp-missing", writeToken, `{"shared_with":["user-b"]}`)
	if missing.Code != http.StatusNotFound {
		t.Fatalf("sharing update of a missing server: status %d, want 404", missing.Code)
	}
	done := make(chan *httptest.ResponseRecorder, 1)
	go func() {
		done <- bindingServe(handler, http.MethodPost, "/api/v1/mcp", writeToken, bindingRegistration("cb_A"))
	}()
	select {
	case response := <-done:
		if response.Code != http.StatusCreated {
			t.Fatalf("registration after the failed update: status %d, body=%s", response.Code, response.Body.String())
		}
	case <-time.After(10 * time.Second):
		t.Fatal("a registration stayed blocked after a failed sharing update: the row lock was not released")
	}
}
