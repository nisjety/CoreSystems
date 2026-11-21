package notification

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// createTablePattern finds the table each up migration creates.
var createTablePattern = regexp.MustCompile(`(?i)CREATE TABLE(?:\s+IF NOT EXISTS)?\s+([a-z_][a-z0-9_]*)`)

// purgeTargetPattern finds the table a purge statement deletes from or updates.
var purgeTargetPattern = regexp.MustCompile(`(?i)(?:DELETE FROM|UPDATE)\s+([a-z_][a-z0-9_]*)`)

// migrationTables returns every table the up migrations in dir create, sorted and without repeats.
func migrationTables(t *testing.T, dir string) []string {
	t.Helper()
	files, err := filepath.Glob(filepath.Join(dir, "*.up.sql"))
	if err != nil {
		t.Fatalf("list migrations: %v", err)
	}
	seen := make(map[string]bool)
	for _, file := range files {
		content, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("read %s: %v", file, err)
		}
		for _, match := range createTablePattern.FindAllStringSubmatch(string(content), -1) {
			seen[strings.ToLower(match[1])] = true
		}
	}
	tables := make([]string, 0, len(seen))
	for table := range seen {
		tables = append(tables, table)
	}
	sort.Strings(tables)
	return tables
}

// purgedTables returns the set of tables the organization purge steps delete from or update.
func purgedTables(t *testing.T) map[string]bool {
	t.Helper()
	targets := make(map[string]bool)
	for _, step := range organizationPurgeSteps {
		match := purgeTargetPattern.FindStringSubmatch(step.statement)
		if match == nil {
			t.Fatalf("purge step %s names no table in its statement: %s", step.table, step.statement)
		}
		targets[strings.ToLower(match[1])] = true
	}
	return targets
}

// The organization erasure must reach every table the migrations create, or name the table as exempt with the reason it is
// not erased by organization. A table a later migration adds fails this test until its erasure is written.
func TestOrganizationPurgeCoversEveryMigrationTable(t *testing.T) {
	tables := migrationTables(t, "../../migrations")
	if len(tables) == 0 {
		t.Fatal("the migrations create no tables")
	}
	purged := purgedTables(t)
	for _, table := range tables {
		if reason, exempt := organizationPurgeExempt[table]; exempt {
			if strings.TrimSpace(reason) == "" {
				t.Fatalf("%s is exempt from the organization erasure without a reason", table)
			}
			if purged[table] {
				t.Fatalf("%s is exempt from the organization erasure, yet a purge step deletes from it", table)
			}
			continue
		}
		if !purged[table] {
			t.Fatalf("the migrations create %s, but no organization purge step removes its rows: add a step, or exempt it with a reason", table)
		}
	}
	for table := range organizationPurgeExempt {
		if idx := sort.SearchStrings(tables, table); idx >= len(tables) || tables[idx] != table {
			t.Fatalf("exempt table %s is not created by any migration", table)
		}
	}
}

// Every statement is scoped to the one organization it is given. A statement without that predicate could widen into a purge
// of every organization, and a second parameter would be one the caller never supplies.
func TestOrganizationPurgeStepsAreScopedToOneOrganization(t *testing.T) {
	for _, step := range organizationPurgeSteps {
		if !strings.Contains(step.statement, "organization_id = $1") && !strings.Contains(step.statement, "org_id = $1") {
			t.Fatalf("%s is not scoped to the organization: %s", step.table, step.statement)
		}
		if strings.Contains(step.statement, "$2") {
			t.Fatalf("%s takes a parameter beyond the organization id: %s", step.table, step.statement)
		}
	}
}

// Children go before parents, and the subscriber identity steps read the memberships, so they run before the memberships
// are removed.
func TestOrganizationPurgeOrdersChildrenBeforeParentsAndIdentityBeforeMemberships(t *testing.T) {
	position := make(map[string]int, len(organizationPurgeSteps))
	for index, step := range organizationPurgeSteps {
		position[step.table] = index
	}
	ordered := [][2]string{
		{"notification_feed_projection_attempts", "notification_delivery_attempts"},
		{"notification_feed_projection_attempts", "notification_requests"},
		{"notification_delivery_attempts", "notification_requests"},
		{"notification_subscribers", "notification_subscribers_detached"},
		{"notification_subscribers", "notification_subscriber_memberships"},
		{"notification_subscribers_detached", "notification_subscriber_memberships"},
	}
	for _, pair := range ordered {
		before, okBefore := position[pair[0]]
		after, okAfter := position[pair[1]]
		if !okBefore || !okAfter {
			t.Fatalf("purge steps %s or %s are missing", pair[0], pair[1])
		}
		if before >= after {
			t.Fatalf("%s runs at %d, after %s at %d: the order must be %s first", pair[0], before, pair[1], after, pair[0])
		}
	}
}

// Web Push subscriptions are removed by the same purge, in the same transaction, so an erased organization leaves no sealed
// endpoint behind even when web push is off. They are never exempt.
func TestOrganizationPurgeRemovesWebPushSubscriptionsInTheSameTransaction(t *testing.T) {
	if !purgedTables(t)["notification_web_push_subscriptions"] {
		t.Fatal("no purge step removes the Web Push subscriptions")
	}
	if _, exempt := organizationPurgeExempt["notification_web_push_subscriptions"]; exempt {
		t.Fatal("Web Push subscriptions are exempt, so a sealed endpoint would survive the erasure")
	}
}
