package cron

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// auditContractText is one contract a schedule might store: one read tool, a closed window.
const auditContractText = `{"expires_at":"2026-12-01T09:00:00Z","max_rounds":6,"max_tokens_per_round":2048,"tools":[{"approved_at":"2026-10-24T10:00:00Z","capability":"cap.tool.web_search","class":"read","tool":"web_search"}],"v":1}`

// auditExecer stands in for a pool or a transaction. It records every statement it is given.
type auditExecer struct {
	execs []recordedExec
	err   error
}

func (e *auditExecer) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	e.execs = append(e.execs, recordedExec{sql: sql, args: args})
	return pgconn.CommandTag{}, e.err
}

// recordingAuditor keeps each row it is asked to write, and writes nothing.
type recordingAuditor struct {
	rows []ContractAuditRow
}

func (a *recordingAuditor) Append(_ context.Context, _ AuditExecer, row ContractAuditRow) error {
	a.rows = append(a.rows, row)
	return nil
}

// failingAuditor refuses every row, as a registry that is down would.
type failingAuditor struct{ calls int }

func (a *failingAuditor) Append(context.Context, AuditExecer, ContractAuditRow) error {
	a.calls++
	return errors.New("registry_audit_log is unavailable")
}

func decodeAuditDiff(t *testing.T, raw []byte) map[string]any {
	t.Helper()
	var diff map[string]any
	if err := json.Unmarshal(raw, &diff); err != nil {
		t.Fatalf("diff %s is not a JSON object: %v", raw, err)
	}
	return diff
}

func keysOf(diff map[string]any) []string {
	keys := make([]string, 0, len(diff))
	for key := range diff {
		keys = append(keys, key)
	}
	return keys
}

// A contract's diff names its digest, its tool count, its expiry and its class, and nothing about its
// tools, its arguments, its goal or its text.
func TestContractAuditDiffCarriesOnlyClosedFields(t *testing.T) {
	raw := ContractAuditDiffJSON(auditContractText, "")
	diff := decodeAuditDiff(t, raw)
	if want := []string{"class", "contract_digest", "expires_at", "tool_count"}; !reflect.DeepEqual(sortedKeys(keysOf(diff)), want) {
		t.Fatalf("diff keys = %v, want exactly %v", sortedKeys(keysOf(diff)), want)
	}
	if diff["contract_digest"] != ContractDigest(auditContractText) {
		t.Fatalf("contract_digest = %v, want the digest of the stored text", diff["contract_digest"])
	}
	if diff["tool_count"] != float64(1) || diff["expires_at"] != "2026-12-01T09:00:00Z" || diff["class"] != "read" {
		t.Fatalf("diff = %v, want one tool, the expiry and the read class", diff)
	}
	for _, leak := range []string{"web_search", "cap.tool", "approved_at", "max_rounds", "approval_contract", "summarise"} {
		if strings.Contains(string(raw), leak) {
			t.Fatalf("diff leaks %q: %s", leak, raw)
		}
	}
}

// A refusal's diff carries the closed reason token and whatever the text can be read to say.
func TestRefusalDiffCarriesItsClosedReasonOnly(t *testing.T) {
	raw := ContractAuditDiffJSON(auditContractText, ContractTokenExpired)
	diff := decodeAuditDiff(t, raw)
	if diff["reason"] != "contract_expired" {
		t.Fatalf("reason = %v, want contract_expired", diff["reason"])
	}
	if strings.Contains(string(raw), "web_search") {
		t.Fatalf("a refusal diff leaks a tool name: %s", raw)
	}
}

// A text that cannot be read says nothing about its tools, its expiry or its class.
func TestUnreadableContractNamesNothingAboutItsTools(t *testing.T) {
	diff := decodeAuditDiff(t, ContractAuditDiffJSON(`{"v":2,"tools":"web_search"}`, ContractTokenInvalid))
	if want := []string{"contract_digest", "reason"}; !reflect.DeepEqual(sortedKeys(keysOf(diff)), want) {
		t.Fatalf("unreadable diff keys = %v, want %v", sortedKeys(keysOf(diff)), want)
	}
	if got := string(ContractAuditDiffJSON("", "")); got != "{}" {
		t.Fatalf("an empty diff = %s, want {}", got)
	}
}

