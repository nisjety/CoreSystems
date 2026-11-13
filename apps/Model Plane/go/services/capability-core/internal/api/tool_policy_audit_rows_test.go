package api

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// W2-WS13-S7-rule-use-b and rule-expiry: the two new audit rows of a stored "Always" rule are
// content-free, and the expiry sweep is one bounded statement.

type auditCaptureDB struct {
	execSQL  string
	execArgs []any
	execErr  error
	rowSQL   string
	rowArgs  []any
	rowCount int
	rowErr   error
}

func (d *auditCaptureDB) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	d.execSQL, d.execArgs = sql, args
	return pgconn.CommandTag{}, d.execErr
}

func (d *auditCaptureDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	return nil, errors.New("unexpected query")
}

func (d *auditCaptureDB) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	d.rowSQL, d.rowArgs = sql, args
	return countRow{n: d.rowCount, err: d.rowErr}
}

type countRow struct {
	n   int
	err error
}

func (r countRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	*dest[0].(*int) = r.n
	return nil
}

func TestRecordRuleUseWritesOneContentFreeAuditRow(t *testing.T) {
	db := &auditCaptureDB{}
	handler := NewToolPoliciesHandler(db)
	use := RuleUse{OrgID: "org-a", PolicyID: "tp_1", CapabilityID: "cap.mcp_tool.x", ScopeKind: "user",
		RunID: "run-a", ToolDigest: strings.Repeat("a", 64), Actor: "execution-core"}
	if err := handler.RecordRuleUse(context.Background(), use); err != nil {
		t.Fatalf("RecordRuleUse: %v", err)
	}
	if !strings.Contains(db.execSQL, "INSERT INTO registry_audit_log") {
		t.Fatalf("sql = %s", db.execSQL)
	}
	if len(db.execArgs) != 7 {
		t.Fatalf("args = %v", db.execArgs)
	}
	id, _ := db.execArgs[0].(string)
	if !strings.HasPrefix(id, "ral_") || db.execArgs[1] != "tool_policy" || db.execArgs[2] != "tp_1" ||
		db.execArgs[3] != "tool_policy_rule_used" || db.execArgs[4] != "execution-core" || db.execArgs[5] != "org-a" {
		t.Fatalf("row = %v", db.execArgs)
	}
	var diff map[string]any
	if err := json.Unmarshal([]byte(db.execArgs[6].(string)), &diff); err != nil {
		t.Fatalf("diff_json: %v", err)
	}
	keys := make([]string, 0, len(diff))
	for key := range diff {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	if strings.Join(keys, ",") != "capability_id,run_id,scope_kind,tool_digest" {
		t.Fatalf("diff_json keys = %v; only ids, a closed scope word and a digest are allowed", keys)
	}
}

func TestRecordRuleUseReturnsTheWriteError(t *testing.T) {
	db := &auditCaptureDB{execErr: errors.New("db down")}
	if err := NewToolPoliciesHandler(db).RecordRuleUse(context.Background(), RuleUse{PolicyID: "tp_1"}); err == nil {
		t.Fatal("a failed write was reported as recorded; the gate would relax without a row")
	}
}

func TestToolPolicyExpirySweepIsOneBoundedContentFreeStatement(t *testing.T) {
	db := &auditCaptureDB{rowCount: 3}
	sweeper := NewToolPolicyExpirySweeper(db)
	if sweeper.interval.Minutes() != 5 || sweeper.batch != 200 {
		t.Fatalf("interval=%v batch=%d; want 5m and 200", sweeper.interval, sweeper.batch)
	}
	n, err := sweeper.SweepOnce(context.Background())
	if err != nil || n != 3 {
		t.Fatalf("SweepOnce = %d, %v", n, err)
	}
	if len(db.rowArgs) != 2 || db.rowArgs[0] != 200 || db.rowArgs[1] != "tool_policy_rule_expired" {
		t.Fatalf("args = %v; want the batch and the action", db.rowArgs)
	}
	sql := db.rowSQL
	for _, must := range []string{
		"decision = 'allow'", "revoked_at IS NULL", "expires_at <= now()", "expiry_audited_at IS NULL",
		"FOR UPDATE SKIP LOCKED", "LIMIT $1", "SET expiry_audited_at = now()", "ON CONFLICT (id) DO NOTHING",
		"'tool_policy'", "'system:capability-core'",
	} {
		if !strings.Contains(sql, must) {
			t.Errorf("sweep statement lost %q", must)
		}
	}
	// The diff names a capability, a closed scope word and a time; never the reason or the person.
	for _, never := range []string{"reason", "set_by", "scope_value", "pinned_digest", "DELETE", "revoked_at ="} {
		if strings.Contains(sql, never) {
			t.Errorf("sweep statement touches %q", never)
		}
	}
}

func TestToolPolicyExpirySweepReportsAFailedStatement(t *testing.T) {
	db := &auditCaptureDB{rowErr: errors.New("db down")}
	if n, err := NewToolPolicyExpirySweeper(db).SweepOnce(context.Background()); err == nil || n != 0 {
		t.Fatalf("SweepOnce = %d, %v; want 0 and the error", n, err)
	}
}

// Start stops when its context ends, without a sweep (the first tick is five minutes away).
func TestToolPolicyExpirySweeperStopsWithItsContext(t *testing.T) {
	db := &auditCaptureDB{}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { NewToolPolicyExpirySweeper(db).Start(ctx); close(done) }()
	cancel()
	<-done
	if db.rowSQL != "" {
		t.Fatal("the sweeper ran a statement before its first tick")
	}
}

// A renewed (re-confirmed) rule clears the stamp, so a rule that lapses again is audited again.
func TestRenewingARuleClearsItsExpiryStamp(t *testing.T) {
	update := setToolPolicySQL[:strings.Index(setToolPolicySQL, "), ins AS")]
	if !strings.Contains(update, "expiry_audited_at = NULL") {
		t.Fatal("the re-confirm UPDATE keeps the expiry stamp; a renewed rule would never be audited again")
	}
}
