package api

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// auditCountRow answers the readiness probe with one count.
type auditCountRow struct{ n int }

func (row auditCountRow) Scan(dest ...any) error {
	*dest[0].(*int) = row.n
	return nil
}

// auditSchemaProbe stands in for the database the readiness probe runs on.
type auditSchemaProbe struct {
	count  int
	probes int
}

func (p *auditSchemaProbe) QueryRow(context.Context, string, ...any) pgx.Row {
	p.probes++
	return auditCountRow{n: p.count}
}

// auditBoolRow answers the schedule-exists read of the fire list.
type auditBoolRow struct{ found bool }

func (row auditBoolRow) Scan(dest ...any) error {
	*dest[0].(*bool) = row.found
	return nil
}

// auditFiresDB is the database a fire list reads. Its schedule exists, and the fires query is counted.
type auditFiresDB struct {
	fireQueries int
}

func (db *auditFiresDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, nil
}

func (db *auditFiresDB) Query(_ context.Context, _ string, _ ...any) (pgx.Rows, error) {
	db.fireQueries++
	return nil, errors.New("fires query reached in this test")
}

func (db *auditFiresDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return auditBoolRow{found: true}
}

// auditEmptyRows is a result with no rows.
type auditEmptyRows struct{ pgx.Rows }

func (*auditEmptyRows) Next() bool { return false }
func (*auditEmptyRows) Err() error { return nil }
func (*auditEmptyRows) Close()     {}

// auditListDB records the statement each schedule list is read with.
type auditListDB struct {
	sqls []string
}

func (db *auditListDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, nil
}

func (db *auditListDB) Query(_ context.Context, sql string, _ ...any) (pgx.Rows, error) {
	db.sqls = append(db.sqls, sql)
	return &auditEmptyRows{}, nil
}

func (db *auditListDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return auditCountRow{}
}

// auditExec is one statement a create or a refusal wrote, with its arguments.
type auditExec struct {
	sql  string
	args []any
}

// auditCreateDB records every write of a create, and answers the declared-tool read from declared.
type auditCreateDB struct {
	declared [][5]string
	queryErr error
	execErr  error
	execs    []auditExec
}

func (db *auditCreateDB) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	db.execs = append(db.execs, auditExec{sql: sql, args: args})
	return pgconn.CommandTag{}, db.execErr
}

func (db *auditCreateDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	if db.queryErr != nil {
		return nil, db.queryErr
	}
	return &renewDeclaredRows{rows: db.declared}, nil
}

func (db *auditCreateDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return auditCountRow{}
}

// auditRecorder keeps each contract audit row a handler asks it to write.
type auditRecorder struct {
	rows []cron.ContractAuditRow
}

func (a *auditRecorder) Append(_ context.Context, _ cron.AuditExecer, row cron.ContractAuditRow) error {
	a.rows = append(a.rows, row)
	return nil
}

// auditDiffOf decodes a row's diff and checks that it names only the closed fields.
func auditDiffOf(t *testing.T, raw []byte) map[string]any {
	t.Helper()
	var diff map[string]any
	if err := json.Unmarshal(raw, &diff); err != nil {
		t.Fatalf("diff %s is not a JSON object: %v", raw, err)
	}
	allowed := map[string]bool{"contract_digest": true, "tool_count": true, "expires_at": true, "class": true, "reason": true}
	for key := range diff {
		if !allowed[key] {
			t.Fatalf("diff names %q, which is not a closed field: %s", key, raw)
		}
	}
	return diff
}

// A schedule's approval contract may name only its own tools, so the leak checks use the tool name
// and the goal the contract and the template carry.
func assertNoContractContent(t *testing.T, raw string) {
	t.Helper()
	for _, leak := range []string{"web_search", "cap.tool", "approved_at", "summarise", "approval_contract"} {
		if strings.Contains(raw, leak) {
			t.Fatalf("audit content leaks %q: %s", leak, raw)
		}
	}
}

// auditCreateRequest is a POST of one schedule by the given creator, in org-1.
func auditCreateRequest(t *testing.T, body map[string]any) *http.Request {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodPost, "/api/v1/cron", strings.NewReader(string(raw)))
	return request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user",
	}))
}