func sortedKeys(keys []string) []string {
	for i := range keys {
		for j := i + 1; j < len(keys); j++ {
			if keys[j] < keys[i] {
				keys[i], keys[j] = keys[j], keys[i]
			}
		}
	}
	return keys
}

// A refusal's token is one of the closed set, and an error that names none is contract_invalid. The
// refusal's message is unchanged, so an answer that prints it reads as it did.
func TestContractRefusalTokensAreClosed(t *testing.T) {
	inner := errors.New("schedule contract max_rounds must be between 1 and 8")
	refusal := &ContractRefusal{Token: ContractTokenInvalid, Err: inner}
	if refusal.Error() != inner.Error() {
		t.Fatalf("refusal message = %q, want the inner message %q", refusal.Error(), inner.Error())
	}
	if got := ContractRefusalToken(fmt.Errorf("wrapped: %w", refusal)); got != ContractTokenInvalid {
		t.Fatalf("wrapped token = %q, want %q", got, ContractTokenInvalid)
	}
	if got := ContractRefusalToken(errors.New("registry read failed")); got != ContractTokenInvalid {
		t.Fatalf("an unnamed error gave token %q, want contract_invalid", got)
	}
	if !errors.Is(refusal, inner) {
		t.Fatal("a refusal must unwrap to the error it names")
	}
}

// An expired window and a window past the maximum are told apart, because the operator needs to know which.
func TestContractWindowTokenSeparatesExpiryFromWindow(t *testing.T) {
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	expired := ScheduleContract{ExpiresAt: "2026-11-01T09:00:00Z"}
	tooLong := ScheduleContract{ExpiresAt: "2027-06-01T09:00:00Z"}
	if got := ContractWindowToken(expired, now); got != ContractTokenExpired {
		t.Fatalf("expired window token = %q, want contract_expired", got)
	}
	if got := ContractWindowToken(tooLong, now); got != ContractTokenWindowInvalid {
		t.Fatalf("over-long window token = %q, want window_invalid", got)
	}
}

// The ids of expiry and refusal rows are fixed to their slot, so a retried write lands once and a
// different slot never collides.
func TestContractAuditIDsAreFixedToTheirSlot(t *testing.T) {
	first := ExpiredAuditID("sched-1", "2026-11-23T09:00:00Z")
	if first != ExpiredAuditID("sched-1", "2026-11-23T09:00:00Z") {
		t.Fatal("an expiry id must be the same on every attempt for the same slot")
	}
	if first == ExpiredAuditID("sched-1", "2026-11-24T09:00:00Z") || first == ExpiredAuditID("sched-2", "2026-11-23T09:00:00Z") {
		t.Fatal("different slots or schedules must not share an expiry id")
	}
	if !strings.HasPrefix(first, "ral_expired_") {
		t.Fatalf("expiry id %q lacks its prefix", first)
	}
	if RefusedFireAuditID("task_1") != RefusedFireAuditID("task_1") || RefusedFireAuditID("task_1") == RefusedFireAuditID("task_2") {
		t.Fatal("a refusal id must be fixed to its task")
	}
}

