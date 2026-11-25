package taskexec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/model-plane/services/capability-core/internal/cron"
)

// auditFireDB records each statement a refusal row is written through, with its arguments.
type auditFireDB struct {
	calls [][]any
	n     int
}

func (db *auditFireDB) Exec(_ context.Context, _ string, args ...any) (pgconn.CommandTag, error) {
	db.calls = append(db.calls, args)
	db.n++
	return pgconn.CommandTag{}, nil
}

// auditFireTemplate is the canonical template of a cron task whose stored contract ends at expires.
func auditFireTemplate(t *testing.T, expires time.Time) []byte {
	t.Helper()
	contract := `{"expires_at":"` + expires.UTC().Format("2006-01-02T15:04:05Z") + `","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}`
	raw, err := json.Marshal(map[string]any{"kind": "cron", "description": "summarise the week", "approval_contract": contract})
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func auditFireIntent() cron.FireIntent {
	return cron.FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "sched-1",
		FireKey: "2026-11-23T09:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "sched-1:2026-11-23T09:00:00Z",
	}
}

// auditRecordingAuditor keeps each row it is asked to write.
type auditRecordingAuditor struct {
	rows []cron.ContractAuditRow
}

func (a *auditRecordingAuditor) Append(_ context.Context, _ cron.AuditExecer, row cron.ContractAuditRow) error {
	a.rows = append(a.rows, row)
	return nil
}

// auditExecAuditor writes each row through the database it is given, as the registry auditor does.
type auditExecAuditor struct{}

func (auditExecAuditor) Append(ctx context.Context, db cron.AuditExecer, row cron.ContractAuditRow) error {
	_, err := db.Exec(ctx, "INSERT INTO registry_audit_log", row.ID, row.Action, row.EntityID, row.OrgID, string(row.Diff), row.Actor)
	return err
}

// auditFlakyAuditor refuses its first failures appends, then writes every row it is given.
type auditFlakyAuditor struct {
	failures int
	written  []cron.ContractAuditRow
}

func (a *auditFlakyAuditor) Append(_ context.Context, _ cron.AuditExecer, row cron.ContractAuditRow) error {
	if a.failures > 0 {
		a.failures--
		return errors.New("registry_audit_log is unavailable")
	}
	a.written = append(a.written, row)
	return nil
}

// A fire the stored contract refuses writes one refused row for its task, through the database the
// dispatcher holds. The row names the schedule, the actor, the closed token and nothing else.
func TestRefusedFireWritesOneContentFreeRefusedRow(t *testing.T) {
	now := time.Now().UTC()
	template := auditFireTemplate(t, now.Add(-time.Hour))
	refusal := refuseExpiredContract(template, now)
	if refusal == nil {
		t.Fatal("an expired contract must refuse the fire")
	}
	d := &WorkflowDispatcher{}
	d.SetContractAuditor(auditExecAuditor{})
	db := &auditFireDB{}
	d.auditFireRefusal(context.Background(), db, "task_1", auditFireIntent(), template, refusal)
	if db.n != 1 {
		t.Fatalf("refusal rows = %d, want exactly one", db.n)
	}
	args := db.calls[0]
	if args[0] != cron.RefusedFireAuditID("task_1") || args[1] != cron.ContractAuditRefused ||
		args[2] != "sched-1" || args[3] != "org-1" || args[5] != dispatchActor {
		t.Fatalf("refusal row args = %v", args)
	}
	diff, _ := args[4].(string)
	var fields map[string]any
	if err := json.Unmarshal([]byte(diff), &fields); err != nil {
		t.Fatalf("diff %q is not JSON: %v", diff, err)
	}
	if fields["reason"] != cron.ContractTokenExpired {
		t.Fatalf("reason = %v, want %s", fields["reason"], cron.ContractTokenExpired)
	}
	for _, leak := range []string{"web_search", "cap.tool", "summarise", "approval_contract"} {
		if strings.Contains(diff, leak) {
			t.Fatalf("refusal diff leaks %q: %s", leak, diff)
		}
	}
}

// A refusal row that cannot be written is queued and written on the next cron dispatch, once. The refusal
// itself stands either way.
func TestRefusedFireRowIsRetriedOnTheNextDispatch(t *testing.T) {
	ctx := context.Background()
	now := time.Now().UTC()
	template := auditFireTemplate(t, now.Add(-time.Hour))
	refusal := refuseExpiredContract(template, now)
	flaky := &auditFlakyAuditor{failures: 1}
	d := &WorkflowDispatcher{}
	d.SetContractAuditor(flaky)
	db := &auditFireDB{}

	d.auditFireRefusal(ctx, db, "task_1", auditFireIntent(), template, refusal)
	if len(flaky.written) != 0 || d.auditRetry.Pending() != 1 {
		t.Fatalf("a failed row must be queued: written=%d queued=%d", len(flaky.written), d.auditRetry.Pending())
	}
	d.auditRetry.Flush(ctx, d.contractAudit, db)
	if len(flaky.written) != 1 || flaky.written[0].ID != cron.RefusedFireAuditID("task_1") {
		t.Fatalf("the next dispatch wrote %+v, want the queued refusal row", flaky.written)
	}
	d.auditRetry.Flush(ctx, d.contractAudit, db)
	if len(flaky.written) != 1 {
		t.Fatalf("a queued row was written twice: %d", len(flaky.written))
	}
}

// Without an auditor no refusal row is written, and nothing else about the refusal changes.
func TestRefusedFireWithoutAnAuditorWritesNothing(t *testing.T) {
	now := time.Now().UTC()
	template := auditFireTemplate(t, now.Add(-time.Hour))
	d := &WorkflowDispatcher{}
	db := &auditFireDB{}
	d.auditFireRefusal(context.Background(), db, "task_1", auditFireIntent(), template, refuseExpiredContract(template, now))
	if db.n != 0 {
		t.Fatalf("a dispatcher without an auditor wrote %d row(s)", db.n)
	}
}