// createFixture is a create that a Control decision signed for one contract and one template.
type createFixture struct {
	f     *renewalFixture
	token string
	body  map[string]any
}

func newCreateFixture(t *testing.T) createFixture {
	t.Helper()
	f := newRenewalFixture(t)
	text := contractText(f.now.AddDate(0, 0, 30), "web_search")
	template := map[string]any{"kind": "cron", "description": renewGoalText, "approval_contract": text}
	raw, err := json.Marshal(template)
	if err != nil {
		t.Fatal(err)
	}
	digest := "sha256:" + hex.EncodeToString(sha256Sum(raw)[:])
	const key = "space-cron-create:sched-1:1"
	token := f.sign(t, renewalDecision{
		schema: renewSchemaCreate2, templateDigest: digest, contract: text,
		contractExpiresAt: f.now.AddDate(0, 0, 30).UTC().Format("2006-01-02T15:04:05Z"),
		idempotencyKey:    key, subject: "user-1",
	})
	return createFixture{f: f, token: token, body: map[string]any{
		"id": "sched-1", "name": "weekly", "schedule_expr": "0 9 * * 1", "timezone": "UTC",
		"task_template": template, "enabled": true, "space_ref": "space-1", "creator_subject_id": "user-1",
		"space_schedule_create_decision": token, "idempotency_key": key,
	}}
}

// handlerFor builds the cron handler that verifies the fixture's decisions, over db.
func (c createFixture) handlerFor(t *testing.T, db registryDatabase) *CronHandler {
	t.Helper()
	public := c.f.private.Public().(ed25519.PublicKey)
	verifier, err := cron.NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	return NewCronHandler(db).WithSpaceDecisionVerifier(verifier).WithScheduleContract(true).WithContractGuards(true)
}

// A create with a contract writes its schedule and its created row in one statement. The row names the
// digest, the count, the expiry and the class, and nothing about the tools or the goal.
func TestCreateWithContractWritesItsCreatedRowInTheSameStatement(t *testing.T) {
	fixture := newCreateFixture(t)
	db := &auditCreateDB{declared: [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}}}
	handler := fixture.handlerFor(t, db)
	recorder := httptest.NewRecorder()
	handler.listOrCreate(recorder, auditCreateRequest(t, fixture.body))
	if recorder.Code != http.StatusCreated {
		t.Fatalf("status = %d, want 201: %s", recorder.Code, recorder.Body.String())
	}
	if len(db.execs) != 1 || db.execs[0].sql != cronScheduleCreateAuditedSQL {
		t.Fatalf("writes = %d, want the one audited create statement", len(db.execs))
	}
	args := db.execs[0].args
	if len(args) != 27 {
		t.Fatalf("arguments = %d, want the 23 schedule values and the 4 audit values", len(args))
	}
	if id, _ := args[23].(string); !strings.HasPrefix(id, "ral_") {
		t.Fatalf("audit row id = %v, want a fresh ral_ id", args[23])
	}
	if args[24] != "user-1" || args[25] != "org-1" {
		t.Fatalf("audit actor and org = %v, %v; want user-1 and org-1", args[24], args[25])
	}
	diff := auditDiffOf(t, []byte(args[26].(string)))
	if want := []string{"class", "contract_digest", "expires_at", "tool_count"}; !reflect.DeepEqual(sortedKeysOf(diff), want) {
		t.Fatalf("created diff keys = %v, want %v", sortedKeysOf(diff), want)
	}
	if diff["class"] != "read" || diff["tool_count"] != float64(1) {
		t.Fatalf("created diff = %v, want one read tool", diff)
	}
	assertNoContractContent(t, args[26].(string))
	if !strings.Contains(cronScheduleCreateAuditedSQL, "'"+cron.ContractAuditCreated+"'") {
		t.Fatal("the audited statement does not write the created action")
	}
}

