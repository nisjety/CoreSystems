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

func TestOutboundDeliveryReceiptMigrationKeepsSubmissionAndDeliverySeparate(t *testing.T) {
	contents, err := os.ReadFile(filepath.Join("migrations", "011_outbound_delivery_receipts.sql"))
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToLower(string(contents))
	for _, required := range []string{
		"provider_delivery_status", "provider_delivery_occurred_at", "provider_delivery_error_code",
		"'unconfirmed', 'delivered', 'read', 'failed'", "provider_message_id <> ''",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	for _, forbidden := range []string{"body_text", "body_html", "provider_thread_id"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("delivery receipt migration stores forbidden content/routing field %q", forbidden)
		}
	}
}

func TestCSATOutcomeMigrationIsScoreOnlyAndTenantScoped(t *testing.T) {
	contents, err := os.ReadFile(filepath.Join("migrations", "023_ticket_csat_outcomes.sql"))
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToLower(string(contents))
	for _, required := range []string{
		"ticket_csat_outcomes", "org_id text not null", "score smallint not null check (score between 1 and 5)",
		"primary key (org_id, ticket_id)", "recorded_by", "recorded_at",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	for _, forbidden := range []string{"body_text", "body_html", "email", "phone", "survey_token", "response_comment"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("CSAT outcome migration stores prohibited content or contact field %q", forbidden)
		}
	}
}
