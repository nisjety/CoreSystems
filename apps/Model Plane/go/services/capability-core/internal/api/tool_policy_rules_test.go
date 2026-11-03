package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

// A confirmed "Always" rule lapses ninety days out unless the person asks for a time, and never
// later than a year out (build plan WS13 S7, K3-10).
func TestARuleLapsesInNinetyDaysByDefaultAndNoLaterThanAYear(t *testing.T) {
	now := time.Date(2026, time.November, 1, 12, 0, 0, 0, time.UTC)
	lapse, err := ruleExpiry(now, "")
	if err != nil {
		t.Fatalf("the default lapse: %v", err)
	}
	if want := now.Add(90 * 24 * time.Hour); !lapse.Equal(want) {
		t.Fatalf("default lapse = %s, want ninety days out: %s", lapse, want)
	}
	if _, err := ruleExpiry(now, now.Add(365*24*time.Hour).Format(time.RFC3339)); err != nil {
		t.Fatalf("a year is the longest a rule may ask for: %v", err)
	}
	refused := map[string]string{
		"past a year":   now.Add(366 * 24 * time.Hour).Format(time.RFC3339),
		"not after now": now.Format(time.RFC3339),
		"not a time":    "next tuesday",
	}
	for name, requested := range refused {
		if _, err := ruleExpiry(now, requested); err == nil {
			t.Errorf("%s is refused: %q", name, requested)
		}
	}
}

// A lapse belongs to an allow only, and an allow may not lapse outside the window. Every refusal
// is answered before the registry is read.
func TestALapseBelongsToAnAllowAndStaysWithinAYear(t *testing.T) {
	future := time.Now().Add(time.Hour).UTC().Format(time.RFC3339)
	past := time.Now().Add(-time.Hour).UTC().Format(time.RFC3339)
	tooLong := time.Now().Add(400 * 24 * time.Hour).UTC().Format(time.RFC3339)
	tests := []struct {
		name string
		body string
	}{
		{"an ask does not lapse", `{"decision":"ask","expires_at":"` + future + `"}`},
		{"a never does not lapse", `{"decision":"never","expires_at":"` + future + `"}`},
		{"an allow cannot lapse in the past", `{"decision":"allow","pinned_digest":"` + knownAnswerToolDigest + `","expires_at":"` + past + `"}`},
		{"an allow cannot lapse after a year", `{"decision":"allow","pinned_digest":"` + knownAnswerToolDigest + `","expires_at":"` + tooLong + `"}`},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			principal := person(authz.WriteScope)
			response := serveToolPolicies(t, toolPolicyRequest(http.MethodPut, "/api/v1/tool-policies/cap.mcp_tool.x", tc.body, &principal))
			if response.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want %d, body=%s", response.Code, http.StatusBadRequest, response.Body.String())
			}
		})
	}
}

// toolPolicyScanRow answers a single read with the values it holds, in order.
type toolPolicyScanRow struct{ values []string }

func (r toolPolicyScanRow) Scan(dest ...any) error {
	if len(dest) > len(r.values) {
		return errors.New("the recording database holds fewer values than the scan asks for")
	}
	for i, target := range dest {
		text, ok := target.(*string)
		if !ok {
			return errors.New("the recording database only scans text")
		}
		*text = r.values[i]
	}
	return nil
}

// toolPolicyRecordingDB answers the reads a rule write makes (the capability's kind and its
// registry row) and keeps the arguments of every write, so the audit row can be read back without
// a Postgres.
type toolPolicyRecordingDB struct {
	t          *testing.T
	pinned     string
	statements []recordedStatement
}

type recordedStatement struct {
	sql  string
	args []any
}

func (d *toolPolicyRecordingDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.t.Fatal("a policy write is a single statement, never an Exec")
	return pgconn.CommandTag{}, nil
}

func (d *toolPolicyRecordingDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.t.Fatal("a policy write reads no list")
	return nil, nil
}

func (d *toolPolicyRecordingDB) QueryRow(_ context.Context, sql string, args ...any) pgx.Row {
	d.statements = append(d.statements, recordedStatement{sql: sql, args: args})
	switch {
	case strings.Contains(sql, "FROM capabilities"):
		return toolPolicyScanRow{values: []string{"mcp_tool"}}
	case strings.Contains(sql, "FROM capability_tools"):
		return toolPolicyScanRow{values: []string{"external_write", "none", d.pinned, reviewApproved}}
	default:
		return toolPolicyScanRow{values: []string{"tpol_recorded"}}
	}
}