func sortedKeysOf(diff map[string]any) []string {
	keys := make([]string, 0, len(diff))
	for key := range diff {
		keys = append(keys, key)
	}
	for i := range keys {
		for j := i + 1; j < len(keys); j++ {
			if keys[j] < keys[i] {
				keys[i], keys[j] = keys[j], keys[i]
			}
		}
	}
	return keys
}

// A created row that cannot be written fails the statement, which rolls the schedule back too. The caller
// gets a typed 503 and nothing is created.
func TestCreateWithContractRollsBackWhenItsAuditRowFails(t *testing.T) {
	fixture := newCreateFixture(t)
	db := &auditCreateDB{
		declared: [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}},
		execErr:  errors.New("relation registry_audit_log does not exist"),
	}
	handler := fixture.handlerFor(t, db)
	recorder := httptest.NewRecorder()
	handler.listOrCreate(recorder, auditCreateRequest(t, fixture.body))
	if recorder.Code != http.StatusServiceUnavailable || errorCode(t, recorder) != "audit_unavailable" {
		t.Fatalf("status = %d code = %s, want 503 audit_unavailable: %s", recorder.Code, errorCode(t, recorder), recorder.Body.String())
	}
	if len(db.execs) != 1 {
		t.Fatalf("writes = %d, want the single statement that failed", len(db.execs))
	}
}

// A schedule id that already exists is a conflict, not an audit failure.
func TestCreateWithContractAnswersADuplicateIdAsAConflict(t *testing.T) {
	fixture := newCreateFixture(t)
	db := &auditCreateDB{
		declared: [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}},
		execErr:  &pgconn.PgError{Code: "23505"},
	}
	recorder := httptest.NewRecorder()
	fixture.handlerFor(t, db).listOrCreate(recorder, auditCreateRequest(t, fixture.body))
	if recorder.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409: %s", recorder.Code, recorder.Body.String())
	}
}

// A refused contract is refused as it always was, and its refusal row names the closed token only.
func TestCreateRefusedContractWritesARefusedRowWithItsToken(t *testing.T) {
	fixture := newCreateFixture(t)
	expired := contractText(fixture.f.now.Add(-time.Hour), "web_search")
	fixture.body["task_template"] = map[string]any{"kind": "cron", "description": renewGoalText, "approval_contract": expired}
	db := &auditCreateDB{declared: [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}}}
	handler := fixture.handlerFor(t, db)
	recorder := &auditRecorder{}
	handler.WithContractAuditor(recorder)
	response := httptest.NewRecorder()
	handler.listOrCreate(response, auditCreateRequest(t, fixture.body))
	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403 as before: %s", response.Code, response.Body.String())
	}
	if len(db.execs) != 0 {
		t.Fatalf("a refused contract wrote %d statement(s)", len(db.execs))
	}
	if len(recorder.rows) != 1 {
		t.Fatalf("refusal rows = %d, want one", len(recorder.rows))
	}
	row := recorder.rows[0]
	if row.Action != cron.ContractAuditRefused || row.EntityID != "sched-1" || row.Actor != "user-1" || row.OrgID != "org-1" {
		t.Fatalf("refusal row = %+v", row)
	}
	diff := auditDiffOf(t, row.Diff)
	if diff["reason"] != cron.ContractTokenExpired {
		t.Fatalf("refusal reason = %v, want contract_expired", diff["reason"])
	}
	assertNoContractContent(t, string(row.Diff))
}