// refuseExpiredContract keeps the message it always had, prefixed by its token, and the typed refusal
// carries the same token to the audit row.
func TestRefuseExpiredContractKeepsItsMessageAndToken(t *testing.T) {
	now := time.Now().UTC()
	expired := refuseExpiredContract(auditFireTemplate(t, now.Add(-time.Minute)), now)
	if expired == nil || !strings.HasPrefix(expired.Error(), "contract_expired: ") {
		t.Fatalf("expired refusal = %v, want the contract_expired prefix", expired)
	}
	if got := cron.ContractRefusalToken(expired); got != cron.ContractTokenExpired {
		t.Fatalf("expired token = %q, want contract_expired", got)
	}

	malformed := []byte(`{"approval_contract":"{\"v\":1,\"surprise\":true}"}`)
	invalid := refuseExpiredContract(malformed, now)
	if invalid == nil || !strings.HasPrefix(invalid.Error(), "contract_invalid: ") {
		t.Fatalf("malformed refusal = %v, want the contract_invalid prefix", invalid)
	}
	if got := cron.ContractRefusalToken(invalid); got != cron.ContractTokenInvalid {
		t.Fatalf("malformed token = %q, want contract_invalid", got)
	}
}

// A contract whose window runs past the maximum is refused with the window_invalid audit token. Its message
// keeps the contract_expired prefix the refusal has always had, so a switch-off deployment reads the same text.
func TestRefuseFireNamesAWindowPastTheMaximum(t *testing.T) {
	now := time.Now().UTC()
	err := refuseExpiredContract(auditFireTemplate(t, now.AddDate(0, 0, 120)), now)
	if err == nil || cron.ContractRefusalToken(err) != cron.ContractTokenWindowInvalid {
		t.Fatalf("over-long window refusal = %v, want window_invalid", err)
	}
	if !strings.HasPrefix(err.Error(), "contract_expired: ") {
		t.Fatalf("over-long window message = %q, want the contract_expired prefix it always had", err.Error())
	}
}

// The executor offers its dispatcher a flush of the refusal rows it holds on every claim tick, so a refusal row
// does not wait for another cron dispatch. A dispatcher with no queue, or with the audit off, is left alone.
func TestExecutorFlushesTheRefusalRowsOnEachTick(t *testing.T) {
	ctx := context.Background()
	now := time.Now().UTC()
	template := auditFireTemplate(t, now.Add(-time.Hour))
	refusal := refuseExpiredContract(template, now)
	flaky := &auditFlakyAuditor{failures: 1}
	d := &WorkflowDispatcher{}
	d.SetContractAuditor(flaky)
	d.auditFireRefusal(ctx, &auditFireDB{}, "task_1", auditFireIntent(), template, refusal)
	if len(flaky.written) != 0 || d.auditRetry.Pending() != 1 {
		t.Fatalf("the refusal row must be queued: written=%d queued=%d", len(flaky.written), d.auditRetry.Pending())
	}

	NewExecutor(nil, d).flushContractAudit(ctx)
	if len(flaky.written) != 1 || flaky.written[0].ID != cron.RefusedFireAuditID("task_1") {
		t.Fatalf("the executor tick wrote %+v, want the queued refusal row", flaky.written)
	}
	NewExecutor(nil, d).flushContractAudit(ctx)
	if len(flaky.written) != 1 {
		t.Fatalf("a queued row was written twice: %d", len(flaky.written))
	}

	NewExecutor(nil, NewNatsDispatcher(nil, false)).flushContractAudit(ctx)
	NewExecutor(nil, &WorkflowDispatcher{}).flushContractAudit(ctx)
	if len(flaky.written) != 1 {
		t.Fatalf("a flush without a queue wrote a row: %d", len(flaky.written))
	}
}

// The executor's tick flushes the refusal rows at the top of RunOnce, so a row queued by an earlier fire is written
// on the next tick even when that tick's claim then fails. The pool refuses every connection and the context is
// already done, so the claim fails at its Begin, after the flush has run.
func TestExecutorRunOnceFlushesTheRefusalRowsBeforeItsClaim(t *testing.T) {
	now := time.Now().UTC()
	template := auditFireTemplate(t, now.Add(-time.Hour))
	refusal := refuseExpiredContract(template, now)
	flaky := &auditFlakyAuditor{failures: 1}
	d := &WorkflowDispatcher{}
	d.SetContractAuditor(flaky)
	d.auditFireRefusal(context.Background(), &auditFireDB{}, "task_1", auditFireIntent(), template, refusal)
	if len(flaky.written) != 0 || d.auditRetry.Pending() != 1 {
		t.Fatalf("the refusal row must be queued: written=%d queued=%d", len(flaky.written), d.auditRetry.Pending())
	}

	pool, err := pgxpool.New(context.Background(), "postgres://127.0.0.1:1/none")
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	claimed, err := NewExecutor(pool, d).RunOnce(ctx)
	if !errors.Is(err, context.Canceled) || claimed != 0 {
		t.Fatalf("a tick whose claim cannot begin: claimed=%d err=%v, want no claim and context.Canceled", claimed, err)
	}
	if len(flaky.written) != 1 || flaky.written[0].ID != cron.RefusedFireAuditID("task_1") {
		t.Fatalf("the executor tick wrote %+v, want the queued refusal row", flaky.written)
	}
	if got := d.auditRetry.Pending(); got != 0 {
		t.Fatalf("queued rows after the tick = %d, want none", got)
	}
}
