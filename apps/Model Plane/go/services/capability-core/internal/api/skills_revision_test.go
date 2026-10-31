package api

import (
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

// recordingTx is the transaction recordingDatabase hands out: it sends every
// statement to the same recorder, except the one that names the actor, which it
// keeps in actorCalls.
type recordingTx struct {
	pgx.Tx
	database *recordingDatabase
}

func (tx recordingTx) Exec(ctx context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	if query == skillActorSQL {
		tx.database.actorCalls = append(tx.database.actorCalls, databaseCall{query: query, args: append([]any(nil), args...)})
		return pgconn.NewCommandTag("SELECT 1"), nil
	}
	return tx.database.Exec(ctx, query, args...)
}

func (tx recordingTx) Query(ctx context.Context, query string, args ...any) (pgx.Rows, error) {
	return tx.database.Query(ctx, query, args...)
}

func (tx recordingTx) QueryRow(ctx context.Context, query string, args ...any) pgx.Row {
	return tx.database.QueryRow(ctx, query, args...)
}

func (tx recordingTx) Commit(context.Context) error {
	tx.database.commits++
	return nil
}

func (tx recordingTx) Rollback(context.Context) error { return nil }

func (database *recordingDatabase) Begin(context.Context) (pgx.Tx, error) {
	return recordingTx{database: database}, nil
}

// noTransactionDatabase is a registry database without Begin.
type noTransactionDatabase struct{ inner *recordingDatabase }

func (database noTransactionDatabase) Exec(ctx context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	return database.inner.Exec(ctx, query, args...)
}

func (database noTransactionDatabase) Query(ctx context.Context, query string, args ...any) (pgx.Rows, error) {
	return database.inner.Query(ctx, query, args...)
}

func (database noTransactionDatabase) QueryRow(ctx context.Context, query string, args ...any) pgx.Row {
	return database.inner.QueryRow(ctx, query, args...)
}

func serveSkill(t *testing.T, pool registryDatabase, token func(sign mcpTokenSigner) string, method, target, body string) *httptest.ResponseRecorder {
	t.Helper()
	skills := NewSkillsHandler(pool)
	mux := http.NewServeMux()
	skills.Register(mux)
	handler, sign := mcpAuthenticatedHandlerWithSigner(t, mux)
	request := httptest.NewRequest(method, target, strings.NewReader(body))
	request.Header.Set("Authorization", "Bearer "+token(sign))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func writeToken(sign mcpTokenSigner) string {
	nonZDR := false
	return sign("user", []string{authz.WriteScope}, &nonZDR)
}

func adminToken(sign mcpTokenSigner) string {
	nonZDR := false
	return sign("user", []string{authz.WriteScope, "admin"}, &nonZDR)
}

// A skill change from the registry names its caller in the transaction that
// makes it, so the revision the trigger writes carries the verified actor and
// not whatever the body claims.
func TestPatchWritesRevisionWithActor(t *testing.T) {
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPatch, "/api/v1/skills/sk-1",
		`{"content":"better steps","actor_id":"someone-else"}`)
	if response.Code != http.StatusOK {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.actorCalls) != 1 {
		t.Fatalf("actor statements = %d, want one before the update", len(database.actorCalls))
	}
	args := database.actorCalls[0].args
	if len(args) != 4 || args[0] != "user" || args[1] != "user-a" || args[2] != "" || args[3] != "" {
		t.Fatalf("actor args = %#v, want the verified user and no decision", args)
	}
	if len(database.execs) != 1 || !strings.Contains(database.execs[0].query, "UPDATE agent_skills") {
		t.Fatalf("the update must run once, inside the transaction: %#v", database.execs)
	}
	if database.commits != 1 {
		t.Fatalf("commits = %d, want 1", database.commits)
	}
}

func TestDeleteWritesRevisionWithActor(t *testing.T) {
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodDelete, "/api/v1/skills/sk-1", "")
	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.actorCalls) != 1 || database.actorCalls[0].args[1] != "user-a" || database.commits != 1 {
		t.Fatalf("a delete must name its actor and commit once: %#v commits=%d", database.actorCalls, database.commits)
	}
}

