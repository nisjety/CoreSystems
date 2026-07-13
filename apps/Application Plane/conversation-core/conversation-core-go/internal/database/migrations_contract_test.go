package database

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestOutboundIntentMigrationIsContentFreeAndConservative(t *testing.T) {
	path := filepath.Join("migrations", "004_outbound_intents.sql")
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToLower(string(contents))
	for _, required := range []string{
		"unique (org_id, idempotency_key)",
		"'sending', 'submitted', 'failed', 'unknown'",
		"where ai_action_id <> ''",
		"alter column status set default 'sending'",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	for _, forbidden := range []string{"body_text", "body_html", "payload json", "message_content"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("content-free outbound ledger contains forbidden column %q", forbidden)
		}
	}
}

func TestOutboundAuthorizationBindingMigrationIsContentFree(t *testing.T) {
	contents, err := os.ReadFile(filepath.Join("migrations", "005_outbound_authorization_binding.sql"))
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToLower(string(contents))
	for _, required := range []string{
		"authorization_kind", "actor_user_id", "approval_id", "action_id",
		"operation", "payload_sha256", "'retryable'",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	for _, forbidden := range []string{"body_text", "body_html", "payload json", "message_content"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("authorization binding stores content via %q", forbidden)
		}
	}
}
