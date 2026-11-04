package cron

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/services/capability-core/internal/notifyclient"
)

// taskConfigJSON is what carries a schedule's workflow selection into the task
// the sweeper creates. taskexec's dispatchPlan reads tasks.config_json to pick
// the workflow type, so a regression here silently pins every scheduled run to
// the default workflow — which is exactly the bug this function fixed.
func TestTaskConfigJSON(t *testing.T) {
	tests := []struct {
		name     string
		template string
		// wantWorkflowType is what dispatchPlan would read back; empty means it
		// would fall through to the default workflow.
		wantWorkflowType string
		wantObject       bool
	}{
		{
			name:             "workflow type survives",
			template:         `{"kind":"cron","title":"Nightly","workflow_type":"DeepTaskWorkflow"}`,
			wantWorkflowType: "DeepTaskWorkflow",
			wantObject:       true,
		},
		{
			name:             "workflow input survives alongside the type",
			template:         `{"workflow_type":"WideResearchWorkflow","workflow_input":{"queries":["a","b"]}}`,
			wantWorkflowType: "WideResearchWorkflow",
			wantObject:       true,
		},
		{
			name:             "template without workflow fields is still a valid object",
			template:         `{"kind":"cron","title":"Plain"}`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "absent template yields an empty object, never invalid JSON",
			template:         ``,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "whitespace-only template yields an empty object",
			template:         "  \n\t ",
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "json null yields an empty object",
			template:         `null`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			// An array would make dispatchPlan's unmarshal fail and turn every
			// fire into a failed task, so it must be normalized away here.
			name:             "array template is rejected rather than passed through",
			template:         `[{"workflow_type":"DeepTaskWorkflow"}]`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "scalar template is rejected rather than passed through",
			template:         `"DeepTaskWorkflow"`,
			wantWorkflowType: "",
			wantObject:       true,
		},
		{
			name:             "malformed json is rejected rather than passed through",
			template:         `{"workflow_type":`,
			wantWorkflowType: "",
			wantObject:       true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := taskConfigJSON([]byte(tc.template), nil)

			// Mirror dispatchPlan exactly: plain Unmarshal into the dispatch
			// template. If this errors, the dispatcher would fail the task.
			var tpl struct {
				WorkflowType  string          `json:"workflow_type"`
				WorkflowInput json.RawMessage `json:"workflow_input"`
				Policy        string          `json:"policy"`
			}
			if err := json.Unmarshal(got, &tpl); err != nil {
				t.Fatalf("dispatchPlan would fail to unmarshal config_json %q: %v", got, err)
			}
			if tpl.WorkflowType != tc.wantWorkflowType {
				t.Errorf("workflow_type = %q, want %q (config_json was %q)",
					tpl.WorkflowType, tc.wantWorkflowType, got)
			}

			if tc.wantObject {
				var probe map[string]json.RawMessage
				if err := json.Unmarshal(got, &probe); err != nil {
					t.Errorf("config_json %q is not a JSON object: %v", got, err)
				}
			}
		})
	}
}

func TestSweeperFailsClosedWithoutFreshFireAuthorizer(t *testing.T) {
	_, err := NewSweeper(nil).RunOnce(context.Background())
	if err == nil || !strings.Contains(err.Error(), "authorizer") {
		t.Fatalf("unconfigured sweeper must refuse to claim schedules, got %v", err)
	}
}

// The column is NOT NULL, so the function must never return something Postgres
// would reject as a jsonb value.
func TestTaskConfigJSONIsNeverEmptyBytes(t *testing.T) {
	for _, template := range []string{``, `null`, `[]`, `"x"`, `{`, `  `} {
		if got := taskConfigJSON([]byte(template), nil); len(got) == 0 {
			t.Errorf("taskConfigJSON(%q) returned empty bytes; NOT NULL jsonb needs a value", template)
		}
	}
}