func TestCreateWritesRevisionWithActor(t *testing.T) {
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"mine","content":"body","scope":"user"}`)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	if len(database.actorCalls) != 1 || database.actorCalls[0].args[3] != "" {
		t.Fatalf("a plain create names its actor and no decision: %#v", database.actorCalls)
	}
}

// Promotion is recorded as one: the copy's first revision is 'promoted' and its
// reason names the source.
func TestPromotionIsRecordedAsPromoted(t *testing.T) {
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: true})
	response := serveSkill(t, database, adminToken, http.MethodPost, "/api/v1/skills",
		`{"name":"shared runbook","content":"body","scope":"org","promoted_from":"sk-user-1"}`)
	if response.Code != http.StatusCreated {
		t.Fatalf("status = %d, body=%s", response.Code, response.Body.String())
	}
	args := database.actorCalls[0].args
	if args[3] != "promoted" || args[2] != "promoted_from:sk-user-1" {
		t.Fatalf("actor args = %#v, want the promoted action and its source", args)
	}
	source := database.queryRows[0]
	if !strings.Contains(source.query, "review_state = 'accepted'") || !strings.Contains(source.query, "scope = 'user'") {
		t.Fatalf("the source must be an accepted user skill:\n%s", source.query)
	}
}

func TestPromotionOfAnInvisibleSourceCreatesNothing(t *testing.T) {
	database := &recordingDatabase{}
	database.nextRows = append(database.nextRows, boolRow{value: false})
	response := serveSkill(t, database, adminToken, http.MethodPost, "/api/v1/skills",
		`{"name":"x","content":"body","scope":"org","promoted_from":"sk-private"}`)
	if response.Code != http.StatusNotFound {
		t.Fatalf("status = %d, body=%s, want 404", response.Code, response.Body.String())
	}
	if len(database.execs) != 0 {
		t.Fatalf("nothing may be inserted for a source the admin cannot see: %#v", database.execs)
	}
}

func TestPromotionMustCreateAnOrgSkill(t *testing.T) {
	database := &recordingDatabase{}
	response := serveSkill(t, database, writeToken, http.MethodPost, "/api/v1/skills",
		`{"name":"x","content":"body","scope":"user","promoted_from":"sk-1"}`)
	if response.Code != http.StatusUnprocessableEntity || database.databaseCalls() != 0 {
		t.Fatalf("status = %d, calls = %d, want 422 before any statement", response.Code, database.databaseCalls())
	}
}

// A database that cannot open a transaction cannot mutate a skill: running the
// change outside one would leave a revision with no actor.
func TestSkillMutationWithoutATransactionIsRefused(t *testing.T) {
	inner := &recordingDatabase{}
	response := serveSkill(t, noTransactionDatabase{inner: inner}, writeToken, http.MethodDelete, "/api/v1/skills/sk-1", "")
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status = %d, body=%s, want 500", response.Code, response.Body.String())
	}
	if inner.databaseCalls() != 0 {
		t.Fatalf("no statement may run outside a transaction: %#v", inner.execs)
	}
}

// The settings the registry sets are the ones the trigger reads. A rename on
// either side would turn every revision into an unattributed one.
func TestSkillActorSettingsMatchTheMigration(t *testing.T) {
	migration, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "rust", "services", "session-core", "migrations", "0064_skill_revisions.sql"))
	if err != nil {
		t.Skipf("the session-core migration is not beside this module: %v", err)
	}
	for _, setting := range []string{"mp.actor_kind", "mp.actor_id", "mp.run_id", "mp.thread_id", "mp.reason", "mp.skill_action"} {
		if !strings.Contains(skillActorSQL, "set_config('"+setting+"'") {
			t.Errorf("skillActorSQL does not set %s", setting)
		}
		if !strings.Contains(string(migration), "current_setting('"+setting+"', true)") {
			t.Errorf("the trigger does not read %s", setting)
		}
	}
}

type boolRow struct{ value bool }

func (row boolRow) Scan(dest ...any) error {
	*(dest[0].(*bool)) = row.value
	return nil
}
