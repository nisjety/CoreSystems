package database

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestTicketOperationReservationMigrationPreventsVisibleUncommittedEffects(t *testing.T) {
	contents, err := os.ReadFile(filepath.Join("migrations", "031_ticket_operation_reservations.sql"))
	if err != nil {
		t.Fatal(err)
	}
	sql := strings.ToLower(string(contents))
	for _, required := range []string{
		"drop not null",
		"pending_control_commit",
		"control_reservation_id",
		"action_schema_hash",
		"payload_digest",
		"grant_ref",
		"ticket_id is not null",
		"audit_event_id is not null",
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