// Each way a contract can be refused maps to its closed token, and nothing else is ever a token.
func TestContractRefusalTokensFromCreateIntent(t *testing.T) {
	now := time.Now().UTC()
	declared := [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}}
	cases := []struct {
		name     string
		handler  func() *CronHandler
		template any
		want     string
	}{
		{
			name:     "contracts switched off",
			handler:  func() *CronHandler { return NewCronHandler(nil) },
			template: contractTemplate(contractText(now.AddDate(0, 0, 30), "web_search")),
			want:     cron.ContractTokenContractsDisabled,
		},
		{
			name:     "a contract that is not text",
			handler:  func() *CronHandler { return NewCronHandler(nil).WithScheduleContract(true) },
			template: map[string]any{"approval_contract": map[string]any{"v": 1}},
			want:     cron.ContractTokenInvalid,
		},
		{
			name:     "a contract that does not parse",
			handler:  func() *CronHandler { return NewCronHandler(nil).WithScheduleContract(true) },
			template: contractTemplate(strings.Replace(contractText(now.AddDate(0, 0, 30), "web_search"), `"max_rounds":6`, `"max_rounds":99`, 1)),
			want:     cron.ContractTokenInvalid,
		},
		{
			name: "an expired contract",
			handler: func() *CronHandler {
				return NewCronHandler(&auditCreateDB{declared: declared}).WithScheduleContract(true)
			},
			template: contractTemplate(contractText(now.Add(-time.Hour), "web_search")),
			want:     cron.ContractTokenExpired,
		},
		{
			name: "a window past the maximum",
			handler: func() *CronHandler {
				return NewCronHandler(&auditCreateDB{declared: declared}).WithScheduleContract(true)
			},
			template: contractTemplate(contractText(now.AddDate(0, 0, 120), "web_search")),
			want:     cron.ContractTokenWindowInvalid,
		},
		{
			name: "a tool the registry does not declare",
			handler: func() *CronHandler {
				return NewCronHandler(&auditCreateDB{}).WithScheduleContract(true)
			},
			template: contractTemplate(contractText(now.AddDate(0, 0, 30), "web_search")),
			want:     cron.ContractTokenRegistryMismatch,
		},
		{
			name: "a registry read that fails",
			handler: func() *CronHandler {
				return NewCronHandler(&auditCreateDB{queryErr: errors.New("reset by peer")}).WithScheduleContract(true)
			},
			template: contractTemplate(contractText(now.AddDate(0, 0, 30), "web_search")),
			want:     cron.ContractTokenInvalid,
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, err := tc.handler().createIntentFor(context.Background(), "org-1", tc.template, cron.CreateIntent{})
			if err == nil {
				t.Fatal("the contract was accepted")
			}
			if got := cron.ContractRefusalToken(err); got != tc.want {
				t.Fatalf("token = %q, want %q (error: %v)", got, tc.want, err)
			}
		})
	}
	// A contract the registry declares and the window allows is not refused at all.
	handler := NewCronHandler(&auditCreateDB{declared: declared}).WithScheduleContract(true)
	if _, err := handler.createIntentFor(context.Background(), "org-1", contractTemplate(contractText(now.AddDate(0, 0, 30), "web_search")), cron.CreateIntent{}); err != nil {
		t.Fatalf("a valid contract was refused: %v", err)
	}
}

// A renewal refused for its contract writes the refused row of the schedule, and the renewal writes nothing.
func TestRenewalRefusedContractWritesARefusedRow(t *testing.T) {
	f := newRenewalFixture(t)
	recorder := &auditRecorder{}
	f.handler.WithContractAuditor(recorder)
	expired := contractText(f.now.Add(-time.Minute), "web_search")
	response := f.patch(t, "user-1", map[string]any{
		"approval_contract": expired, "space_schedule_renew_decision": "placeholder", "idempotency_key": "space-cron-renew:sched-1:2",
	})
	if response.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 as before: %s", response.Code, response.Body.String())
	}
	if len(f.db.updates) != 0 {
		t.Fatal("a refused renewal wrote")
	}
	if len(recorder.rows) != 1 || recorder.rows[0].Action != cron.ContractAuditRefused || recorder.rows[0].EntityID != "sched-1" {
		t.Fatalf("renewal refusal rows = %+v, want one refused row for sched-1", recorder.rows)
	}
	if diff := auditDiffOf(t, recorder.rows[0].Diff); diff["reason"] != cron.ContractTokenExpired {
		t.Fatalf("renewal refusal reason = %v", diff["reason"])
	}
	assertNoContractContent(t, string(recorder.rows[0].Diff))
}

// failingAuditor counts the rows it was asked for and writes none.
type failingAuditor struct {
	calls int
}

func (a *failingAuditor) Append(context.Context, cron.AuditExecer, cron.ContractAuditRow) error {
	a.calls++
	return errors.New("relation registry_audit_log does not exist")
}

