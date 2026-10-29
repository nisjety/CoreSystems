package emailsync

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
	integrationdb "github.com/triodelab/integration-corev2/internal/db"
	"github.com/triodelab/integration-corev2/internal/store"
)

type postgresTeamsHistoryStore struct {
	*store.PostgresRepository
	connection store.Connection
}

func (s postgresTeamsHistoryStore) ListConnections(_ context.Context, filter store.ConnectionFilter) ([]store.Connection, error) {
	if filter.ProviderKey == "microsoft" {
		return []store.Connection{s.connection}, nil
	}
	return nil, nil
}

func TestTeamsHistoryExtensionPersistsThroughPostgresAndStaleWorkerSave(t *testing.T) {
	databaseURL := strings.TrimSpace(os.Getenv("INTEGRATION_TEST_DATABASE_URL"))
	if databaseURL == "" {
		t.Skip("INTEGRATION_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
	defer cancel()
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		t.Fatalf("open disposable postgres: %v", err)
	}
	defer pool.Close()
	var databaseName string
	if err := pool.QueryRow(ctx, `SELECT current_database()`).Scan(&databaseName); err != nil {
		t.Fatalf("read database name: %v", err)
	}
	if normalized := strings.ToLower(databaseName); !strings.Contains(normalized, "test") && !strings.Contains(normalized, "tmp") {
		t.Fatalf("refusing history test against non-disposable database %q", databaseName)
	}
	if err := integrationdb.ApplyMigrations(ctx, pool); err != nil {
		t.Fatalf("apply migrations: %v", err)
	}

	connectionID := "teams-history-test-" + uuid.NewString()
	stateKey := connectionID + ":teams"
	defer func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM email_sync_state WHERE connection_id = $1`, stateKey)
	}()
	repository := store.NewPostgresRepository(pool)
	if _, err := repository.ExtendEmailSyncHistory(ctx, stateKey, "teams", 30, 3650); err != nil {
		t.Fatalf("queue 60-day total history: %v", err)
	}

	graph := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"), strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			http.NotFound(w, r)
		}
	}))
	defer graph.Close()
	connection := store.Connection{
		ID: connectionID, ProviderKey: "microsoft", OrganizationID: "org-test", UserID: "user-test",
		Status: "active", Capabilities: []string{"teams.messages.read"},
	}
	historyStore := postgresTeamsHistoryStore{PostgresRepository: repository, connection: connection}
	worker := Worker{
		Store: historyStore, Tokens: fakeTokens{}, Ingest: &fakeIngestor{},
		Teams: &TeamsFetcher{BaseURL: graph.URL, HTTP: graph.Client()},
	}

	if _, err := worker.RunOnce(ctx); err != nil {
		t.Fatalf("first 60-day worker run: %v", err)
	}
	first, err := repository.GetEmailSyncState(ctx, stateKey)
	if err != nil {
		t.Fatalf("read first cursor: %v", err)
	}
	firstCursor := decodeTeamsCursor(decodeFetcherCursor(worker.Teams, first.Cursor), 60*24*time.Hour)
	if first.HistoryBackfillDays != 30 || firstCursor.HistoryDays != 60 {
		t.Fatalf("first durable state = additional %d, cursor days %d; want 30/60", first.HistoryBackfillDays, firstCursor.HistoryDays)
	}
	firstBootstrap := firstCursor.Bootstrap

	if _, err := worker.RunOnce(ctx); err != nil {
		t.Fatalf("resume 60-day worker run: %v", err)
	}
	resumed, err := repository.GetEmailSyncState(ctx, stateKey)
	if err != nil {
		t.Fatalf("read resumed cursor: %v", err)
	}
	resumedCursor := decodeTeamsCursor(decodeFetcherCursor(worker.Teams, resumed.Cursor), 60*24*time.Hour)
	if resumedCursor.Bootstrap != firstBootstrap {
		t.Fatalf("60-day cursor reset twice: %q -> %q", firstBootstrap, resumedCursor.Bootstrap)
	}

	staleWorkerState := resumed
	if _, err := repository.ExtendEmailSyncHistory(ctx, stateKey, "teams", 30, 3650); err != nil {
		t.Fatalf("queue 90-day total history: %v", err)
	}
	if err := repository.UpsertEmailSyncState(ctx, staleWorkerState); err != nil {
		t.Fatalf("persist stale worker state: %v", err)
	}
	preserved, err := repository.GetEmailSyncState(ctx, stateKey)
	if err != nil {
		t.Fatalf("read concurrently preserved request: %v", err)
	}
	if preserved.HistoryBackfillDays != 60 {
		t.Fatalf("stale worker erased extension: additional days = %d, want 60", preserved.HistoryBackfillDays)
	}

	if _, err := worker.RunOnce(ctx); err != nil {
		t.Fatalf("90-day worker run: %v", err)
	}
	finalState, err := repository.GetEmailSyncState(ctx, stateKey)
	if err != nil {
		t.Fatalf("read final cursor: %v", err)
	}
	finalCursor := decodeTeamsCursor(decodeFetcherCursor(worker.Teams, finalState.Cursor), 90*24*time.Hour)
	if finalState.HistoryBackfillDays != 60 || finalCursor.HistoryDays != 90 {
		t.Fatalf("final durable state = additional %d, cursor days %d; want 60/90", finalState.HistoryBackfillDays, finalCursor.HistoryDays)
	}
}
