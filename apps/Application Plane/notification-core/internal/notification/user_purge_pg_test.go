package notification

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
)

// This test is opt-in. It writes and deletes rows in every user-keyed table, so it needs a disposable database and the explicit
// mutation acknowledgement the other notification-core Postgres proofs use.
func TestUserPurgeAgainstRealPostgres(t *testing.T) {
	databaseURL := strings.TrimSpace(os.Getenv("NOTIFICATION_CORE_TEST_DATABASE_URL"))
	if databaseURL == "" || os.Getenv("NOTIFICATION_CORE_ALLOW_DB_MUTATION") != "1" {
		t.Skip("set NOTIFICATION_CORE_TEST_DATABASE_URL and NOTIFICATION_CORE_ALLOW_DB_MUTATION=1 for disposable Postgres proof")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, databaseURL)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatal(err)
	}

	const doomedUser = "user-erasure-proof-doomed"
	const keptUser = "user-erasure-proof-kept"
	const orgA = "user-erasure-proof-org-a"
	const orgB = "user-erasure-proof-org-b"
	repository := NewRepository(db.Pool)

	exec := func(sql string, args ...any) {
		t.Helper()
		if _, err := db.Pool.Exec(ctx, sql, args...); err != nil {
			t.Fatalf("exec %q: %v", sql, err)
		}
	}
	count := func(sql string, args ...any) int64 {
		t.Helper()
		var n int64
		if err := db.Pool.QueryRow(ctx, sql, args...).Scan(&n); err != nil {
			t.Fatalf("count %q: %v", sql, err)
		}
		return n
	}

	// clearProofRows removes every row this proof writes, children before parents. Every id, user id and organization id it
	// uses carries the prefix, so no row of a real database matches, and a rerun against the same database starts clean.
	clearProofRows := func() {
		for _, statement := range []string{
			`DELETE FROM notification_feed_projection_attempts WHERE id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_delivery_attempts WHERE id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_requests WHERE id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_feed_items WHERE id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_preferences WHERE user_id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_channel_configs WHERE org_id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_web_push_subscriptions WHERE user_id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_subscriber_memberships WHERE user_id LIKE 'user-erasure-proof-%'`,
			`DELETE FROM notification_subscribers WHERE user_id LIKE 'user-erasure-proof-%'`,
		} {
			exec(statement)
		}
	}
	clearProofRows()
	defer clearProofRows()

	// insertSubscription writes one Web Push row with stand-in sealed bytes. The purge never opens a row, so the bytes only
	// need to be recognisable to this proof.
	insertSubscription := func(organizationID, userID, id, endpointHash string, revoked bool) {
		t.Helper()
		var revokedAt any
		if revoked {
			revokedAt = time.Now().UTC()
		}
		exec(`INSERT INTO notification_web_push_subscriptions (id, organization_id, user_id, endpoint_hash, endpoint_ct, p256dh_ct, auth_ct, revoked_at)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)`, id, organizationID, userID, endpointHash,
			[]byte("sealed-endpoint-"+id), []byte("sealed-p256dh-"+id), []byte("sealed-auth-"+id), revokedAt)
	}
	// insertQuarantined writes one row with no organization, the shape migration 007 leaves on rows it could not assign to a
	// tenant. The organization check rejects such a row, so the check is lifted for this one insert and restored as NOT VALID.
	insertQuarantined := func(table, constraint, statement string) {
		t.Helper()
		exec(`ALTER TABLE ` + table + ` DROP CONSTRAINT ` + constraint)
		defer exec(`ALTER TABLE ` + table + ` ADD CONSTRAINT ` + constraint +
			` CHECK (organization_id IS NOT NULL AND btrim(organization_id) <> '') NOT VALID`)
		exec(statement)
	}
	// seedUser writes one row into every user-keyed table for one user in one organization, and one active Web Push
	// subscription. The ids carry the prefix, so a surviving row can be traced to its user and organization.
	seedUser := func(organizationID, userID, prefix string) {
		t.Helper()
		requestID := prefix + "-request"
		exec(`INSERT INTO notification_requests (id, organization_id, recipient_kind, recipient_id, type, status, request_sha256, retention_mode)
VALUES ($1, $2, 'user', $3, 'modelplane.needs_approval', 'submitted', repeat('a', 64), 'standard')`, requestID, organizationID, userID)
		exec(`INSERT INTO notification_delivery_attempts (id, notification_id, attempt_number) VALUES ($1, $2, 1)`, prefix+"-attempt", requestID)
		exec(`INSERT INTO notification_feed_projection_attempts (id, attempt_id, notification_id) VALUES ($1, $2, $3)`, prefix+"-projection", prefix+"-attempt", requestID)
		exec(`INSERT INTO notification_feed_items (id, organization_id, recipient_id, event_type, channel) VALUES ($1, $2, $3, 'modelplane.needs_approval', 'in_app')`, requestID+"-feed", organizationID, userID)
		exec(`INSERT INTO notification_preferences (organization_id, user_id, event_type, channel) VALUES ($1, $2, 'modelplane.needs_approval', 'in_app')`, organizationID, userID)
		exec(`INSERT INTO notification_subscriber_memberships (organization_id, user_id, provider_subscriber_id, status, occurred_at) VALUES ($1, $2, $3, 'active', now())`, organizationID, userID, prefix+"-provider")
		insertSubscription(organizationID, userID, prefix+"-push", strings.Repeat("d", 64), false)
	}

	// The catalogue rows are the org-wide defaults the migrations seed. No user erasure may remove them.
	defaultsBefore := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = '_default'`)
	if defaultsBefore == 0 {
		t.Fatal("the migrations seeded no _default channel configs to protect")
	}
	// The organization policy names no user, so it survives the erasure of a user who is a member of the organization.
	exec(`INSERT INTO notification_channel_configs (org_id, event_type, channel) VALUES ($1, 'modelplane.needs_approval', 'in_app')`, orgA)

	// The doomed user belongs to two organizations. Each holds a request, its attempts, a feed item, a preference, a membership
	// and an active Web Push subscription. A revoked subscription in the first organization goes too, and the identity names the
	// first organization as its cache.
	seedUser(orgA, doomedUser, "user-erasure-proof-a")
	seedUser(orgB, doomedUser, "user-erasure-proof-b")
	insertSubscription(orgA, doomedUser, "user-erasure-proof-a-revoked", strings.Repeat("e", 64), true)
	exec(`INSERT INTO notification_subscribers (user_id, novu_subscriber_id, org_id, role) VALUES ($1, $2, $3, 'owner')`, doomedUser, "user-erasure-proof-novu", orgA)

	// Quarantined rows of the doomed user: no organization, and the user as recipient. They are the user's own data, so the
	// user erasure removes them, where the organization erasure would leave them.
	insertQuarantined("notification_requests", "notification_requests_organization_required", `INSERT INTO notification_requests (id, organization_id, recipient_kind, recipient_id, type, status, request_sha256, retention_mode)
