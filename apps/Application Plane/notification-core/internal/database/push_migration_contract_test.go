package database

import (
	"os"
	"regexp"
	"strings"
	"testing"
)

func TestWebPushMigrationStoresOnlySealedSubscriptionFields(t *testing.T) {
	migrations, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var sql string
	for _, migration := range migrations {
		if migration.name == "016_web_push_subscriptions.up.sql" {
			sql = migration.sql
			break
		}
	}
	if sql == "" {
		t.Fatal("web push subscriptions migration not found")
	}
	for _, fragment := range []string{
		"notification_web_push_subscriptions",
		"endpoint_ct     BYTEA",
		"p256dh_ct       BYTEA",
		"auth_ct         BYTEA",
		"endpoint_hash   TEXT",
		"UNIQUE (organization_id, user_id, endpoint_hash)",
		"revoked_at      TIMESTAMPTZ",
	} {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration missing %q", fragment)
		}
	}
	plaintextColumn := regexp.MustCompile(`(?m)^\s*(endpoint|p256dh|auth)\s+`)
	if plaintextColumn.MatchString(sql) {
		t.Fatal("web push migration declares a plaintext endpoint or key column")
	}
	if strings.Contains(strings.ToLower(sql), "payload") {
		t.Fatal("web push migration stores a payload column; a subscription must hold no content")
	}
}

func TestWebPushDownMigrationDropsTheTable(t *testing.T) {
	down, err := os.ReadFile("../../migrations/016_web_push_subscriptions.down.sql")
	if err != nil {
		t.Fatalf("read down migration: %v", err)
	}
	if !strings.Contains(string(down), "DROP TABLE IF EXISTS notification_web_push_subscriptions") {
		t.Fatal("down migration does not drop the subscriptions table")
	}
}