// The writer inserts into registry_audit_log with the entity kind fixed to schedule, and a fixed id lands
// once: ON CONFLICT (id) DO NOTHING is part of the statement.
func TestContractAuditAppendWritesOneScheduleRow(t *testing.T) {
	db := &auditExecer{}
	auditor := registryContractAuditor{}
	row := ContractAuditRow{
		ID: "ral_expired_fixed", EntityID: "sched-1", Action: ContractAuditExpired,
		Actor: SweeperAuditActor, OrgID: "org-1", Diff: []byte(`{"reason":"x"}`),
	}
	if err := auditor.Append(context.Background(), db, row); err != nil {
		t.Fatal(err)
	}
	if err := auditor.Append(context.Background(), db, row); err != nil {
		t.Fatal(err)
	}
	for _, exec := range db.execs {
		if !strings.Contains(exec.sql, "INSERT INTO registry_audit_log") || !strings.Contains(exec.sql, "ON CONFLICT (id) DO NOTHING") {
			t.Fatalf("audit insert must be idempotent by id: %s", exec.sql)
		}
		want := []any{"ral_expired_fixed", "schedule", "sched-1", ContractAuditExpired, SweeperAuditActor, "org-1", `{"reason":"x"}`}
		if !reflect.DeepEqual(exec.args, want) {
			t.Fatalf("audit args = %v, want %v", exec.args, want)
		}
	}
	if len(db.execs) != 2 {
		t.Fatalf("statements = %d, want one per Append", len(db.execs))
	}
}

// A row with no id gets a fresh one, as the registry's own rows do.
func TestContractAuditAppendMakesAFreshIdWhenNoneIsFixed(t *testing.T) {
	db := &auditExecer{}
	if err := (registryContractAuditor{}).Append(context.Background(), db, ContractAuditRow{Action: ContractAuditCreated}); err != nil {
		t.Fatal(err)
	}
	if id, _ := db.execs[0].args[0].(string); !strings.HasPrefix(id, "ral_") || len(id) < 8 {
		t.Fatalf("a row without an id got %q, want a fresh ral_ id", id)
	}
}

// The auditor needs the registry pool; a nil pool is an error, so contract audit cannot be quietly absent.
func TestContractAuditorNeedsTheRegistryPool(t *testing.T) {
	if _, err := NewContractAuditor(nil); err == nil {
		t.Fatal("a nil pool must refuse to build the contract auditor")
	}
}

// The guard pause writes its expiry row once, in the pause's own transaction, after the pause itself.
func TestExpiredPauseWritesItsRowOnceInThePauseTransaction(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	slot := now.Add(-time.Minute)
	authorizer := &countingAuthorizer{}
	sweeper := (&Sweeper{authorizer: authorizer}).WithContractGuards(true).WithContractAuditor(registryContractAuditor{})
	tx := &guardBatchTx{}
	var pauses []pauseNotice

	fired, err := sweeper.sweepDue(ctx, tx, guardDue(guardTemplate(t, now.Add(-time.Hour)), slot), now, &pauses)
	if err != nil || fired {
		t.Fatalf("expired pause: fired=%v err=%v, want a clean pause", fired, err)
	}
	pauseAt, auditAt, audits := -1, -1, 0
	for i, exec := range tx.execs {
		switch {
		case strings.Contains(exec.sql, "INSERT INTO cron_fires") && len(exec.args) == 7 && exec.args[6] == "paused_expired":
			pauseAt = i
		case strings.Contains(exec.sql, "INSERT INTO registry_audit_log"):
			audits++
			auditAt = i
			wantID := ExpiredAuditID("sched-1", slot.UTC().Format(time.RFC3339Nano))
			if exec.args[0] != wantID || exec.args[1] != "schedule" || exec.args[2] != "sched-1" ||
				exec.args[3] != ContractAuditExpired || exec.args[4] != SweeperAuditActor || exec.args[5] != "org-1" {
				t.Fatalf("expiry row args = %v", exec.args)
			}
			diff := decodeAuditDiff(t, []byte(exec.args[6].(string)))
			if diff["contract_digest"] == nil || diff["tool_count"] != float64(1) || diff["class"] != "read" {
				t.Fatalf("expiry row diff = %v, want digest, count and class", diff)
			}
			if strings.Contains(exec.args[6].(string), "web_search") || strings.Contains(exec.args[6].(string), "summarise") {
				t.Fatalf("expiry row diff leaks content: %v", exec.args[6])
			}
		}
	}
	if audits != 1 {
		t.Fatalf("expiry rows written = %d, want exactly one", audits)
	}
	if pauseAt < 0 || auditAt < pauseAt {
		t.Fatalf("the expiry row must follow the pause in the same transaction (pause %d, row %d)", pauseAt, auditAt)
	}
	if authorizer.calls != 0 {
		t.Fatalf("an expired approval reached the fire authorizer %d time(s)", authorizer.calls)
	}
}

