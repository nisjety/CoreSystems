//go:build integration

package database

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// WS11-S9 (database tier): run migrations 001-015 into a scratch schema of the
// database named by NOTIFICATION_TEST_DATABASE_URL, then check the seeded
// _default rows, idempotence and the down migration. Run with:
//
//	NOTIFICATION_TEST_DATABASE_URL=postgres://... go test -tags integration ./internal/database -run SpaceAttentionMigrationDB
func TestSpaceAttentionMigrationDB(t *testing.T) {
	url := os.Getenv("NOTIFICATION_TEST_DATABASE_URL")
	if url == "" {
		t.Skip("needs a scratch Postgres: set NOTIFICATION_TEST_DATABASE_URL")
	}
	ctx := context.Background()
	admin, err := pgxpool.New(ctx, url)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	defer admin.Close()
	schema := fmt.Sprintf("t_space_attention_%d", time.Now().UnixNano())
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		t.Fatalf("create schema: %v", err)
	}
	defer admin.Exec(ctx, "DROP SCHEMA "+schema+" CASCADE")

	cfg, err := pgxpool.ParseConfig(url)
	if err != nil {
		t.Fatalf("parse url: %v", err)
	}
	cfg.ConnConfig.RuntimeParams["search_path"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		t.Fatalf("connect scratch schema: %v", err)
	}
	defer pool.Close()

	if err := runMigrations(ctx, poolExecutor{pool: pool}, "../../migrations"); err != nil {
		t.Fatalf("runMigrations: %v", err)
	}
	count := func() (total, inAppOn, emailOn int) {
		row := pool.QueryRow(ctx, `
SELECT count(*),
       count(*) FILTER (WHERE channel = 'in_app' AND enabled),
       count(*) FILTER (WHERE channel = 'email' AND enabled)
FROM notification_channel_configs
WHERE org_id = '_default' AND event_type LIKE 'space.%'`)
		if err := row.Scan(&total, &inAppOn, &emailOn); err != nil {
			t.Fatalf("count: %v", err)
		}
		return
	}
	if total, inApp, email := count(); total != 16 || inApp != 0 || email != 3 {
		t.Fatalf("after up: total=%d in_app_on=%d email_on=%d, want 16/0/3", total, inApp, email)
	}

	up := readSpaceAttentionMigration(t, "015_register_space_attention_notifications.up.sql")
	if _, err := pool.Exec(ctx, up); err != nil {
		t.Fatalf("re-applying up: %v", err)
	}
	if total, _, _ := count(); total != 16 {
		t.Fatalf("after a second up: total=%d, want 16 (idempotent)", total)
	}

	down := readSpaceAttentionMigration(t, "015_register_space_attention_notifications.down.sql")
	if _, err := pool.Exec(ctx, down); err != nil {
		t.Fatalf("down: %v", err)
	}
	if total, _, _ := count(); total != 0 {
		t.Fatalf("after down: total=%d, want 0", total)
	}
}
