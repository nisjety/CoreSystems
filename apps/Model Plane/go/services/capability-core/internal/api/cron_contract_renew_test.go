package api

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

const (
	renewSchemaRenewal = "sha256:space-cron-renew-v1"
	renewSchemaCreate2 = "sha256:space-cron-create-v2"
	renewGoalText      = "summarise the week for the finance team"
)

// renewDB answers the renewal handler's four statements: the schedule load, the registry read, and
// the single renewal statement. It records the renewal statement's arguments.
type renewDB struct {
	template   []byte
	creator    string
	declared   [][5]string
	updateErr  error
	updateRows int
	loads      int
	updates    [][]any
}

func (db *renewDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, nil
}

func (db *renewDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return &renewDeclaredRows{rows: db.declared}, nil
}

func (db *renewDB) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	if strings.Contains(sql, "WITH upd AS") {
		db.updates = append(db.updates, args)
		return renewCountRow{n: db.updateRows, err: db.updateErr}
	}
	db.loads++
	return renewLoadRow{db: db}
}

type renewLoadRow struct{ db *renewDB }

func (row renewLoadRow) Scan(dest ...any) error {
	*dest[0].(*[]byte) = row.db.template
	if len(dest) == 1 { // the bare re-enable guard reads the template only
		return nil
	}
	*dest[1].(*string) = row.db.creator
	*dest[2].(*string) = "space-1"
	*dest[3].(*string) = "0 9 * * *"
	*dest[4].(*string) = "UTC"
	return nil
}

type renewCountRow struct {
	n   int
	err error
}

func (row renewCountRow) Scan(dest ...any) error {
	if row.err != nil {
		return row.err
	}
	*dest[0].(*int) = row.n
	return nil
}

type renewDeclaredRows struct {
	pgx.Rows
	rows [][5]string
	next int
}

func (rows *renewDeclaredRows) Next() bool {
	rows.next++
	return rows.next <= len(rows.rows)
}

func (rows *renewDeclaredRows) Scan(dest ...any) error {
	for i, value := range rows.rows[rows.next-1] {
		*dest[i].(*string) = value
	}
	return nil
}

func (rows *renewDeclaredRows) Err() error { return nil }
func (rows *renewDeclaredRows) Close()     {}

// renewalFixture is one schedule with an expired approval plus a verifier whose key signs the
// decisions below.
type renewalFixture struct {
	db       *renewDB
	handler  *CronHandler
	private  ed25519.PrivateKey
	template map[string]any
	contract string
	now      time.Time
}

func contractText(expires time.Time, tool string) string {
	return `{"expires_at":"` + expires.UTC().Format("2006-01-02T15:04:05Z") + `","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.` + tool + `","class":"read","tool":"` + tool + `"}],"v":1}`
}

func newRenewalFixture(t *testing.T) *renewalFixture {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := cron.NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	template := map[string]any{"kind": "cron", "description": renewGoalText, "approval_contract": contractText(now.Add(-time.Hour), "web_search")}
	stored, err := json.Marshal(template)
	if err != nil {
		t.Fatal(err)
	}
	db := &renewDB{
		template: stored, creator: "user-1", updateRows: 1,
		declared: [][5]string{{"web_search", "cap.tool.web_search", "read", "none", ""}},
	}
	handler := NewCronHandler(db).WithSpaceDecisionVerifier(verifier).WithScheduleContract(true).WithContractGuards(true)
	return &renewalFixture{
		db: db, handler: handler, private: private, template: template, now: now,
		contract: contractText(now.AddDate(0, 0, 30), "web_search"),
	}
}

// templateDigestFor is the digest of the stored template once `contract` replaces its approval.
func (f *renewalFixture) templateDigestFor(contract string) string {
	next := map[string]any{}
	for key, value := range f.template {
		next[key] = value
	}
	next["approval_contract"] = contract
	raw, _ := json.Marshal(next)
	return "sha256:" + hex.EncodeToString(sha256Sum(raw))
}