// A failed expiry row never undoes the pause. The row waits on its pause until the batch commits, and the next
// tick then writes it once.
func TestFailedExpiredRowNeverBlocksThePauseAndIsRetried(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	slot := now.Add(-time.Minute)
	failing := &failingAuditor{}
	sweeper := (&Sweeper{authorizer: &countingAuthorizer{}}).WithContractGuards(true).WithContractAuditor(failing)
	tx := &guardBatchTx{}
	var pauses []pauseNotice

	fired, err := sweeper.sweepDue(ctx, tx, guardDue(guardTemplate(t, now.Add(-time.Hour)), slot), now, &pauses)
	if err != nil || fired {
		t.Fatalf("a failed expiry row must not fail the pause: fired=%v err=%v", fired, err)
	}
	var disabled, paused bool
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "SET enabled=false") {
			disabled = true
		}
		if strings.Contains(exec.sql, "INSERT INTO cron_fires") && len(exec.args) == 7 && exec.args[6] == "paused_expired" {
			paused = true
		}
	}
	if !disabled || !paused || len(pauses) != 1 {
		t.Fatalf("the pause must stand: disabled=%v paused=%v pauses=%d", disabled, paused, len(pauses))
	}
	if failing.calls != 1 {
		t.Fatalf("append attempts = %d, want one", failing.calls)
	}
	if len(pauses) != 1 || pauses[0].unwritten == nil || pauses[0].unwritten.Action != ContractAuditExpired {
		t.Fatalf("the failed row must wait on its pause: %+v", pauses)
	}
	if got := sweeper.auditRetry.Pending(); got != 0 {
		t.Fatalf("queued before the batch committed: %d rows", got)
	}
	if err := sweeper.commitBatch(ctx, tx, pauses); err != nil {
		t.Fatalf("commit: %v", err)
	}
	if got := sweeper.auditRetry.Pending(); got != 1 {
		t.Fatalf("queued rows after the commit = %d, want the failed expiry row", got)
	}

	// The next tick writes the queued row once, and a further tick writes nothing more.
	written := &recordingAuditor{}
	sweeper.auditRetry.Flush(ctx, written, nil)
	if len(written.rows) != 1 || written.rows[0].Action != ContractAuditExpired {
		t.Fatalf("retry wrote %+v, want the one expiry row", written.rows)
	}
	sweeper.auditRetry.Flush(ctx, written, nil)
	if len(written.rows) != 1 {
		t.Fatalf("a row written once was written again: %d rows", len(written.rows))
	}
}

// A row that fails again on retry stays queued, in order, ahead of the rows queued after it.
func TestRetryKeepsARowThatFailsAgain(t *testing.T) {
	ctx := context.Background()
	queue := &ContractAuditRetry{}
	queue.Add(ContractAuditRow{ID: "first", Action: ContractAuditExpired})
	queue.Flush(ctx, &failingAuditor{}, nil)
	queue.Add(ContractAuditRow{ID: "second", Action: ContractAuditRefused})
	written := &recordingAuditor{}
	queue.Flush(ctx, written, nil)
	if len(written.rows) != 2 || written.rows[0].ID != "first" || written.rows[1].ID != "second" {
		t.Fatalf("retry order = %+v, want first then second", written.rows)
	}
}

