package database

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestBrowserSiteChoicesBackfillErasesOnlyNonActiveMembers pins P2-cp-04-rel1: migration 035
// deletes the choices and redacts the user_pref history of every member who is not active, and
// it is written so migration 034's append-only trigger admits each redaction it makes. The
// Postgres-backed test in internal/browsersites runs the statements over legacy rows; this one
// fails fast without a database.
func TestBrowserSiteChoicesBackfillErasesOnlyNonActiveMembers(t *testing.T) {
	body, err := os.ReadFile(filepath.Join("..", "..", "migrations", "035_browser_site_choices_backfill.up.sql"))
	if err != nil {
		t.Fatalf("read browser site choices backfill: %v", err)
	}
	text := string(body)
	for _, required := range []string{
		"DELETE FROM user_browser_site_prefs p",
		"WHERE NOT EXISTS (",
		"m.org_id = p.org_id AND m.user_id = p.user_id AND m.status = 'active'",
		"UPDATE org_browser_site_history h",
		"h.change_kind = 'user_pref'",
		"h.subject_user_id <> 'erased'",
		"SET subject_user_id = 'erased', actor_id = 'erased', before_set = '{}'::jsonb, after_set = '{}'::jsonb",
		"m.org_id = h.org_id AND m.user_id = h.subject_user_id AND m.status = 'active'",
	} {
		if !strings.Contains(text, required) {
			t.Fatalf("browser site choices backfill is missing %q", required)
		}
	}
	if strings.Contains(text, "change_kind = 'policy'") {
		t.Fatal("browser site choices backfill must not redact policy rows: their actor is an administrator, not a member's choice")
	}

	// The redaction must be the one transition migration 034's trigger admits: the trigger names
	// the same tombstone and the same empty sets, so a change to either side fails here first.
	trigger, err := os.ReadFile(filepath.Join("..", "..", "migrations", "034_browser_site_history_erasure.up.sql"))
	if err != nil {
		t.Fatalf("read browser site history erasure trigger: %v", err)
	}
	for _, required := range []string{
		"NEW.subject_user_id = 'erased'",
		"NEW.actor_id = 'erased'",
		"NEW.before_set = '{}'::jsonb",
		"NEW.after_set = '{}'::jsonb",
	} {
		if !strings.Contains(string(trigger), required) {
			t.Fatalf("migration 034 trigger no longer admits the backfill redaction: missing %q", required)
		}
	}
}