func sha256Sum(raw []byte) []byte {
	sum := sha256.Sum256(raw)
	return sum[:]
}

type renewalDecision struct {
	schema            string
	renewalOf         string
	templateDigest    string
	contract          string
	contractExpiresAt string
	idempotencyKey    string
	subject           string
	zdr               bool
}

// sign builds the wire form of a Control decision: the payload digest is computed here from the
// documented field order, independently of the verifier under test.
func (f *renewalFixture) sign(t *testing.T, d renewalDecision) string {
	t.Helper()
	scheduleID := "sched-1"
	fields := [][2]string{
		{"org_id", "org-1"}, {"user_id", d.subject}, {"space_id", "space-1"},
		{"schedule_id", scheduleID}, {"template_digest", d.templateDigest},
		{"recipient_audience_ref", "audience-1"}, {"recipient_audience_hash", "sha256:audience"},
		{"privacy_policy_ref", "privacy-1"}, {"resource_authorization_ref", "resource-1"},
		{"action_schema_hash", d.schema}, {"idempotency_key", d.idempotencyKey},
		{"contract_digest", cron.ContractDigest(d.contract)}, {"contract_expires_at", d.contractExpiresAt},
		{"contract_max_class", "read"},
	}
	if d.renewalOf != "" {
		fields = append(fields, [2]string{"renewal_of", d.renewalOf})
	}
	hash := sha256.New()
	hash.Write([]byte("model.cron.create\x00v1\x00"))
	for _, field := range fields {
		hash.Write([]byte(field[0]))
		hash.Write([]byte{0})
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(field[1])))
		hash.Write(length[:])
		hash.Write([]byte(field[1]))
	}
	for _, revision := range []struct {
		name  string
		value int64
	}{{"authority_revision", 7}, {"membership_revision", 4}, {"privacy_revision", 5}, {"recipient_audience_revision", 2}, {"entitlement_revision", 3}} {
		hash.Write([]byte(revision.name))
		hash.Write([]byte{0})
		var encoded [8]byte
		binary.BigEndian.PutUint64(encoded[:], uint64(revision.value))
		hash.Write(encoded[:])
	}
	payload, err := json.Marshal(map[string]any{
		"decision_ref": "decision-1", "org_id": "org-1", "space_ref": "space-1", "subject_id": d.subject,
		"service_audience": "model-plane-capability-core", "action_id": "model.cron.create",
		"action_schema_hash": d.schema, "payload_digest": "sha256:" + hex.EncodeToString(hash.Sum(nil)),
		"idempotency_key": d.idempotencyKey, "recipient_audience_ref": "audience-1", "recipient_audience_hash": "sha256:audience",
		"privacy_policy_ref": "privacy-1", "resource_authorization_ref": "resource-1",
		"authority_revision": 7, "membership_revision": 4, "privacy_revision": 5,
		"recipient_audience_revision": 2, "entitlement_revision": 3,
		"permissions": []string{"cron:create"}, "zero_data_retention": d.zdr,
		"issued_at": f.now.Add(-time.Second), "expires_at": f.now.Add(time.Minute),
	})
	if err != nil {
		t.Fatal(err)
	}
	signed := "v2." + base64.RawURLEncoding.EncodeToString([]byte("key-1")) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(f.private, []byte(signed)))
}

// goodDecision is a renewal decision for this fixture's contract.
func (f *renewalFixture) goodDecision() renewalDecision {
	return renewalDecision{
		schema: renewSchemaRenewal, renewalOf: "sched-1", templateDigest: f.templateDigestFor(f.contract),
		contract: f.contract, contractExpiresAt: f.now.AddDate(0, 0, 30).UTC().Format("2006-01-02T15:04:05Z"),
		idempotencyKey: "space-cron-renew:sched-1:1", subject: "user-1",
	}
}

