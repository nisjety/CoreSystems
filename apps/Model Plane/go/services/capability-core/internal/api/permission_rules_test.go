package api

import (
	"context"
	"errors"
	"os"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// W5-074-b: the reader, the cache and the pure matcher of the org permission rules.

func TestPermissionRulesSwitchIsOnlyTheLiteralTrue(t *testing.T) {
	for value, want := range map[string]bool{"true": true, "": false, "TRUE": false, "True": false, "1": false, "yes": false, " true": false, "true ": false, "on": false} {
		got := PermissionRulesEnabledFromLookup(func(name string) string {
			if name != PermissionRulesEnabledEnv {
				t.Fatalf("read %q", name)
			}
			return value
		})
		if got != want {
			t.Errorf("%q -> %t, want %t", value, got, want)
		}
	}
}

func rule(id, tool, kind, pattern, decision string) PermissionRule {
	return PermissionRule{ID: id, ToolPattern: tool, TargetKind: kind, TargetPattern: pattern, Decision: decision}
}

func TestMatchPermissionRules(t *testing.T) {
	cases := []struct {
		name                   string
		rules                  []PermissionRule
		tool, kind, value      string
		wantDecision, wantRule string
	}{
		{"no rules", nil, "web_fetch", "domain", "a.example", "", ""},
		{"any rule covers a call with no tool", []PermissionRule{rule("r1", "*", "any", "*", "ask")}, "", "", "", "ask", "r1"},
		{"exact tool", []PermissionRule{rule("r1", "web_fetch", "any", "*", "deny")}, "web_fetch", "", "", "deny", "r1"},
		{"exact tool does not cover another", []PermissionRule{rule("r1", "web_fetch", "any", "*", "deny")}, "web_search", "", "", "", ""},
		{"prefix glob", []PermissionRule{rule("r1", "mail.*", "any", "*", "ask")}, "mail.send", "", "", "ask", "r1"},
		{"prefix glob misses", []PermissionRule{rule("r1", "mail.*", "any", "*", "ask")}, "web_fetch", "", "", "", ""},
		{"exact domain, case-insensitive", []PermissionRule{rule("r1", "*", "domain", "Blocked.Example", "deny")}, "web_fetch", "domain", "blocked.EXAMPLE", "deny", "r1"},
		{"exact domain does not cover a subdomain", []PermissionRule{rule("r1", "*", "domain", "blocked.example", "deny")}, "web_fetch", "domain", "a.blocked.example", "", ""},
		{"suffix covers itself", []PermissionRule{rule("r1", "*", "domain", "*.blocked.example", "deny")}, "web_fetch", "domain", "blocked.example", "deny", "r1"},
		{"suffix covers a subdomain", []PermissionRule{rule("r1", "*", "domain", "*.blocked.example", "deny")}, "web_fetch", "domain", "x.y.blocked.example", "deny", "r1"},
		{"suffix is not a substring match", []PermissionRule{rule("r1", "*", "domain", "*.blocked.example", "deny")}, "web_fetch", "domain", "notblocked.example", "", ""},
		{"kind must match", []PermissionRule{rule("r1", "*", "recipient", "*", "deny")}, "web_fetch", "domain", "a.example", "", ""},
		{"star covers every value of the kind", []PermissionRule{rule("r1", "*", "recipient", "*", "ask")}, "mail.send", "recipient", "corp.example", "ask", "r1"},
		{"command prefix exact token", []PermissionRule{rule("r1", "shell", "command_prefix", "rm", "deny")}, "shell", "command_prefix", "rm", "deny", "r1"},
		{"command prefix exact token misses a longer one", []PermissionRule{rule("r1", "shell", "command_prefix", "rm", "deny")}, "shell", "command_prefix", "rmdir", "", ""},
		{"command prefix glob", []PermissionRule{rule("r1", "shell", "command_prefix", "git*", "ask")}, "shell", "command_prefix", "git-lfs", "ask", "r1"},
		{"deny wins whatever the order", []PermissionRule{rule("a", "*", "any", "*", "ask"), rule("d", "*", "domain", "*", "deny")}, "web_fetch", "domain", "a.example", "deny", "d"},
		{"first ask names itself", []PermissionRule{rule("a1", "*", "any", "*", "ask"), rule("a2", "*", "any", "*", "ask")}, "web_fetch", "", "", "ask", "a1"},
		{"an unknown decision asks, never allows", []PermissionRule{rule("x", "*", "any", "*", "allow")}, "web_fetch", "", "", "ask", "x"},
		// A ZDR or waived run sends the kind and withholds the value: a rule of the kind covers the withheld target.
		{"a withheld domain is covered by the exact domain deny", []PermissionRule{rule("r1", "web_fetch", "domain", "blocked.example", "deny")}, "web_fetch", "domain", "", "deny", "r1"},
		{"a withheld domain is covered by a suffix ask", []PermissionRule{rule("r1", "*", "domain", "*.blocked.example", "ask")}, "web_fetch", "domain", "", "ask", "r1"},
		{"a withheld recipient is covered by a deny", []PermissionRule{rule("r1", "*", "recipient", "*.corp.example", "deny")}, "mail.send", "recipient", "", "deny", "r1"},
		{"a withheld command prefix is covered by a glob ask", []PermissionRule{rule("r1", "shell", "command_prefix", "git*", "ask")}, "shell", "command_prefix", "", "ask", "r1"},
		{"deny outranks an ask on a withheld target", []PermissionRule{rule("a", "*", "domain", "*", "ask"), rule("d", "*", "domain", "blocked.example", "deny")}, "web_fetch", "domain", "", "deny", "d"},
		{"a withheld target is not covered by a rule of another kind", []PermissionRule{rule("r1", "*", "recipient", "*", "deny")}, "web_fetch", "domain", "", "", ""},
		{"a call that names no kind is covered by no kind rule", []PermissionRule{rule("r1", "*", "domain", "*", "deny")}, "web_fetch", "", "", "", ""},
		{"a persistent value still needs an exact match", []PermissionRule{rule("r1", "web_fetch", "domain", "blocked.example", "deny")}, "web_fetch", "domain", "fine.example", "", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			decision, ruleID := MatchPermissionRules(tc.rules, tc.tool, tc.kind, tc.value)
			if decision != tc.wantDecision || ruleID != tc.wantRule {
				t.Fatalf("got (%q, %q), want (%q, %q)", decision, ruleID, tc.wantDecision, tc.wantRule)
			}
			if decision == policy.DecisionAllow {
				t.Fatal("a permission rule allowed a call")
			}
		})
	}
}