func TestTaskConfigCarriesNonSecretFireIntentButNoDecisionToken(t *testing.T) {
	intent := &FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey: "2026-08-13T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "schedule-1:2026-08-13T00:00:00Z",
	}
	var got map[string]json.RawMessage
	if err := json.Unmarshal(taskConfigJSON([]byte(`{"workflow_type":"x"}`), intent), &got); err != nil {
		t.Fatalf("decode task config: %v", err)
	}
	if _, ok := got["schedule_fire_intent"]; !ok {
		t.Fatal("fired task is missing the non-secret reauthorization intent")
	}
	if strings.Contains(string(got["schedule_fire_intent"]), "token") {
		t.Fatalf("task config must not retain a Control bearer: %s", got["schedule_fire_intent"])
	}
}

func TestMalformedTemplateCannotDropFreshFireIntent(t *testing.T) {
	intent := &FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-1",
		FireKey: "2026-08-13T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "schedule-1:2026-08-13T00:00:00Z",
	}
	var got map[string]json.RawMessage
	if err := json.Unmarshal(taskConfigJSON([]byte(`{"workflow_type":`), intent), &got); err != nil {
		t.Fatalf("decode normalized malformed template: %v", err)
	}
	if _, ok := got["schedule_fire_intent"]; !ok {
		t.Fatal("malformed template erased the required fresh-fire intent")
	}
}

func TestTemplateCannotOverrideSchedulerDerivedFireIntent(t *testing.T) {
	intent := &FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "schedule-real",
		FireKey: "2026-08-13T00:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "schedule-real:2026-08-13T00:00:00Z",
	}
	template := `{"schedule_fire_intent":{"schedule_id":"attacker"}}`
	var got struct {
		Intent FireIntent `json:"schedule_fire_intent"`
	}
	if err := json.Unmarshal(taskConfigJSON([]byte(template), intent), &got); err != nil {
		t.Fatalf("decode task config: %v", err)
	}
	if got.Intent.ScheduleID != intent.ScheduleID || got.Intent.OrgID != intent.OrgID {
		t.Fatalf("template overrode scheduler-derived fire intent: %+v", got.Intent)
	}
}

// guardBatchTx stands in for the batch transaction the guard tests run against. It
// records every statement with its arguments, answers the fire-history query with the
// statuses it was given, and is its own savepoint.
type guardBatchTx struct {
	pgx.Tx
	statuses []string
	execs    []recordedExec
}

func (tx *guardBatchTx) Begin(context.Context) (pgx.Tx, error) { return tx, nil }

func (tx *guardBatchTx) Commit(context.Context) error { return nil }

func (tx *guardBatchTx) Rollback(context.Context) error { return nil }

func (tx *guardBatchTx) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	tx.execs = append(tx.execs, recordedExec{sql: sql, args: args})
	return pgconn.CommandTag{}, nil
}

func (tx *guardBatchTx) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return &statusRows{statuses: tx.statuses}, nil
}

// statusRows yields one status column per row.
type statusRows struct {
	pgx.Rows
	statuses []string
	next     int
}

func (rows *statusRows) Next() bool {
	if rows.next >= len(rows.statuses) {
		return false
	}
	rows.next++
	return true
}

func (rows *statusRows) Scan(dest ...any) error {
	*dest[0].(*string) = rows.statuses[rows.next-1]
	return nil
}

func (rows *statusRows) Err() error { return nil }

func (rows *statusRows) Close() {}

// countingAuthorizer records each fire it is asked to authorise.
type countingAuthorizer struct{ calls int }

func (a *countingAuthorizer) AuthorizeFire(context.Context, FireIntent) error {
	a.calls++
	return nil
}

// recordingPauseNotifier keeps every notice it is asked to send.
type recordingPauseNotifier struct{ requests []notifyclient.Request }

func (n *recordingPauseNotifier) Accept(_ context.Context, req notifyclient.Request) error {
	n.requests = append(n.requests, req)
	return nil
}