func (f *renewalFixture) patch(t *testing.T, actor string, body map[string]any) *httptest.ResponseRecorder {
	t.Helper()
	raw, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodPatch, "/api/v1/cron/sched-1", strings.NewReader(string(raw)))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		OrganizationID: "org-1", ActorID: actor, PrincipalType: "user",
	}))
	recorder := httptest.NewRecorder()
	f.handler.patch(recorder, request, "sched-1")
	return recorder
}

func (f *renewalFixture) renewBody(token string) map[string]any {
	return map[string]any{
		"approval_contract": f.contract, "space_schedule_renew_decision": token,
		"idempotency_key": "space-cron-renew:sched-1:1", "enabled": true,
	}
}

func TestRenewalReplacesTheLapsedContractAndResumesTheSchedule(t *testing.T) {
	f := newRenewalFixture(t)
	recorder := f.patch(t, "user-1", f.renewBody(f.sign(t, f.goodDecision())))
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200: %s", recorder.Code, recorder.Body.String())
	}
	if len(f.db.updates) != 1 {
		t.Fatalf("renewal statements = %d, want exactly one", len(f.db.updates))
	}
	args := f.db.updates[0]
	// $1 new template, $2 digest, $3 enabled, $5 next_fire_at, $7 id, $9 creator, $10 old template.
	var stored map[string]any
	if err := json.Unmarshal(args[0].([]byte), &stored); err != nil {
		t.Fatal(err)
	}
	if stored["approval_contract"] != f.contract {
		t.Fatalf("the contract was not replaced: %v", stored["approval_contract"])
	}
	if stored["description"] != renewGoalText || stored["kind"] != "cron" {
		t.Fatalf("a renewal changed more than the contract: %v", stored)
	}
	if args[1] != f.templateDigestFor(f.contract) {
		t.Fatalf("template digest = %v, want the digest of the renewed template", args[1])
	}
	if enabled, ok := args[2].(*bool); !ok || enabled == nil || !*enabled {
		t.Fatalf("a renewal with enabled:true must resume the schedule, got %v", args[2])
	}
	if next, ok := args[4].(time.Time); !ok || !next.After(f.now) {
		t.Fatalf("next_fire_at was not recomputed: %v", args[4])
	}
	if args[6] != "sched-1" || args[8] != "user-1" {
		t.Fatalf("the update is not scoped to the schedule and its creator: %v", args)
	}
	if string(args[9].([]byte)) != string(f.db.template) {
		t.Fatal("the update is not guarded on the template it was computed from")
	}
}

func TestRenewalAuditRowCarriesNoContent(t *testing.T) {
	f := newRenewalFixture(t)
	if recorder := f.patch(t, "user-1", f.renewBody(f.sign(t, f.goodDecision()))); recorder.Code != http.StatusOK {
		t.Fatalf("status = %d: %s", recorder.Code, recorder.Body.String())
	}
	args := f.db.updates[0]
	if args[10] != renewalAuditID("org-1", "sched-1", "space-cron-renew:sched-1:1") {
		t.Fatalf("audit id is not the deterministic one-shot id: %v", args[10])
	}
	diff := args[11].(string)
	var keys map[string]any
	if err := json.Unmarshal([]byte(diff), &keys); err != nil {
		t.Fatal(err)
	}
	if len(keys) != 4 || keys["contract_digest"] != cron.ContractDigest(f.contract) || keys["tool_count"] != float64(1) ||
		keys["expires_at"] != f.now.AddDate(0, 0, 30).UTC().Format("2006-01-02T15:04:05Z") || keys["class"] != cron.ContractClassCeiling {
		t.Fatalf("audit diff must hold the digest, tool count, expiry and class only: %s", diff)
	}
	for _, leak := range []string{renewGoalText, "web_search", "approval_contract", "cap.tool"} {
		if strings.Contains(diff, leak) {
			t.Fatalf("audit diff leaks %q: %s", leak, diff)
		}
	}
	if cronRenewalAuditAction != "schedule.contract.renewed" || !strings.Contains(cronRenewalUpdateSQL, "'"+cronRenewalAuditAction+"'") {
		t.Fatal("the renewal statement does not write the schedule.contract.renewed audit row")
	}
	if cron.ContractAuditEntityKind != "schedule" || !strings.Contains(cronRenewalUpdateSQL, "'"+cron.ContractAuditEntityKind+"'") ||
		strings.Contains(cronRenewalUpdateSQL, "'cron_schedule'") {
		t.Fatal("the renewal row must carry the entity kind schedule, not cron_schedule")
	}
}

