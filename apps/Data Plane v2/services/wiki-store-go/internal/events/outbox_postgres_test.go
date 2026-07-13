package events

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"
)

func TestPostgresOutboxLeaseRetryAndDeliveryLifecycle(t *testing.T) {
	databaseURL := os.Getenv("WIKI_TEST_DATABASE_URL")
	if databaseURL == "" {
		t.Skip("set WIKI_TEST_DATABASE_URL to a disposable local *_test database")
	}
	config, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatal(err)
	}
	if config.ConnConfig.Host != "localhost" && config.ConnConfig.Host != "127.0.0.1" && config.ConnConfig.Host != "::1" {
		t.Fatalf("refusing non-local test database host %q", config.ConnConfig.Host)
	}
	if !strings.HasSuffix(config.ConnConfig.Database, "_test") {
		t.Fatalf("refusing non-test database %q", config.ConnConfig.Database)
	}

	ctx := context.Background()
	admin, err := pgxpool.NewWithConfig(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	defer admin.Close()
	schema := "wiki_delivery_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _, _ = admin.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE") })

	isolatedConfig, _ := pgxpool.ParseConfig(databaseURL)
	isolatedConfig.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, isolatedConfig)
	if err != nil {
		t.Fatal(err)
	}
	defer pool.Close()
	migrationPath := filepath.Join("..", "..", "..", "..", "infra", "postgres", "migrations", "20260711180000_wiki_event_outbox.sql")
	migration, err := os.ReadFile(migrationPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := pool.Exec(ctx, string(migration)); err != nil {
		t.Fatal(err)
	}
	payload := `{"page_id":"page","version_id":"version","org_id":"org","workspace_id":"workspace","title":"title","path":"/title","content":"content","zdr":false}`
	if _, err := pool.Exec(ctx, `INSERT INTO wiki_event_outbox
		(org_id, event_type, payload, idempotency_key) VALUES ($1, $2, $3::jsonb, $4)`,
		"org", SubjectWikiPublished, payload, "wiki.published:version"); err != nil {
		t.Fatal(err)
	}

	store := &postgresOutboxStore{pool: pool}
	first, err := store.Claim(ctx, "worker-a", 10, 30*time.Second)
	if err != nil || len(first) != 1 || first[0].Attempts != 1 {
		t.Fatalf("first claim = %+v, %v", first, err)
	}
	second, err := store.Claim(ctx, "worker-b", 10, 30*time.Second)
	if err != nil || len(second) != 0 {
		t.Fatalf("SKIP LOCKED lease isolation = %+v, %v", second, err)
	}
	if err := store.Retry(ctx, first[0].ID, "worker-a", 0, "retry"); err != nil {
		t.Fatal(err)
	}
	reclaimed, err := store.Claim(ctx, "worker-b", 10, 30*time.Second)
	if err != nil || len(reclaimed) != 1 || reclaimed[0].Attempts != 2 {
		t.Fatalf("reclaim = %+v, %v", reclaimed, err)
	}
	if err := store.MarkDelivered(ctx, reclaimed[0].ID, "worker-b"); err != nil {
		t.Fatal(err)
	}
	var status string
	var delivered bool
	if err := pool.QueryRow(ctx, `SELECT status, delivered_at IS NOT NULL
		FROM wiki_event_outbox WHERE outbox_id = $1`, reclaimed[0].ID).Scan(&status, &delivered); err != nil {
		t.Fatal(err)
	}
	if status != "delivered" || !delivered {
		t.Fatalf("final status=%q delivered=%v", status, delivered)
	}
}