// A refused create whose refused row cannot be written is answered 503 audit_unavailable, not its 403,
// so no refusal is answered without its row. Nothing is written.
func TestCreateRefusedContractAnswers503WhenItsRefusedRowFails(t *testing.T) {
	fixture := newCreateFixture(t)
	expired := contractText(fixture.f.now.Add(-time.Hour), "web_search")
	fixture.body["task_template"] = map[string]any{"kind": "cron", "description": renewGoalText, "approval_contract": expired}
	db := &auditCreateDB{declared: [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}}}
	handler := fixture.handlerFor(t, db)
	auditor := &failingAuditor{}
	handler.WithContractAuditor(auditor)
	response := httptest.NewRecorder()
	handler.listOrCreate(response, auditCreateRequest(t, fixture.body))
	if response.Code != http.StatusServiceUnavailable || errorCode(t, response) != "audit_unavailable" {
		t.Fatalf("status = %d code = %s, want 503 audit_unavailable: %s", response.Code, errorCode(t, response), response.Body.String())
	}
	if auditor.calls != 1 {
		t.Fatalf("refused row attempts = %d, want one", auditor.calls)
	}
	if len(db.execs) != 0 {
		t.Fatalf("a refused create wrote %d statement(s)", len(db.execs))
	}
	assertNoContractContent(t, response.Body.String())
}

// A refused renewal whose refused row cannot be written is answered 503 audit_unavailable, not its 400,
// and the renewal writes nothing.
func TestRenewalRefusedContractAnswers503WhenItsRefusedRowFails(t *testing.T) {
	f := newRenewalFixture(t)
	auditor := &failingAuditor{}
	f.handler.WithContractAuditor(auditor)
	expired := contractText(f.now.Add(-time.Minute), "web_search")
	response := f.patch(t, "user-1", map[string]any{
		"approval_contract": expired, "space_schedule_renew_decision": "placeholder", "idempotency_key": "space-cron-renew:sched-1:2",
	})
	if response.Code != http.StatusServiceUnavailable || errorCode(t, response) != "audit_unavailable" {
		t.Fatalf("status = %d code = %s, want 503 audit_unavailable: %s", response.Code, errorCode(t, response), response.Body.String())
	}
	if auditor.calls != 1 {
		t.Fatalf("refused row attempts = %d, want one", auditor.calls)
	}
	if len(f.db.updates) != 0 {
		t.Fatal("a refused renewal wrote an UPDATE")
	}
	assertNoContractContent(t, response.Body.String())
}

// A renewal whose audit row cannot be written is one failed statement, so the renewal is rolled back with
// its row. The caller gets a typed 503, and the contract is not replaced.
func TestRenewalRollsBackWhenItsAuditRowFails(t *testing.T) {
	f := newRenewalFixture(t)
	f.db.updateErr = errors.New("relation registry_audit_log does not exist")
	recorder := f.patch(t, "user-1", f.renewBody(f.sign(t, f.goodDecision())))
	if recorder.Code != http.StatusServiceUnavailable || errorCode(t, recorder) != "audit_unavailable" {
		t.Fatalf("status = %d code = %s, want 503 audit_unavailable: %s", recorder.Code, errorCode(t, recorder), recorder.Body.String())
	}
	if len(f.db.updates) != 1 {
		t.Fatalf("renewal statements = %d, want the one statement that failed", len(f.db.updates))
	}
}

// Run now answers 503 while the fire schema is pending, and no fire reaches Control or the database.
func TestRunNowAnswers503WhileTheFireSchemaIsPending(t *testing.T) {
	authorizer := &allowingFireAuthorizer{}
	probe := &auditSchemaProbe{count: 4}
	mux := http.NewServeMux()
	NewCronHandler(unreachableDatabase{t}).
		WithRunNow(runNowStubDB{creator: "user-1"}, authorizer, true).
		WithFireSchemaGate(cron.NewFireSchemaGate(probe)).
		Register(mux)
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, runNowRequest("/api/v1/cron/sched-1/run", `{"request_id":"`+runNowTestRequestID+`"}`, "user-1"))
	if recorder.Code != http.StatusServiceUnavailable || errorCode(t, recorder) != "cron_fires_schema_pending" {
		t.Fatalf("status = %d code = %s, want 503 cron_fires_schema_pending: %s", recorder.Code, errorCode(t, recorder), recorder.Body.String())
	}
	if authorizer.calls != 0 {
		t.Fatalf("a pending run reached Control %d time(s)", authorizer.calls)
	}
}