func TestRenewalRefusals(t *testing.T) {
	cases := []struct {
		name       string
		actor      string
		mutate     func(f *renewalFixture, body map[string]any, d *renewalDecision)
		setup      func(f *renewalFixture)
		wantStatus int
	}{
		{name: "an org admin cannot renew another person's approval", actor: "admin-1", wantStatus: http.StatusForbidden},
		{
			name: "a create decision replayed as a renewal is refused", actor: "user-1", wantStatus: http.StatusForbidden,
			mutate: func(f *renewalFixture, _ map[string]any, d *renewalDecision) {
				d.schema = renewSchemaCreate2
				d.renewalOf = ""
			},
		},
		{
			name: "a decision signed over a different contract does not bind this one", actor: "user-1", wantStatus: http.StatusBadRequest,
			mutate: func(f *renewalFixture, _ map[string]any, d *renewalDecision) {
				d.contract = strings.Replace(f.contract, `"max_rounds":6`, `"max_rounds":5`, 1)
			},
		},
		{
			name: "a decision for another schedule is refused", actor: "user-1", wantStatus: http.StatusBadRequest,
			mutate: func(_ *renewalFixture, _ map[string]any, d *renewalDecision) { d.renewalOf = "sched-2" },
		},
		{
			name: "a zero data retention decision is refused", actor: "user-1", wantStatus: http.StatusForbidden,
			mutate: func(_ *renewalFixture, _ map[string]any, d *renewalDecision) { d.zdr = true },
		},
		{
			name: "a decision for another subject is refused", actor: "user-1", wantStatus: http.StatusForbidden,
			mutate: func(_ *renewalFixture, _ map[string]any, d *renewalDecision) { d.subject = "user-2" },
		},
		{
			name: "a tool that is no longer read-class is refused before the decision", actor: "user-1", wantStatus: http.StatusBadRequest,
			setup: func(f *renewalFixture) {
				f.db.declared = [][5]string{{"web_search", "cap.tool.web_search", "external_write", "none", ""}}
			},
		},
		{
			name: "a contract that has already expired is refused", actor: "user-1", wantStatus: http.StatusBadRequest,
			mutate: func(f *renewalFixture, body map[string]any, d *renewalDecision) {
				f.contract = contractText(f.now.Add(-time.Minute), "web_search")
				body["approval_contract"] = f.contract
				*d = f.goodDecision()
			},
		},
		{
			name: "a contract beyond the 90 day maximum is refused", actor: "user-1", wantStatus: http.StatusBadRequest,
			mutate: func(f *renewalFixture, body map[string]any, d *renewalDecision) {
				f.contract = contractText(f.now.AddDate(0, 0, 120), "web_search")
				body["approval_contract"] = f.contract
				*d = f.goodDecision()
			},
		},
		{
			name: "a schedule with no approval has nothing to renew", actor: "user-1", wantStatus: http.StatusConflict,
			setup: func(f *renewalFixture) { f.db.template = []byte(`{"kind":"cron","description":"draft"}`) },
		},
		{
			name: "a decision applied twice is refused", actor: "user-1", wantStatus: http.StatusConflict,
			setup: func(f *renewalFixture) { f.db.updateErr = &pgconn.PgError{Code: "23505"} },
		},
		{
			name: "a schedule changed under the renewal is refused", actor: "user-1", wantStatus: http.StatusConflict,
			setup: func(f *renewalFixture) { f.db.updateRows = 0 },
		},
		{
			name: "a renewal without a decision is refused", actor: "user-1", wantStatus: http.StatusBadRequest,
			mutate: func(_ *renewalFixture, body map[string]any, _ *renewalDecision) {
				delete(body, "space_schedule_renew_decision")
			},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := newRenewalFixture(t)
			if tc.setup != nil {
				tc.setup(f)
			}
			decision := f.goodDecision()
			body := f.renewBody("placeholder")
			if tc.mutate != nil {
				tc.mutate(f, body, &decision)
			}
			if _, set := body["space_schedule_renew_decision"]; set {
				body["space_schedule_renew_decision"] = f.sign(t, decision)
			}
			recorder := f.patch(t, tc.actor, body)
			if recorder.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d: %s", recorder.Code, tc.wantStatus, recorder.Body.String())
			}
			wrote := len(f.db.updates) > 0
			wantWrite := tc.wantStatus == http.StatusConflict && (tc.name == "a decision applied twice is refused" || tc.name == "a schedule changed under the renewal is refused")
			if wrote != wantWrite {
				t.Fatalf("renewal statement issued = %v, want %v", wrote, wantWrite)
			}
		})
	}
}