// guardTemplate is the task template a contract schedule stores: the goal, and the
// approval as JSON text, the way WS20-S4 writes it.
func guardTemplate(t *testing.T, expires time.Time) json.RawMessage {
	t.Helper()
	contract := `{"expires_at":"` + expires.UTC().Format("2006-01-02T15:04:05Z") + `","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}`
	raw, err := json.Marshal(map[string]any{"kind": "cron", "description": "summarise the week", "approval_contract": contract})
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

// guardDue is a due schedule of org-1 whose slot is the given instant.
func guardDue(template json.RawMessage, slot time.Time) dueSchedule {
	return dueSchedule{
		id:             "sched-1",
		orgID:          "org-1",
		spaceRef:       "space-1",
		subjectID:      "user-1",
		templateDigest: "sha256:" + strings.Repeat("a", 64),
		expr:           "0 9 * * *",
		tz:             "UTC",
		template:       template,
		nextFire:       &slot,
	}
}

// An expired approval never fires. The sweeper pauses the schedule, records the pause as
// a paused fire, and holds the owner's notice until the batch has committed.
func TestExpiredContractPausesSchedule(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 10, 31, 9, 0, 0, 0, time.UTC)
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{authorizer: authorizer}).WithContractGuards(true)
	tx := &guardBatchTx{}
	var pauses []pauseNotice

	fired, err := sweeper.sweepDue(ctx, tx, guardDue(guardTemplate(t, now.Add(-time.Hour)), now.Add(-time.Minute)), now, &pauses)
	if err != nil || fired {
		t.Fatalf("expired schedule: fired=%v err=%v, want a clean pause", fired, err)
	}
	if authorizer.calls != 0 {
		t.Fatalf("an expired approval reached the fire authorizer %d time(s)", authorizer.calls)
	}
	if len(pauses) != 1 || pauses[0].outcome != "paused_expired" {
		t.Fatalf("pauses = %+v, want one paused_expired notice", pauses)
	}
	var disabled, recorded bool
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "SET enabled=false") {
			disabled = true
		}
		if strings.Contains(exec.sql, "INSERT INTO cron_fires") && len(exec.args) == 7 &&
			exec.args[3] == "paused" && exec.args[6] == "paused_expired" {
			recorded = true
		}
	}
	if !disabled || !recorded {
		t.Fatalf("pause did not disable the schedule and record a paused fire: %+v", tx.execs)
	}

	notifier := &recordingPauseNotifier{}
	sweeper.WithPauseNotifier(notifier)
	sweeper.sendPauseNotices(ctx, pauses)
	if len(notifier.requests) != 1 {
		t.Fatalf("sent %d notices, want 1", len(notifier.requests))
	}
	req := notifier.requests[0]
	if req.Type != "modelplane.scheduled_run_outcome" || req.OrganizationID != "org-1" || req.Recipient.ID != "user-1" {
		t.Fatalf("notice went to %+v", req)
	}
	if req.Payload["outcome"] != "paused_expired" {
		t.Fatalf("notice outcome = %v, want paused_expired", req.Payload["outcome"])
	}
	body, err := json.Marshal(req.Payload)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(body), "summarise") {
		t.Fatalf("pause notice carries the schedule's goal: %s", body)
	}
}

