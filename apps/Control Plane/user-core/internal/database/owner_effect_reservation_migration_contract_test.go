package database

import (
	"os"
	"strings"
	"testing"
)

func TestOwnerEffectReservationMigrationIsContentFreeAndRevocationFenced(t *testing.T) {
	contents, err := os.ReadFile("../../migrations/025_owner_effect_reservations.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToLower(string(contents))
	for _, required := range []string{
		"space_owner_effect_reservations",
		"operation_id text primary key",
		"action_schema_hash",
		"payload_digest",
		"grant_ref",
		"status text not null check (status in ('reserved', 'committed', 'cancelled'))",
		"space_authority_revisions",
		"cancel_pending_owner_effect_reservations",
	} {
		if !strings.Contains(sql, required) {
			t.Fatalf("migration missing %q", required)
		}
	}
	for _, forbidden := range []string{
		"control_decision_token", "body_text", "body_html", "payload json", "message_content", "service_token",
	} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("migration stores prohibited bearer or content field %q", forbidden)
		}
	}
}
