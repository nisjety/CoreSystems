package notification

import (
	"sort"
	"strings"
	"testing"
)

// userPurgedTables returns the set of tables the user purge steps delete from.
func userPurgedTables(t *testing.T) map[string]bool {
	t.Helper()
	targets := make(map[string]bool)
	for _, step := range userPurgeSteps {
		match := purgeTargetPattern.FindStringSubmatch(step.statement)
		if match == nil {
			t.Fatalf("user purge step %s names no table in its statement: %s", step.table, step.statement)
		}
		targets[strings.ToLower(match[1])] = true
	}
	return targets
}

// The user erasure must reach every table the migrations create, or name the table as exempt with the reason it is not
// erased by user. A table a later migration adds fails this test until its erasure is written.
func TestUserPurgeCoversEveryMigrationTable(t *testing.T) {
	tables := migrationTables(t, "../../migrations")
	if len(tables) == 0 {
		t.Fatal("the migrations create no tables")
	}
	purged := userPurgedTables(t)
	for _, table := range tables {
		if reason, exempt := userPurgeExempt[table]; exempt {
			if strings.TrimSpace(reason) == "" {
				t.Fatalf("%s is exempt from the user erasure without a reason", table)
			}
			if purged[table] {
				t.Fatalf("%s is exempt from the user erasure, yet a purge step deletes from it", table)
			}
			continue
		}
		if !purged[table] {
			t.Fatalf("the migrations create %s, but no user purge step removes its rows: add a step, or exempt it with a reason", table)
		}
	}
	for table := range userPurgeExempt {
		if idx := sort.SearchStrings(tables, table); idx >= len(tables) || tables[idx] != table {
			t.Fatalf("exempt table %s is not created by any migration", table)
		}
	}
}

// Every user statement is scoped to the one user it is given, and it takes no parameter beyond that user id. It must not narrow
// to one organization either: a user's erasure reaches every organization the user belongs to.
func TestUserPurgeStepsAreScopedToOneUserInEveryOrganization(t *testing.T) {
	for _, step := range userPurgeSteps {
		if !strings.Contains(step.statement, "user_id = $1") && !strings.Contains(step.statement, "recipient_id = $1") {
			t.Fatalf("%s is not scoped to the user: %s", step.table, step.statement)
		}
		if strings.Contains(step.statement, "$2") {
			t.Fatalf("%s takes a parameter beyond the user id: %s", step.table, step.statement)
		}
		if strings.Contains(step.statement, "organization_id") || strings.Contains(step.statement, "org_id") {
			t.Fatalf("%s narrows the user erasure to one organization: %s", step.table, step.statement)
		}
	}
}

// The attempts reference the requests, so they are removed before the requests that name the user.
func TestUserPurgeOrdersChildrenBeforeParents(t *testing.T) {
	position := make(map[string]int, len(userPurgeSteps))
	for index, step := range userPurgeSteps {
		position[step.table] = index
	}
	ordered := [][2]string{
		{"notification_feed_projection_attempts", "notification_delivery_attempts"},
		{"notification_feed_projection_attempts", "notification_requests"},
		{"notification_delivery_attempts", "notification_requests"},
	}
	for _, pair := range ordered {
		before, okBefore := position[pair[0]]
		after, okAfter := position[pair[1]]
		if !okBefore || !okAfter {
			t.Fatalf("user purge steps %s or %s are missing", pair[0], pair[1])
		}
		if before >= after {
			t.Fatalf("%s runs at %d, after %s at %d: the order must be %s first", pair[0], before, pair[1], after, pair[0])
		}
	}
}

// Web Push subscriptions are removed by the user erasure in the same transaction, so a user's sealed endpoints survive nowhere.
// They are never exempt.
func TestUserPurgeRemovesWebPushSubscriptions(t *testing.T) {
	if !userPurgedTables(t)["notification_web_push_subscriptions"] {
		t.Fatal("no user purge step removes the Web Push subscriptions")
	}
	if _, exempt := userPurgeExempt["notification_web_push_subscriptions"]; exempt {
		t.Fatal("Web Push subscriptions are exempt, so a sealed endpoint would survive the user erasure")
	}
}

// The subscriber identity and every membership of the user are removed, and neither is exempt.
func TestUserPurgeRemovesTheIdentityAndTheMemberships(t *testing.T) {
	purged := userPurgedTables(t)
	for _, table := range []string{"notification_subscribers", "notification_subscriber_memberships", "notification_preferences", "notification_feed_items"} {
		if !purged[table] {
			t.Fatalf("no user purge step removes %s", table)
		}
		if _, exempt := userPurgeExempt[table]; exempt {
			t.Fatalf("%s is exempt from the user erasure, so the user's rows would survive it", table)
		}
	}
}