// Three failed fires in a row pause the schedule before a fourth fire is made. Any fire
// that did not fail breaks the run, so a renewed schedule starts its count again.
func TestThreeFailuresPause(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 10, 31, 9, 0, 0, 0, time.UTC)
	live := guardTemplate(t, now.Add(30*24*time.Hour))
	cases := []struct {
		name      string
		statuses  []string
		wantPause bool
	}{
		{name: "three failed fires in a row pause", statuses: []string{"failed", "failed", "failed"}, wantPause: true},
		{name: "a completed fire breaks the run", statuses: []string{"failed", "completed", "failed"}},
		{name: "a paused fire breaks the run", statuses: []string{"failed", "paused", "failed"}},
		{name: "an unknown fire is not a failure", statuses: []string{"failed", "failed", "unknown"}},
		{name: "two failed fires do not trip the limit", statuses: []string{"failed", "failed"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			authorizer := &countingAuthorizer{}
			sweeper := (&Sweeper{authorizer: authorizer}).WithContractGuards(true)
			tx := &guardBatchTx{statuses: tc.statuses}
			var pauses []pauseNotice
			fired, err := sweeper.sweepDue(ctx, tx, guardDue(live, now.Add(-time.Minute)), now, &pauses)
			if err != nil {
				t.Fatalf("sweepDue: %v", err)
			}
			if tc.wantPause {
				if fired || authorizer.calls != 0 || len(pauses) != 1 || pauses[0].outcome != "paused_failures" {
					t.Fatalf("want a paused_failures pause and no fire, got fired=%v calls=%d pauses=%+v", fired, authorizer.calls, pauses)
				}
				return
			}
			if !fired || authorizer.calls != 1 || len(pauses) != 0 {
				t.Fatalf("want the fire to go ahead, got fired=%v calls=%d pauses=%+v", fired, authorizer.calls, pauses)
			}
		})
	}
}

// With the guards off the sweeper reads no approval, so an expired contract reaches the
// fire authorizer and no schedule is paused, exactly as before WS20-S8.
func TestGuardsOffLeaveAnExpiredContractToTheAuthorizer(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 10, 31, 9, 0, 0, 0, time.UTC)
	authorizer := &countingAuthorizer{}
	sweeper := &Sweeper{authorizer: authorizer}
	tx := &guardBatchTx{statuses: []string{"failed", "failed", "failed"}}
	var pauses []pauseNotice

	fired, err := sweeper.sweepDue(ctx, tx, guardDue(guardTemplate(t, now.Add(-time.Hour)), now.Add(-time.Minute)), now, &pauses)
	if err != nil || !fired || authorizer.calls != 1 || len(pauses) != 0 {
		t.Fatalf("guards off: fired=%v err=%v calls=%d pauses=%+v, want the fire to go ahead", fired, err, authorizer.calls, pauses)
	}
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "enabled=false") {
			t.Fatalf("guards off still disabled a schedule: %s", exec.sql)
		}
	}
}

func TestApprovalContractLapsesWhenExpiredOrUnreadable(t *testing.T) {
	now := time.Date(2026, 10, 31, 9, 0, 0, 0, time.UTC)
	cases := []struct {
		name        string
		template    json.RawMessage
		wantPresent bool
		wantLapsed  bool
	}{
		{name: "a draft-only schedule carries no approval", template: json.RawMessage(`{"kind":"cron","title":"Nightly"}`)},
		{name: "a live approval", template: guardTemplate(t, now.Add(time.Hour)), wantPresent: true},
		{name: "an expired approval", template: guardTemplate(t, now.Add(-time.Second)), wantPresent: true, wantLapsed: true},
		{name: "an approval that is not JSON text", template: json.RawMessage(`{"approval_contract":{"v":1}}`), wantPresent: true, wantLapsed: true},
		{name: "an approval that does not parse", template: json.RawMessage(`{"approval_contract":"{\"v\":1}"}`), wantPresent: true, wantLapsed: true},
		{name: "a template that is not an object", template: json.RawMessage(`[]`)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			present, lapsed := approvalContract(tc.template, now)
			if present != tc.wantPresent || lapsed != tc.wantLapsed {
				t.Fatalf("approvalContract = (present %v, lapsed %v), want (%v, %v)", present, lapsed, tc.wantPresent, tc.wantLapsed)
			}
			if got := ApprovalContractLapsed(tc.template, now); got != tc.wantLapsed {
				t.Fatalf("ApprovalContractLapsed = %v, want %v", got, tc.wantLapsed)
			}
		})
	}
}