// Once the schema is in place the same press makes its fire.
func TestRunNowProceedsOnceTheFireSchemaIsReady(t *testing.T) {
	authorizer := &allowingFireAuthorizer{}
	mux := http.NewServeMux()
	NewCronHandler(unreachableDatabase{t}).
		WithRunNow(runNowStubDB{creator: "user-1"}, authorizer, true).
		WithFireSchemaGate(cron.NewFireSchemaGate(&auditSchemaProbe{count: 5})).
		Register(mux)
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, runNowRequest("/api/v1/cron/sched-1/run", `{"request_id":"`+runNowTestRequestID+`"}`, "user-1"))
	if recorder.Code != http.StatusAccepted || authorizer.calls != 1 {
		t.Fatalf("status = %d, Control calls = %d; want 202 and one call: %s", recorder.Code, authorizer.calls, recorder.Body.String())
	}
}

// The fire list answers 503 while the fire schema is pending, and reads no fires.
func TestFireListAnswers503WhileTheFireSchemaIsPending(t *testing.T) {
	db := &auditFiresDB{}
	mux := http.NewServeMux()
	NewCronHandler(db).WithFireSchemaGate(cron.NewFireSchemaGate(&auditSchemaProbe{count: 0})).Register(mux)
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, listRequest("/api/v1/cron/sched-1/fires"))
	if recorder.Code != http.StatusServiceUnavailable || errorCode(t, recorder) != "cron_fires_schema_pending" {
		t.Fatalf("status = %d code = %s, want 503 cron_fires_schema_pending", recorder.Code, errorCode(t, recorder))
	}
	if db.fireQueries != 0 {
		t.Fatalf("a pending fire list read %d fire query(ies)", db.fireQueries)
	}
}

// The schedule list reads each schedule with its newest fire once the schema is in place, and without the
// fire join while it is pending, so the list still answers.
func TestScheduleListDropsTheFireJoinWhilePending(t *testing.T) {
	pending := &auditListDB{}
	NewCronHandler(pending).WithFireSchemaGate(cron.NewFireSchemaGate(&auditSchemaProbe{count: 4})).
		listOrCreate(httptest.NewRecorder(), listRequest("/api/v1/cron"))
	if len(pending.sqls) != 1 || pending.sqls[0] != cronScheduleListNoFiresSQL || strings.Contains(pending.sqls[0], "cron_fires") {
		t.Fatalf("pending list read = %q, want the list without the fire join", pending.sqls)
	}

	ready := &auditListDB{}
	NewCronHandler(ready).WithFireSchemaGate(cron.NewFireSchemaGate(&auditSchemaProbe{count: 5})).
		listOrCreate(httptest.NewRecorder(), listRequest("/api/v1/cron"))
	if len(ready.sqls) != 1 || ready.sqls[0] != cronScheduleListSQL || !strings.Contains(ready.sqls[0], "LEFT JOIN LATERAL") {
		t.Fatalf("ready list read did not keep the fire join")
	}
}

// The gate is one shared object, so a pending probe is made once per request and a positive answer is kept.
func TestFireSchemaGateIsSharedByTheHandlerAndCachesPositive(t *testing.T) {
	probe := &auditSchemaProbe{count: 5}
	gate := cron.NewFireSchemaGate(probe)
	h := NewCronHandler(&auditListDB{}).WithFireSchemaGate(gate)
	for i := 0; i < 3; i++ {
		h.listOrCreate(httptest.NewRecorder(), listRequest("/api/v1/cron"))
	}
	if probe.probes != 1 {
		t.Fatalf("probes = %d, want one: the migrated schema is cached", probe.probes)
	}
}