// With no auditor the pause writes no expiry row, and its statements are exactly the ones it always wrote.
func TestExpiredPauseWithoutAnAuditorIsUnchanged(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	sweeper := (&Sweeper{authorizer: &countingAuthorizer{}}).WithContractGuards(true)
	tx := &guardBatchTx{}
	var pauses []pauseNotice
	if _, err := sweeper.sweepDue(ctx, tx, guardDue(guardTemplate(t, now.Add(-time.Hour)), now.Add(-time.Minute)), now, &pauses); err != nil {
		t.Fatal(err)
	}
	if len(tx.execs) != 2 {
		t.Fatalf("statements = %d, want the disable and the paused fire only", len(tx.execs))
	}
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "registry_audit_log") {
			t.Fatalf("a sweeper without an auditor wrote an audit row: %s", exec.sql)
		}
	}
}

// The retry queue holds a bounded number of rows and drops the oldest past its limit.
func TestRetryQueueIsBoundedAndDropsTheOldest(t *testing.T) {
	queue := &ContractAuditRetry{}
	for i := 0; i < contractAuditRetryLimit+10; i++ {
		queue.Add(ContractAuditRow{ID: fmt.Sprint(i)})
	}
	if len(queue.pending) != contractAuditRetryLimit {
		t.Fatalf("queued %d rows, want the limit %d", len(queue.pending), contractAuditRetryLimit)
	}
	if queue.pending[0].ID != "10" {
		t.Fatalf("oldest kept row = %q, want 10", queue.pending[0].ID)
	}
}

// uncommittedBatchTx is a batch whose commit fails, as a lost connection would make it. Its statements run
// as guardBatchTx's do.
type uncommittedBatchTx struct{ *guardBatchTx }

func (tx *uncommittedBatchTx) Commit(context.Context) error {
	return errors.New("commit: connection lost")
}

// A batch that does not commit rolls its pauses back, so the expiry row of one of them is dropped, never queued.
// The next tick would otherwise record an expiry for a pause that never committed.
func TestExpiredRowOfABatchThatDoesNotCommitIsNeverQueued(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	slot := now.Add(-time.Minute)
	sweeper := (&Sweeper{authorizer: &countingAuthorizer{}}).WithContractGuards(true).WithContractAuditor(&failingAuditor{})
	tx := &uncommittedBatchTx{guardBatchTx: &guardBatchTx{}}
	var pauses []pauseNotice

	if _, err := sweeper.sweepDue(ctx, tx, guardDue(guardTemplate(t, now.Add(-time.Hour)), slot), now, &pauses); err != nil {
		t.Fatal(err)
	}
	if len(pauses) != 1 || pauses[0].unwritten == nil {
		t.Fatalf("the failed row must wait on its pause: %+v", pauses)
	}
	if err := sweeper.commitBatch(ctx, tx, pauses); err == nil {
		t.Fatal("a batch whose commit fails must report it")
	}
	if got := sweeper.auditRetry.Pending(); got != 0 {
		t.Fatalf("queued rows = %d, want none: the pause did not commit", got)
	}
	written := &recordingAuditor{}
	sweeper.auditRetry.Flush(ctx, written, nil)
	if len(written.rows) != 0 {
		t.Fatalf("the next tick wrote %+v for a pause that never committed", written.rows)
	}
}

// The durable recovery runs only while the approval guards and the audit sink are both on. It runs on the first
// sweep after a start, and then once per interval.
func TestExpiredAuditRecoveryIsGatedAndPaced(t *testing.T) {
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	gated := map[string]*Sweeper{
		"switched off": {},
		"guards only":  (&Sweeper{}).WithContractGuards(true),
		"sink only":    (&Sweeper{}).WithContractAuditor(&recordingAuditor{}),
	}
	for name, s := range gated {
		if s.expiredAuditRecoveryDue(now) || !s.auditRecoverAt.IsZero() {
			t.Fatalf("recovery ran with %s", name)
		}
	}
	on := (&Sweeper{}).WithContractGuards(true).WithContractAuditor(&recordingAuditor{})
	if !on.expiredAuditRecoveryDue(now) {
		t.Fatal("the first sweep after a start must recover")
	}
	if on.expiredAuditRecoveryDue(now.Add(expiredAuditRecoveryInterval - time.Second)) {
		t.Fatal("recovery ran again before its interval had passed")
	}
	if !on.expiredAuditRecoveryDue(now.Add(expiredAuditRecoveryInterval)) {
		t.Fatal("recovery did not run once its interval had passed")
	}
}

