package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	pluginloader "github.com/triodelab/model-plane/services/capability-core/internal/plugins"
)

// killFakeDB is the pool a kill and an uninstall run against. Query answers the two reads the loader makes: the locked
// plugin rows of a digest (packages) and the installed components of one plugin (components, keyed by plugin id). Every
// Exec is recorded, so each skill removal can be read back next to the actor statement that named its posture.
type killFakeDB struct {
	packages   [][]any
	components map[string][][]any
	execs      []databaseCall
	commits    int
}

func (db *killFakeDB) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	db.execs = append(db.execs, databaseCall{query: query, args: append([]any(nil), args...)})
	return pgconn.NewCommandTag("UPDATE 1"), nil
}

func (db *killFakeDB) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	switch {
	case strings.Contains(query, "FROM plugin_packages"):
		return &scanFakeRows{rows: db.packages}, nil
	case strings.Contains(query, "FROM plugin_components"):
		id, _ := args[0].(string)
		return &scanFakeRows{rows: db.components[id]}, nil
	}
	return nil, fmt.Errorf("unexpected query: %s", query)
}

func (db *killFakeDB) QueryRow(context.Context, string, ...any) pgx.Row {
	return errorRow{err: pgx.ErrNoRows}
}

func (db *killFakeDB) Begin(context.Context) (pgx.Tx, error) {
	return killFakeTx{db: db}, nil
}

// killFakeTx runs its statements on the pool's fake. Only the calls a kill and an uninstall make are overridden.
type killFakeTx struct {
	pgx.Tx
	db *killFakeDB
}

func (tx killFakeTx) Exec(ctx context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	return tx.db.Exec(ctx, query, args...)
}

func (tx killFakeTx) Query(ctx context.Context, query string, args ...any) (pgx.Rows, error) {
	return tx.db.Query(ctx, query, args...)
}

func (tx killFakeTx) QueryRow(ctx context.Context, query string, args ...any) pgx.Row {
	return tx.db.QueryRow(ctx, query, args...)
}

func (tx killFakeTx) Commit(context.Context) error {
	tx.db.commits++
	return nil
}

func (tx killFakeTx) Rollback(context.Context) error { return nil }

// scanFakeRows answers a read with rows of plain values. Scan takes the destinations the kill reads use.
type scanFakeRows struct {
	rows  [][]any
	index int
}

func (rows *scanFakeRows) Next() bool {
	rows.index++
	return rows.index <= len(rows.rows)
}

func (rows *scanFakeRows) Scan(dest ...any) error {
	values := rows.rows[rows.index-1]
	if len(dest) != len(values) {
		return fmt.Errorf("scan %d columns into %d destinations", len(values), len(dest))
	}
	for i, target := range dest {
		switch out := target.(type) {
		case *string:
			*out = values[i].(string)
		case *bool:
			*out = values[i].(bool)
		case *json.RawMessage:
			*out = values[i].(json.RawMessage)
		default:
			return fmt.Errorf("unsupported scan destination %T", target)
		}
	}
	return nil
}

func (rows *scanFakeRows) Close()                                       {}
func (rows *scanFakeRows) Err() error                                   { return nil }
func (rows *scanFakeRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (rows *scanFakeRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (rows *scanFakeRows) Values() ([]any, error)                       { return nil, nil }
func (rows *scanFakeRows) RawValues() [][]byte                          { return nil }
func (rows *scanFakeRows) Conn() *pgx.Conn                              { return nil }

// skillRemoval is one skill delete and the posture its actor statement set just before it in the same transaction.
type skillRemoval struct {
	id, orgID, posture string
}

// skillRemovals pairs every skill delete with the actor statement that preceded it. An actor statement that is not
// followed by a skill delete fails the test, so no removal can run without its posture set first.
func skillRemovals(t *testing.T, execs []databaseCall) []skillRemoval {
	t.Helper()
	var removals []skillRemoval
	for i, call := range execs {
		if call.query != skillActorSQL {
			continue
		}
		if i+1 >= len(execs) || !strings.HasPrefix(execs[i+1].query, "DELETE FROM agent_skills") {
			t.Fatalf("the actor statement at %d is not followed by a skill delete", i)
		}
		posture, _ := call.args[4].(string)
		id, _ := execs[i+1].args[0].(string)
		orgID, _ := execs[i+1].args[1].(string)
		removals = append(removals, skillRemoval{id: id, orgID: orgID, posture: posture})
	}
	return removals
}

// LEAD-FIX-GLOBALKILL-ZDR, end to end: a global kill of one digest installed in a zero-retention org and in a non-ZDR
// org, run by a platform service principal that is itself explicitly non-ZDR. Both affected orgs' skills are removed
// under the content-free posture, because the platform's own posture describes only the platform's org. The revision
// row itself is written by session-core's 0134 trigger, which is not run here; this test proves the posture each
// removal sets for that trigger to read.
func TestGlobalKillRemovesEachAffectedOrgsSkillContentFree(t *testing.T) {
	platform := authctx.Principal{
		OrganizationID: pluginGlobalOrg, ActorID: "service:platform-kill", PrincipalType: "service",
		Scopes: []string{authz.WriteScope, authz.GlobalWriteScope}, RetentionPolicyPresent: true,
	}
	db := &killFakeDB{
		packages: [][]any{
			{"plugin-zdr", "org-zdr", "research-kit", "1.0.0", json.RawMessage(`{}`), goodArchive, true},
			{"plugin-b", "org-b", "research-kit", "1.0.0", json.RawMessage(`{}`), goodArchive, true},
		},
		components: map[string][][]any{
			"plugin-zdr": {{pluginloader.KindSkill, "triage", "skill-zdr"}},
			"plugin-b":   {{pluginloader.KindSkill, "triage", "skill-b"}},
		},
	}
	handler := NewPluginCatalogHandler(db).WithLoaderEnabled(true).WithChildStores(&SkillsHandler{}, nil)
	body := `{"digest":"` + goodArchive + `","scope":"global","reason":"malicious release"}`
	recorder := servePlugins(handler, platform, http.MethodPost, pluginKillListPath, body)
	if recorder.Code != http.StatusCreated {
		t.Fatalf("global kill status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	var answer struct {
		ChildrenRemoved int `json:"children_removed"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &answer); err != nil {
		t.Fatal(err)
	}
	if answer.ChildrenRemoved != 2 || db.commits != 1 {
		t.Fatalf("children removed = %d, commits = %d; want both orgs' skills removed in one commit", answer.ChildrenRemoved, db.commits)
	}
	removals := skillRemovals(t, db.execs)
	want := []skillRemoval{
		{id: "skill-zdr", orgID: "org-zdr", posture: "true"},
		{id: "skill-b", orgID: "org-b", posture: "true"},
	}
	if !slices.Equal(removals, want) {
		t.Fatalf("skill removals = %#v, want %#v", removals, want)
	}
	for _, call := range db.execs {
		if strings.Contains(call.query, "INSERT INTO plugin_kill_list") && call.args[1] != pluginGlobalOrg {
			t.Fatalf("the kill entry covers %v, want the global org", call.args[1])
		}
	}
}