// With either switch off, a PATCH that carries the renewal fields is refused as not_supported, so the
// browser can fall back to its create path. Nothing is read, no decision is checked and nothing is written.
func TestRenewalFieldsAreRefusedWhileTheSwitchesAreOff(t *testing.T) {
	for name, configure := range map[string]func(*CronHandler){
		"contracts off": func(h *CronHandler) { h.WithScheduleContract(false) },
		"guards off":    func(h *CronHandler) { h.WithContractGuards(false) },
	} {
		t.Run(name, func(t *testing.T) {
			f := newRenewalFixture(t)
			configure(f.handler)
			recorder := f.patch(t, "user-1", map[string]any{
				"approval_contract": f.contract, "space_schedule_renew_decision": "ignored", "idempotency_key": "k",
			})
			if recorder.Code != http.StatusNotImplemented || errorCode(t, recorder) != "not_supported" {
				t.Fatalf("a renewal body must be refused as not_supported while off: %d %s", recorder.Code, recorder.Body.String())
			}
			if f.db.loads != 0 || len(f.db.updates) != 0 {
				t.Fatalf("the renewal path ran with its switches off (loads %d, updates %d)", f.db.loads, len(f.db.updates))
			}
		})
	}
}

// An enable that rides along with a contract is refused the same way while renewal is off: it never
// resumes a lapsed schedule on the strength of a contract that was not renewed.
func TestContractWithEnableIsRefusedWhileRenewalIsOff(t *testing.T) {
	f := newRenewalFixture(t)
	f.handler.WithContractGuards(false)
	recorder := f.patch(t, "user-1", map[string]any{"approval_contract": f.contract, "enabled": true})
	if recorder.Code != http.StatusNotImplemented || errorCode(t, recorder) != "not_supported" {
		t.Fatalf("status=%d body=%s, want 501 not_supported", recorder.Code, recorder.Body.String())
	}
	if f.db.loads != 0 || len(f.db.updates) != 0 {
		t.Fatalf("the refused PATCH read or wrote (loads %d, updates %d)", f.db.loads, len(f.db.updates))
	}
}

// A bare re-enable of a lapsed approval is still refused once renewal exists.
func TestBareEnableStillRefusedWhileRenewalIsOn(t *testing.T) {
	f := newRenewalFixture(t)
	recorder := f.patch(t, "user-1", map[string]any{"enabled": true})
	if recorder.Code != http.StatusConflict {
		t.Fatalf("a bare re-enable of a lapsed approval answered %d, want 409: %s", recorder.Code, recorder.Body.String())
	}
	if len(f.db.updates) != 0 {
		t.Fatal("a refused re-enable still wrote")
	}
}