// The first sweep after a start reaches the durable recovery before its batch begins, so the recovery is armed even
// when the batch then fails. The pool refuses every connection and the context is already done, so the batch fails at
// its Begin. A sweep with either switch off never arms the recovery.
func TestFirstSweepArmsTheExpiredAuditRecoveryBeforeItsBatch(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	on := (&Sweeper{pool: unreachableSweepPool(t), authorizer: &countingAuthorizer{}}).WithContractGuards(true).WithContractAuditor(&recordingAuditor{})
	if _, err := on.RunOnce(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("a sweep whose batch cannot begin: err=%v, want context.Canceled", err)
	}
	if on.auditRecoverAt.IsZero() {
		t.Fatal("the first sweep after a start must reach the recovery before its batch begins")
	}

	twins := []struct {
		name string
		s    *Sweeper
	}{
		{"with the switches off", &Sweeper{pool: unreachableSweepPool(t), authorizer: &countingAuthorizer{}}},
		{"without the audit sink", (&Sweeper{pool: unreachableSweepPool(t), authorizer: &countingAuthorizer{}}).WithContractGuards(true)},
		{"without the approval guards", (&Sweeper{pool: unreachableSweepPool(t), authorizer: &countingAuthorizer{}}).WithContractAuditor(&recordingAuditor{})},
	}
	for _, twin := range twins {
		if _, err := twin.s.RunOnce(ctx); !errors.Is(err, context.Canceled) {
			t.Fatalf("a sweep %s: err=%v, want context.Canceled", twin.name, err)
		}
		if !twin.s.auditRecoverAt.IsZero() {
			t.Fatalf("the recovery ran in a sweep %s", twin.name)
		}
	}
}

// A sweep writes the expiry rows queued by an earlier tick before its own batch begins, so a queued row lands even
// when the batch then fails. The pool refuses every connection and the context is already done, so the batch fails at
// its Begin. The guards are off here, so the durable recovery stays out of the way and only the flush can write the row.
func TestSweepRunOnceFlushesTheQueuedExpiryRowsBeforeItsBatch(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	written := &recordingAuditor{}
	s := (&Sweeper{pool: unreachableSweepPool(t), authorizer: &countingAuthorizer{}}).WithContractAuditor(written)
	queued := ContractAuditRow{ID: "ral_expired_queued", Action: ContractAuditExpired}
	s.auditRetry.Add(queued)

	if _, err := s.RunOnce(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("a sweep whose batch cannot begin: err=%v, want context.Canceled", err)
	}
	if len(written.rows) != 1 || !reflect.DeepEqual(written.rows[0], queued) {
		t.Fatalf("rows written before the batch = %+v, want exactly the queued row", written.rows)
	}
	if got := s.auditRetry.Pending(); got != 0 {
		t.Fatalf("rows still queued = %d, want none once the flush has written them", got)
	}
}