VALUES ('user-erasure-proof-q-request', NULL, 'user', 'user-erasure-proof-doomed', 'modelplane.needs_approval', 'submitted', repeat('a', 64), 'standard')`)
	insertQuarantined("notification_feed_items", "notification_feed_items_organization_required", `INSERT INTO notification_feed_items (id, organization_id, recipient_id, event_type, channel)
VALUES ('user-erasure-proof-q-feed', NULL, 'user-erasure-proof-doomed', 'modelplane.needs_approval', 'in_app')`)
	insertQuarantined("notification_preferences", "notification_preferences_organization_required", `INSERT INTO notification_preferences (organization_id, user_id, event_type, channel)
VALUES (NULL, 'user-erasure-proof-doomed', 'modelplane.needs_approval', 'in_app')`)

	// The kept user is a member of the first organization only, and no row of it may be removed.
	seedUser(orgA, keptUser, "user-erasure-proof-k")
	exec(`INSERT INTO notification_subscribers (user_id, novu_subscriber_id, org_id) VALUES ($1, $2, $3)`, keptUser, "user-erasure-proof-kept-novu", orgA)

	removed, err := repository.PurgeUser(ctx, doomedUser)
	if err != nil {
		t.Fatalf("PurgeUser(doomed) error = %v", err)
	}
	// Every user-keyed table the erasure removes from is named, with the rows it removed. The organization policy and the
	// callback nonces are exempt and do not appear.
	wantRemoved := map[string]int64{
		"notification_feed_projection_attempts": 2,
		"notification_delivery_attempts":        2,
		"notification_requests":                 3,
		"notification_feed_items":               3,
		"notification_preferences":              3,
		"notification_web_push_subscriptions":   3,
		"notification_subscriber_memberships":   2,
		"notification_subscribers":              1,
	}
	if len(removed) != len(wantRemoved) {
		t.Fatalf("removed tables = %v, want every user-keyed table", removed)
	}
	for table, want := range wantRemoved {
		if removed[table] != want {
			t.Fatalf("removed[%s] = %d, want %d", table, removed[table], want)
		}
	}

	// The doomed user leaves no row in any user-keyed table, in any organization, quarantined rows and sealed endpoints included.
	for _, probe := range []struct {
		name string
		sql  string
	}{
		{"requests", `SELECT count(*) FROM notification_requests WHERE recipient_id = $1`},
		{"feed items", `SELECT count(*) FROM notification_feed_items WHERE recipient_id = $1`},
		{"preferences", `SELECT count(*) FROM notification_preferences WHERE user_id = $1`},
		{"memberships", `SELECT count(*) FROM notification_subscriber_memberships WHERE user_id = $1`},
		{"subscribers", `SELECT count(*) FROM notification_subscribers WHERE user_id = $1`},
		{"web push subscriptions", `SELECT count(*) FROM notification_web_push_subscriptions WHERE user_id = $1`},
	} {
		if n := count(probe.sql, doomedUser); n != 0 {
			t.Fatalf("%s left %d rows for the erased user", probe.name, n)
		}
	}
	// Its requests' attempts went with the requests: the children are removed before the parents, and none survive.
	for _, requestID := range []string{"user-erasure-proof-a-request", "user-erasure-proof-b-request"} {
		if n := count(`SELECT count(*) FROM notification_delivery_attempts WHERE notification_id = $1`, requestID); n != 0 {
			t.Fatalf("delivery attempts of %s = %d, want 0", requestID, n)
		}
		if n := count(`SELECT count(*) FROM notification_feed_projection_attempts WHERE notification_id = $1`, requestID); n != 0 {
			t.Fatalf("projection attempts of %s = %d, want 0", requestID, n)
		}
	}

	// The kept user keeps every row it had, in the same organization.
	for _, probe := range []struct {
		name string
		sql  string
	}{
		{"requests", `SELECT count(*) FROM notification_requests WHERE recipient_id = $1`},
		{"feed items", `SELECT count(*) FROM notification_feed_items WHERE recipient_id = $1`},
		{"preferences", `SELECT count(*) FROM notification_preferences WHERE user_id = $1`},
		{"memberships", `SELECT count(*) FROM notification_subscriber_memberships WHERE user_id = $1`},
		{"subscribers", `SELECT count(*) FROM notification_subscribers WHERE user_id = $1`},
		{"web push subscriptions", `SELECT count(*) FROM notification_web_push_subscriptions WHERE user_id = $1`},
	} {
		if n := count(probe.sql, keptUser); n != 1 {
			t.Fatalf("%s of the kept user = %d, want 1", probe.name, n)
		}
	}
	if n := count(`SELECT count(*) FROM notification_delivery_attempts WHERE notification_id = 'user-erasure-proof-k-request'`); n != 1 {
		t.Fatalf("kept user's delivery attempt = %d rows, want 1", n)
	}
	if n := count(`SELECT count(*) FROM notification_feed_projection_attempts WHERE notification_id = 'user-erasure-proof-k-request'`); n != 1 {
		t.Fatalf("kept user's projection attempt = %d rows, want 1", n)
	}

	// The organization policy and the catalogue are untouched by a user erasure.
	if n := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = $1`, orgA); n != 1 {
		t.Fatalf("organization policy of %s = %d rows after the user erasure, want 1", orgA, n)
	}
	if defaultsAfter := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = '_default'`); defaultsAfter != defaultsBefore {
		t.Fatalf("_default channel configs = %d after the user purge, want %d", defaultsAfter, defaultsBefore)
	}

	// A replayed erasure is a no-op: every table reports zero.
	replayed, err := repository.PurgeUser(ctx, doomedUser)
	if err != nil {
		t.Fatalf("replayed PurgeUser() error = %v", err)
	}
	if len(replayed) != len(wantRemoved) {
		t.Fatalf("replayed purge reports %d tables, want %d", len(replayed), len(wantRemoved))
	}
	for table, n := range replayed {
		if n != 0 {
			t.Fatalf("replayed purge removed %d rows from %s, want zero", n, table)
		}
	}

	// An empty user and the catalogue are refused, and the catalogue survives the attempt.
	for _, userID := range []string{"", "_default"} {
		if _, err := repository.PurgeUser(ctx, userID); err == nil {
			t.Fatalf("PurgeUser(%q) succeeded, want a refusal", userID)
		}
	}
	if defaultsAfter := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = '_default'`); defaultsAfter != defaultsBefore {
		t.Fatalf("refused purge changed _default channel configs to %d, want %d", defaultsAfter, defaultsBefore)
	}
}