// writePolicy sends one PUT through the handler and returns what the database was asked to do.
func writePolicy(t *testing.T, body string) *toolPolicyRecordingDB {
	t.Helper()
	db := &toolPolicyRecordingDB{t: t, pinned: knownAnswerToolDigest}
	mux := http.NewServeMux()
	NewToolPoliciesHandler(db).Register(mux)
	principal := person(authz.WriteScope)
	response := httptest.NewRecorder()
	mux.ServeHTTP(response, toolPolicyRequest(http.MethodPut, "/api/v1/tool-policies/cap.mcp_tool.x", body, &principal))
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d, body=%s", response.Code, http.StatusOK, response.Body.String())
	}
	return db
}

// policyWriteArgs returns the arguments of the one statement that writes the policy.
func policyWriteArgs(t *testing.T, db *toolPolicyRecordingDB) []any {
	t.Helper()
	for _, statement := range db.statements {
		if statement.sql == setToolPolicySQL {
			return statement.args
		}
	}
	t.Fatal("the policy write was never sent")
	return nil
}

// An allow is a rule: the statement carries its lapse, its audit row is created on the first write
// and confirmed on a renewal, and the row names the scope, decision, digest and lapse but never the
// reason the person wrote (K3-10).
func TestAnAllowRuleIsAuditedAsCreatedAndItsRowIsContentFree(t *testing.T) {
	const reason = "a private note about this tool"
	body := `{"decision":"allow","pinned_digest":"` + knownAnswerToolDigest + `","reason":"` + reason + `"}`
	args := policyWriteArgs(t, writePolicy(t, body))

	const (
		updateAction = 10 // $11: the action when the policy already existed
		diffArg      = 11 // $12: the audit diff
		lapseArg     = 12 // $13: the lapse time
		insertAction = 13 // $14: the action when the policy was created
	)
	if got := args[updateAction]; got != "tool_policy_rule_confirmed" {
		t.Errorf("a renewal is audited as %v, want tool_policy_rule_confirmed", got)
	}
	if got := args[insertAction]; got != "tool_policy_rule_created" {
		t.Errorf("a first write is audited as %v, want tool_policy_rule_created", got)
	}
	lapse, ok := args[lapseArg].(*time.Time)
	if !ok || lapse == nil {
		t.Fatalf("the statement carries no lapse time: %#v", args[lapseArg])
	}
	if d := time.Until(*lapse) - 90*24*time.Hour; d > time.Minute || d < -time.Minute {
		t.Errorf("the default lapse is %s away, want about ninety days", time.Until(*lapse))
	}
	diff := args[diffArg].(string)
	if strings.Contains(diff, reason) {
		t.Fatalf("the audit diff carries the person's reason: %s", diff)
	}
	var fields map[string]any
	if err := json.Unmarshal([]byte(diff), &fields); err != nil {
		t.Fatalf("the audit diff is not JSON: %v", err)
	}
	for _, key := range []string{"capability_id", "scope_kind", "scope_value", "decision", "pinned_digest", "expires_at"} {
		if _, ok := fields[key]; !ok {
			t.Errorf("the audit diff lacks %s: %s", key, diff)
		}
	}
	if len(fields) != 6 {
		t.Errorf("the audit diff carries more than the content-free fields: %s", diff)
	}
}

// Every other write is audited as before and carries no lapse, so an ask keeps its old row.
func TestAnAskPolicyIsAuditedAsBeforeAndNeverLapses(t *testing.T) {
	args := policyWriteArgs(t, writePolicy(t, `{"decision":"ask","reason":"check each time"}`))
	if got := args[10]; got != "tool_policy_set" {
		t.Errorf("an ask renewal is audited as %v, want tool_policy_set", got)
	}
	if got := args[13]; got != "tool_policy_set" {
		t.Errorf("an ask is audited as %v, want tool_policy_set", got)
	}
	if lapse, ok := args[12].(*time.Time); !ok || lapse != nil {
		t.Errorf("an ask carries a lapse: %#v", args[12])
	}
	if strings.Contains(args[11].(string), "expires_at") {
		t.Errorf("an ask's audit diff names a lapse: %s", args[11])
	}
}