// The recovery writes the expiry row of each disabled schedule whose approval has lapsed, under the id its pause
// wrote: the schedule's next_fire_at. A schedule whose approval still holds, or that has none, gets no row.
func TestRecoveryWritesTheMissingExpiryRowOfEachLapsedSchedule(t *testing.T) {
	ctx := context.Background()
	now := time.Date(2026, 11, 23, 9, 0, 0, 0, time.UTC)
	slot := now.Add(-time.Minute)
	lapsed := guardDue(guardTemplate(t, now.Add(-time.Hour)), slot)
	live := guardDue(guardTemplate(t, now.Add(time.Hour)), slot)
	live.id = "sched-live"
	draft := guardDue(json.RawMessage(`{"kind":"cron","title":"draft"}`), slot)
	draft.id = "sched-draft"
	unset := guardDue(guardTemplate(t, now.Add(-time.Hour)), slot)
	unset.id = "sched-unset"
	unset.nextFire = nil
	schedules := []dueSchedule{lapsed, live, draft, unset}

	// A write that fails is logged and the rest still go on. The next recovery then writes the same rows.
	failing := &failingAuditor{}
	writeMissingExpiredAudits(ctx, failing, nil, schedules, now)
	if failing.calls != 2 {
		t.Fatalf("write attempts = %d, want one per lapsed schedule", failing.calls)
	}
	written := &recordingAuditor{}
	writeMissingExpiredAudits(ctx, written, nil, schedules, now)
	if len(written.rows) != 2 {
		t.Fatalf("expiry rows = %d, want one per lapsed schedule", len(written.rows))
	}
	if want := ExpiredAuditID("sched-1", slot.UTC().Format(time.RFC3339Nano)); written.rows[0].ID != want {
		t.Fatalf("row id = %q, want the id its pause wrote %q", written.rows[0].ID, want)
	}
	if want := ExpiredAuditID("sched-unset", ""); written.rows[1].ID != want {
		t.Fatalf("row id = %q, want the id of a schedule with no slot %q", written.rows[1].ID, want)
	}
	for _, row := range written.rows {
		if row.Action != ContractAuditExpired || row.Actor != SweeperAuditActor || row.OrgID != "org-1" {
			t.Fatalf("row = %+v, want the sweeper's expiry row for org-1", row)
		}
		if strings.Contains(string(row.Diff), "web_search") {
			t.Fatalf("recovery diff leaks a tool name: %s", row.Diff)
		}
	}
}

// scheduleRowsReader answers the recovery's read with the rows it was given, and keeps the statement it was asked.
type scheduleRowsReader struct {
	sql  string
	rows []scheduleRow
}

// scheduleRow is one row of the recovery's read.
type scheduleRow struct {
	id, orgID string
	template  json.RawMessage
	nextFire  *time.Time
}

func (r *scheduleRowsReader) Query(_ context.Context, sql string, _ ...any) (pgx.Rows, error) {
	r.sql = sql
	return &scheduleRows{rows: r.rows}, nil
}

// scheduleRows yields the rows of a scheduleRowsReader.
type scheduleRows struct {
	pgx.Rows
	rows []scheduleRow
	next int
}

func (rows *scheduleRows) Next() bool {
	if rows.next >= len(rows.rows) {
		return false
	}
	rows.next++
	return true
}

func (rows *scheduleRows) Scan(dest ...any) error {
	row := rows.rows[rows.next-1]
	*dest[0].(*string) = row.id
	*dest[1].(*string) = row.orgID
	*dest[2].(*json.RawMessage) = row.template
	*dest[3].(**time.Time) = row.nextFire
	return nil
}

func (rows *scheduleRows) Err() error { return nil }

func (rows *scheduleRows) Close() {}

// The recovery reads only disabled schedules that are not deleted. An enabled schedule is the sweep's to pause, and
// a deleted one is neither fired nor recovered.
func TestLoadDisabledSchedulesReadsOnlyDisabledLiveSchedules(t *testing.T) {
	ctx := context.Background()
	slot := time.Date(2026, 11, 22, 9, 0, 0, 0, time.UTC)
	reader := &scheduleRowsReader{rows: []scheduleRow{
		{id: "sched-1", orgID: "org-1", template: json.RawMessage(`{"kind":"cron"}`), nextFire: &slot},
		{id: "sched-2", orgID: "org-2", template: json.RawMessage(`{"kind":"cron"}`)},
	}}
	schedules, err := loadDisabledSchedules(ctx, reader)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(reader.sql, "WHERE NOT enabled AND deleted_at IS NULL") {
		t.Fatalf("recovery read %q, want disabled live schedules only", reader.sql)
	}
	if len(schedules) != 2 || schedules[0].id != "sched-1" || schedules[0].orgID != "org-1" ||
		schedules[0].nextFire == nil || !schedules[0].nextFire.Equal(slot) || schedules[1].nextFire != nil {
		t.Fatalf("schedules = %+v", schedules)
	}
}