// A pending fire with no fire key can never be matched to a run, so once it is an hour
// old the sweep marks it unknown.
func TestLegacyPendingFiresBecomeUnknownAfterAnHour(t *testing.T) {
	now := time.Date(2026, 10, 31, 9, 0, 0, 0, time.UTC)
	tx := &guardBatchTx{}
	if err := sweepLegacyPendingFires(context.Background(), tx, now); err != nil {
		t.Fatal(err)
	}
	if len(tx.execs) != 1 {
		t.Fatalf("issued %d statements, want 1", len(tx.execs))
	}
	if sql := tx.execs[0].sql; !strings.Contains(sql, "status='pending' AND fire_key=''") || !strings.Contains(sql, "SET status='unknown'") {
		t.Fatalf("sweep does not turn legacy pending fires unknown: %s", sql)
	}
	cutoff, ok := tx.execs[0].args[1].(time.Time)
	if !ok || !cutoff.Equal(now.Add(-time.Hour)) {
		t.Fatalf("sweep cutoff = %v, want one hour before the batch", tx.execs[0].args[1])
	}
}

// A slot fire writes exactly what it wrote before InsertFire was extracted for Run now
// (EP-6-S2): a created task carrying the slot's fire intent, then a pending fire under
// the slot key, with the same arguments in the same order. Only the generated ids vary.
func TestFireOneUnchangedAfterInsertFireExtract(t *testing.T) {
	slot := time.Date(2026, 11, 4, 9, 0, 0, 0, time.UTC)
	now := slot.Add(2 * time.Second)
	slotKey := slot.Format(time.RFC3339Nano)
	template := json.RawMessage(`{"kind":"report","title":"Nightly","description":"summarise","assignee":"ops","priority":2}`)
	tx := &guardBatchTx{}
	if err := (&Sweeper{}).fireOne(context.Background(), tx, guardDue(template, slot), now); err != nil {
		t.Fatalf("fireOne: %v", err)
	}
	if len(tx.execs) != 2 {
		t.Fatalf("fireOne issued %d statements, want 2: %+v", len(tx.execs), tx.execs)
	}
	task, fire := tx.execs[0], tx.execs[1]
	if !strings.Contains(task.sql, "INSERT INTO tasks") || !strings.Contains(task.sql, "VALUES ($1,$2,$3,$4,$5,$6,'created',$7,$8,$9,$9,$9)") {
		t.Fatalf("task statement changed: %s", task.sql)
	}
	taskID, _ := task.args[0].(string)
	if !strings.HasPrefix(taskID, "task_") || len(task.args) != 9 {
		t.Fatalf("task arguments = %+v", task.args)
	}
	for i, want := range []any{"org-1", "report", "Nightly", "summarise", "ops", 2} {
		if task.args[i+1] != want {
			t.Fatalf("task argument %d = %v, want %v", i+1, task.args[i+1], want)
		}
	}
	var config struct {
		Intent FireIntent `json:"schedule_fire_intent"`
	}
	if err := json.Unmarshal(task.args[7].([]byte), &config); err != nil {
		t.Fatal(err)
	}
	wantIntent := FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "sched-1",
		FireKey: slotKey, TemplateDigest: "sha256:" + strings.Repeat("a", 64), IdempotencyKey: "sched-1:" + slotKey,
	}
	if config.Intent != wantIntent {
		t.Fatalf("task intent = %+v, want %+v", config.Intent, wantIntent)
	}
	if task.args[8] != now {
		t.Fatalf("task time = %v, want %v", task.args[8], now)
	}
	if fire.sql != `INSERT INTO cron_fires (id, schedule_id, task_id, fired_at, status, fire_key) VALUES ($1,$2,$3,$4,'pending',$5)` {
		t.Fatalf("fire statement changed: %s", fire.sql)
	}
	fireID, _ := fire.args[0].(string)
	if !strings.HasPrefix(fireID, "cronfire_") || fire.args[1] != "sched-1" || fire.args[2] != taskID ||
		fire.args[3] != now || fire.args[4] != slotKey {
		t.Fatalf("fire arguments = %+v", fire.args)
	}
}
