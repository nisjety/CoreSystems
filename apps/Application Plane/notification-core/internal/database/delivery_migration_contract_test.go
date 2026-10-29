package database

import (
	"strings"
	"testing"
)

func TestDeliveryAttemptsMigrationIsContentFreeAndLeaseFenced(t *testing.T) {
	migrations, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var sql string
	for _, migration := range migrations {
		if migration.name == "010_delivery_attempts.up.sql" {
			sql = migration.sql
			break
		}
	}
	if sql == "" {
		t.Fatal("delivery attempts migration not found")
	}
	for _, fragment := range []string{
		"notification_delivery_attempts",
		"REFERENCES notification_requests(id) ON DELETE CASCADE",
		"status IN ('pending', 'claimed', 'sent_unconfirmed', 'acknowledged', 'failed', 'unknown')",
		"lease_expires_at",
		"provider_receipt_digest",
		"UNIQUE (notification_id, attempt_number)",
		"notification_delivery_attempts_provider_request_idx",
	} {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration missing %q", fragment)
		}
	}
	for _, forbidden := range []string{"payload JSON", "payload JSONB", "body TEXT", "content TEXT"} {
		if strings.Contains(strings.ToLower(sql), strings.ToLower(forbidden)) {
			t.Fatalf("delivery attempts migration stores content field %q", forbidden)
		}
	}
}

func TestDeliveryCallbackReplayMigrationIsDurableAndExpiring(t *testing.T) {
	migrations, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var sql string
	for _, migration := range migrations {
		if migration.name == "011_delivery_callback_replays.up.sql" {
			sql = migration.sql
			break
		}
	}
	if sql == "" {
		t.Fatal("delivery callback replay migration not found")
	}
	for _, fragment := range []string{
		"notification_delivery_callback_replays",
		"nonce      TEXT PRIMARY KEY",
		"expires_at TIMESTAMPTZ NOT NULL",
		"notification_delivery_callback_replays_expiry_idx",
	} {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration missing %q", fragment)
		}
	}
}

func TestFeedProjectionMigrationIsContentFreeAndRetryable(t *testing.T) {
	migrations, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var sql string
	for _, migration := range migrations {
		if migration.name == "012_feed_projection_attempts.up.sql" {
			sql = migration.sql
			break
		}
	}
	if sql == "" {
		t.Fatal("feed projection migration not found")
	}
	for _, fragment := range []string{
		"notification_feed_projection_attempts",
		"REFERENCES notification_delivery_attempts(id) ON DELETE CASCADE",
		"status IN ('pending', 'claimed', 'projected', 'unknown')",
		"delivery_status IN ('submitted', 'delivered')",
		"lease_expires_at",
		"next_attempt_at",
		"notification_feed_projection_claim_idx",
	} {
		if !strings.Contains(sql, fragment) {
			t.Fatalf("migration missing %q", fragment)
		}
	}
	for _, forbidden := range []string{"payload JSON", "payload JSONB", "body TEXT", "content TEXT"} {
		if strings.Contains(strings.ToLower(sql), strings.ToLower(forbidden)) {
			t.Fatalf("feed projection migration stores content field %q", forbidden)
		}
	}
}