// The vocabularies the matcher knows are exactly the ones migration 0040's CHECKs admit.
func TestPermissionRuleVocabularyMatchesMigration0040(t *testing.T) {
	raw, err := os.ReadFile("../../migrations/0040_org_permission_rules.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ReplaceAll(string(raw), "\r\n", "\n")
	kinds := regexp.MustCompile(`target_kind IN \(([^)]*)\)`).FindStringSubmatch(sql)
	decisions := regexp.MustCompile(`decision IN \(([^)]*)\)`).FindStringSubmatch(sql)
	if kinds == nil || decisions == nil {
		t.Fatal("0040 lost its target_kind or decision CHECK")
	}
	wantKinds := "'" + strings.Join([]string{PermissionTargetDomain, PermissionTargetRecipient, PermissionTargetCommandPrefix, PermissionTargetAny}, "', '") + "'"
	if kinds[1] != wantKinds {
		t.Errorf("0040 kinds %s, matcher kinds %s", kinds[1], wantKinds)
	}
	if decisions[1] != "'"+policy.DecisionDeny+"', '"+policy.DecisionAsk+"'" {
		t.Errorf("0040 decisions %s; a rule may only deny or ask", decisions[1])
	}
	if !strings.Contains(sql, "CHECK (target_kind <> 'any' OR target_pattern = '*')") {
		t.Error("0040 no longer forces an any rule to carry '*'")
	}
}

// ---- the store and its cache ----

type permissionRuleRows struct {
	pgx.Rows
	rows [][5]string
	i    int
}

func (r *permissionRuleRows) Next() bool { r.i++; return r.i <= len(r.rows) }
func (r *permissionRuleRows) Scan(dest ...any) error {
	for i, v := range r.rows[r.i-1] {
		*dest[i].(*string) = v
	}
	return nil
}
func (r *permissionRuleRows) Err() error { return nil }
func (r *permissionRuleRows) Close()     {}

type permissionRuleDB struct {
	rows    [][5]string
	err     error
	queries int
	args    []any
}

func (d *permissionRuleDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	return pgconn.CommandTag{}, errors.New("unexpected write")
}
func (d *permissionRuleDB) QueryRow(context.Context, string, ...any) pgx.Row { return nil }
func (d *permissionRuleDB) Query(_ context.Context, _ string, args ...any) (pgx.Rows, error) {
	d.queries++
	d.args = args
	if d.err != nil {
		return nil, d.err
	}
	return &permissionRuleRows{rows: d.rows}, nil
}

func TestPermissionRuleStoreCachesAnOrgForThirtySeconds(t *testing.T) {
	db := &permissionRuleDB{rows: [][5]string{{"r1", "*", "any", "*", "deny"}}}
	store := NewPermissionRuleStore(db)
	now := time.Date(2026, 11, 12, 9, 0, 0, 0, time.UTC)
	store.now = func() time.Time { return now }

	for i := 0; i < 3; i++ {
		decision, ruleID, err := store.Match(context.Background(), "org-a", "web_fetch", "", "")
		if err != nil || decision != "deny" || ruleID != "r1" {
			t.Fatalf("Match = %q %q %v", decision, ruleID, err)
		}
	}
	if db.queries != 1 || db.args[0] != "org-a" || db.args[1] != maxPermissionRulesPerOrg+1 {
		t.Fatalf("queries=%d args=%v; want one read of org-a with the bound plus one", db.queries, db.args)
	}
	now = now.Add(29 * time.Second)
	_, _, _ = store.Match(context.Background(), "org-a", "web_fetch", "", "")
	if db.queries != 1 {
		t.Fatalf("re-read inside the TTL (%d)", db.queries)
	}
	now = now.Add(time.Second)
	_, _, _ = store.Match(context.Background(), "org-a", "web_fetch", "", "")
	if db.queries != 2 {
		t.Fatalf("no re-read at the TTL (%d)", db.queries)
	}
	// Another org is its own entry.
	_, _, _ = store.Match(context.Background(), "org-b", "web_fetch", "", "")
	if db.queries != 3 || db.args[0] != "org-b" {
		t.Fatalf("org-b was served from org-a's entry")
	}
}

func TestPermissionRuleStoreDoesNotCacheAFailedRead(t *testing.T) {
	db := &permissionRuleDB{err: errors.New("db down")}
	store := NewPermissionRuleStore(db)
	if _, _, err := store.Match(context.Background(), "org-a", "t", "", ""); err == nil {
		t.Fatal("a failed read matched")
	}
	db.err = nil
	db.rows = [][5]string{{"r1", "*", "any", "*", "ask"}}
	decision, _, err := store.Match(context.Background(), "org-a", "t", "", "")
	if err != nil || decision != "ask" || db.queries != 2 {
		t.Fatalf("after recovery: %q %v queries=%d", decision, err, db.queries)
	}
}

func TestPermissionRuleStoreRefusesAnOrgOverTheBound(t *testing.T) {
	rows := make([][5]string, maxPermissionRulesPerOrg+1)
	for i := range rows {
		rows[i] = [5]string{"r", "*", "any", "*", "ask"}
	}
	store := NewPermissionRuleStore(&permissionRuleDB{rows: rows})
	if _, _, err := store.Match(context.Background(), "org-a", "t", "", ""); err == nil {
		t.Fatal("an org over the bound was decided on part of its rules")
	}
	exactly := NewPermissionRuleStore(&permissionRuleDB{rows: rows[:maxPermissionRulesPerOrg]})
	if _, _, err := exactly.Match(context.Background(), "org-a", "t", "", ""); err != nil {
		t.Fatalf("an org at the bound was refused: %v", err)
	}
}
