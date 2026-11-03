package database

import (
	"os"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// WS11-S9: migration 015 registers the eight space.* notification types with
// _default channel configs. These tests pin the contract from the SQL text; the
// database-backed run is in space_attention_migration_integration_test.go
// (build tag "integration").

var spaceAttentionTypes = []string{
	"space.agent_awaiting",
	"space.agent_done",
	"space.agent_failed",
	"space.approval_needed",
	"space.invite",
	"space.mention",
	"space.reminder",
	"space.reply",
}

// Email is on by default for the three signals a person must not miss.
var spaceAttentionEmailOn = map[string]bool{
	"space.mention":         true,
	"space.invite":          true,
	"space.approval_needed": true,
}

var spaceAttentionRow = regexp.MustCompile(`\('_default',\s*'(space\.[a-z_]+)',\s*'(in_app|email)',\s*(TRUE|FALSE),\s*(TRUE|FALSE),`)

func readSpaceAttentionMigration(t *testing.T, name string) string {
	t.Helper()
	raw, err := os.ReadFile("../../migrations/" + name)
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	return string(raw)
}

type seededRow struct {
	eventType, channel string
	enabled, defaultOn bool
}

func parseSpaceAttentionRows(t *testing.T, sql string) []seededRow {
	t.Helper()
	var rows []seededRow
	for _, match := range spaceAttentionRow.FindAllStringSubmatch(sql, -1) {
		rows = append(rows, seededRow{
			eventType: match[1],
			channel:   match[2],
			enabled:   match[3] == "TRUE",
			defaultOn: match[4] == "TRUE",
		})
	}
	return rows
}

func TestSpaceAttentionMigrationUpSeedsSixteenDefaultRows(t *testing.T) {
	sql := readSpaceAttentionMigration(t, "015_register_space_attention_notifications.up.sql")
	rows := parseSpaceAttentionRows(t, sql)
	if len(rows) != 16 {
		t.Fatalf("seeded rows = %d, want 16 (8 types x in_app + email)", len(rows))
	}
	types := map[string]int{}
	for _, row := range rows {
		types[row.eventType]++
	}
	var got []string
	for eventType, count := range types {
		if count != 2 {
			t.Fatalf("%s has %d channel rows, want 2", eventType, count)
		}
		got = append(got, eventType)
	}
	sort.Strings(got)
	if strings.Join(got, ",") != strings.Join(spaceAttentionTypes, ",") {
		t.Fatalf("seeded types = %v, want %v", got, spaceAttentionTypes)
	}
}

func TestSpaceAttentionMigrationInAppIsOffForEveryTypeAndEmailOnlyForTheThreeMustSeeSignals(t *testing.T) {
	sql := readSpaceAttentionMigration(t, "015_register_space_attention_notifications.up.sql")
	for _, row := range parseSpaceAttentionRows(t, sql) {
		switch row.channel {
		case "in_app":
			// Convex's inbox is the in-app copy; enabling this would duplicate it.
			if row.enabled || row.defaultOn {
				t.Fatalf("%s in_app enabled=%v default=%v, want both false", row.eventType, row.enabled, row.defaultOn)
			}
		case "email":
			want := spaceAttentionEmailOn[row.eventType]
			if row.enabled != want || row.defaultOn != want {
				t.Fatalf("%s email enabled=%v default=%v, want both %v", row.eventType, row.enabled, row.defaultOn, want)
			}
		}
	}
}

func TestSpaceAttentionMigrationUpIsIdempotentAndOnlyTouchesDefaultConfigs(t *testing.T) {
	sql := readSpaceAttentionMigration(t, "015_register_space_attention_notifications.up.sql")
	if !strings.Contains(sql, "ON CONFLICT (org_id, event_type, channel) DO NOTHING") {
		t.Fatal("migration is not idempotent: missing ON CONFLICT (org_id, event_type, channel) DO NOTHING")
	}
	// The conflict target must be the real primary key of the config table.
	create := readSpaceAttentionMigration(t, "005_create_channel_configs.up.sql")
	if !strings.Contains(create, "PRIMARY KEY (org_id, event_type, channel)") {
		t.Fatal("notification_channel_configs primary key is no longer (org_id, event_type, channel)")
	}
	if got := strings.Count(sql, "INSERT INTO"); got != 1 {
		t.Fatalf("INSERT statements = %d, want 1", got)
	}
	for _, forbidden := range []string{"UPDATE ", "DELETE ", "DROP ", "ALTER ", "TRUNCATE"} {
		if strings.Contains(sql, forbidden) {
			t.Fatalf("up migration contains %q", forbidden)
		}
	}
	if strings.Contains(strings.ToLower(sql), "subject") || strings.Contains(strings.ToLower(sql), "body") {
		t.Fatal("the up migration should carry no content columns")
	}
}

func TestSpaceAttentionMigrationDownRemovesExactlyTheSeededRows(t *testing.T) {
	sql := readSpaceAttentionMigration(t, "015_register_space_attention_notifications.down.sql")
	if !strings.Contains(sql, "DELETE FROM notification_channel_configs") || !strings.Contains(sql, "org_id = '_default'") {
		t.Fatalf("down migration must delete _default rows from notification_channel_configs: %s", sql)
	}
	for _, eventType := range spaceAttentionTypes {
		if !strings.Contains(sql, "'"+eventType+"'") {
			t.Fatalf("down migration does not remove %s", eventType)
		}
	}
	quoted := regexp.MustCompile(`'(space\.[a-z_]+)'`).FindAllStringSubmatch(sql, -1)
	if len(quoted) != len(spaceAttentionTypes) {
		t.Fatalf("down migration names %d event types, want %d", len(quoted), len(spaceAttentionTypes))
	}
}

func TestSpaceAttentionMigrationIsPickedUpAsAnUpMigrationAndTheDownIsNeverRun(t *testing.T) {
	files, err := migrationFileNames("../../migrations")
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}
	var up, down bool
	for _, file := range files {
		if file.name == "015_register_space_attention_notifications.up.sql" {
			up = true
		}
		if strings.HasSuffix(file.name, ".down.sql") {
			down = true
		}
	}
	if !up {
		t.Fatal("migration 015 up is not in the migration list")
	}
	if down {
		t.Fatal("a down migration is in the forward migration list")
	}
}
