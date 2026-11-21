package notification

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/subscribers"
)

// This test is opt-in. It writes and deletes rows in every organization-keyed table, so it needs a disposable database
// and the explicit mutation acknowledgement the other notification-core Postgres proofs use.
func TestOrganizationPurgeAgainstRealPostgres(t *testing.T) {
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

	const doomedOrg = "purge-proof-org-doomed"
	const keptOrg = "purge-proof-org-kept"
	// sharedUser is a member of both organizations, and its identity cache names the doomed one.
	const sharedUser = "purge-proof-shared-user"
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

	// clearProofRows removes every row this proof writes, children before parents. Every id and user id it uses carries the
	// prefix, so no row of a real database matches, and a rerun against the same database starts clean.
	clearProofRows := func() {
		for _, statement := range []string{
			`DELETE FROM notification_feed_projection_attempts WHERE id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_delivery_attempts WHERE id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_requests WHERE id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_feed_items WHERE id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_preferences WHERE user_id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_channel_configs WHERE org_id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_web_push_subscriptions WHERE organization_id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_subscriber_memberships WHERE user_id LIKE 'purge-proof-%' OR organization_id LIKE 'purge-proof-%'`,
			`DELETE FROM notification_subscribers WHERE user_id LIKE 'purge-proof-%'`,
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
	// tenant. The organization check now rejects such a row, and it admits only the rows that already existed, so the check is
	// lifted for this one insert and restored as NOT VALID, as the migration left it.
	insertQuarantined := func(table, constraint, statement string) {
		t.Helper()
		exec(`ALTER TABLE ` + table + ` DROP CONSTRAINT ` + constraint)
		defer exec(`ALTER TABLE ` + table + ` ADD CONSTRAINT ` + constraint +
			` CHECK (organization_id IS NOT NULL AND btrim(organization_id) <> '') NOT VALID`)
		exec(statement)
	}
	// seedOrganization writes one row into every organization-keyed table for one organization, and one Web Push subscription.
	// The ids carry the prefix, so a surviving row can be traced to its organization.
	seedOrganization := func(organizationID, prefix string) {
		requestID := prefix + "-request"
		userID := prefix + "-user"
		exec(`INSERT INTO notification_requests (id, organization_id, recipient_kind, recipient_id, type, status, request_sha256, retention_mode)
VALUES ($1, $2, 'user', $3, 'modelplane.needs_approval', 'submitted', repeat('a', 64), 'standard')`, requestID, organizationID, userID)
		exec(`INSERT INTO notification_delivery_attempts (id, notification_id, attempt_number) VALUES ($1, $2, 1)`, prefix+"-attempt", requestID)
		exec(`INSERT INTO notification_feed_projection_attempts (id, attempt_id, notification_id) VALUES ($1, $2, $3)`, prefix+"-projection", prefix+"-attempt", requestID)
		exec(`INSERT INTO notification_feed_items (id, organization_id, recipient_id, event_type, channel) VALUES ($1, $2, $3, 'modelplane.needs_approval', 'in_app')`, requestID, organizationID, userID)
		exec(`INSERT INTO notification_preferences (organization_id, user_id, event_type, channel) VALUES ($1, $2, 'modelplane.needs_approval', 'in_app')`, organizationID, userID)
		exec(`INSERT INTO notification_channel_configs (org_id, event_type, channel) VALUES ($1, 'modelplane.needs_approval', 'in_app')`, organizationID)
		exec(`INSERT INTO notification_subscriber_memberships (organization_id, user_id, provider_subscriber_id, status, occurred_at) VALUES ($1, $2, $3, 'active', now())`, organizationID, userID, prefix+"-provider")
		exec(`INSERT INTO notification_subscribers (user_id, novu_subscriber_id, org_id) VALUES ($1, $2, $3)`, userID, prefix+"-novu", organizationID)
		insertSubscription(organizationID, userID, prefix+"-push", strings.Repeat("d", 64), false)
	}

	// The catalogue rows are the org-wide defaults the migrations seed. No erasure may remove them.
	defaultsBefore := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = '_default'`)
	if defaultsBefore == 0 {
		t.Fatal("the migrations seeded no _default channel configs to protect")
	}

	seedOrganization(doomedOrg, "purge-proof-a")
	seedOrganization(keptOrg, "purge-proof-b")
	// A revoked subscription of the doomed organization goes with the rest: erasure removes the sealed row, not the revocation.
	insertSubscription(doomedOrg, "purge-proof-a-user", "purge-proof-a-revoked", strings.Repeat("e", 64), true)

	// The shared user has an active membership in each organization, and its identity cache names the doomed one. The purge
	// must keep the identity for the kept organization, detached from the doomed one.
	exec(`INSERT INTO notification_subscriber_memberships (organization_id, user_id, provider_subscriber_id, status, occurred_at) VALUES ($1, $2, $3, 'active', now())`, doomedOrg, sharedUser, "purge-proof-shared-doomed-provider")
	exec(`INSERT INTO notification_subscriber_memberships (organization_id, user_id, provider_subscriber_id, status, occurred_at) VALUES ($1, $2, $3, 'active', now())`, keptOrg, sharedUser, "purge-proof-shared-kept-provider")
	exec(`INSERT INTO notification_subscribers (user_id, novu_subscriber_id, org_id, role) VALUES ($1, $2, $3, 'owner')`, sharedUser, "purge-proof-shared-novu", doomedOrg)

	// An identity with no organization cache whose only membership is in the doomed organization is erased with it: no other
	// organization holds it, and the membership's status does not change that.
	exec(`INSERT INTO notification_subscribers (user_id, novu_subscriber_id) VALUES ($1, $2)`, "purge-proof-n-user", "purge-proof-n-novu")
	exec(`INSERT INTO notification_subscriber_memberships (organization_id, user_id, provider_subscriber_id, status, occurred_at) VALUES ($1, $2, $3, 'removed', now())`, doomedOrg, "purge-proof-n-user", "purge-proof-n-provider")

	// Quarantined rows: no organization, and no membership that names one. The purge must leave every one of them alone.
	insertQuarantined("notification_requests", "notification_requests_organization_required", `INSERT INTO notification_requests (id, organization_id, recipient_kind, recipient_id, type, status, request_sha256, retention_mode)
VALUES ('purge-proof-q-request', NULL, 'user', 'purge-proof-q-user', 'modelplane.needs_approval', 'submitted', repeat('a', 64), 'standard')`)
	insertQuarantined("notification_feed_items", "notification_feed_items_organization_required", `INSERT INTO notification_feed_items (id, organization_id, recipient_id, event_type, channel)
VALUES ('purge-proof-q-feed', NULL, 'purge-proof-q-user', 'modelplane.needs_approval', 'in_app')`)
	insertQuarantined("notification_preferences", "notification_preferences_organization_required", `INSERT INTO notification_preferences (organization_id, user_id, event_type, channel)
VALUES (NULL, 'purge-proof-q-user', 'modelplane.needs_approval', 'in_app')`)
	exec(`INSERT INTO notification_subscribers (user_id, novu_subscriber_id) VALUES ($1, $2)`, "purge-proof-q-user", "purge-proof-q-novu")

	removed, err := repository.PurgeOrganization(ctx, doomedOrg)
	if err != nil {
		t.Fatalf("PurgeOrganization(doomed) error = %v", err)
	}
	// Every tenant table is named, with its rows removed. The single-organization user's identity and the no-cache identity
	// are deleted; the shared user's identity is detached, not deleted. Memberships count the three doomed ones.
	wantRemoved := map[string]int64{
		"notification_feed_projection_attempts": 1,
		"notification_delivery_attempts":        1,
		"notification_requests":                 1,
		"notification_feed_items":               1,
		"notification_preferences":              1,
		"notification_channel_configs":          1,
		"notification_web_push_subscriptions":   2,
		"notification_subscribers":              2,
		"notification_subscribers_detached":     1,
		"notification_subscriber_memberships":   3,
	}
	if len(removed) != len(wantRemoved) {
		t.Fatalf("removed tables = %v, want every organization-keyed table", removed)
	}
	for table, want := range wantRemoved {
		if removed[table] != want {
			t.Fatalf("removed[%s] = %d, want %d", table, removed[table], want)
		}
	}

	// The doomed organization leaves no row in any organization-keyed table, Web Push endpoints and keys included.
	for _, probe := range []struct {
		name string
		sql  string
	}{
		{"requests", `SELECT count(*) FROM notification_requests WHERE organization_id = $1`},
		{"feed items", `SELECT count(*) FROM notification_feed_items WHERE organization_id = $1`},
		{"preferences", `SELECT count(*) FROM notification_preferences WHERE organization_id = $1`},
		{"channel configs", `SELECT count(*) FROM notification_channel_configs WHERE org_id = $1`},
		{"memberships", `SELECT count(*) FROM notification_subscriber_memberships WHERE organization_id = $1`},
		{"subscribers", `SELECT count(*) FROM notification_subscribers WHERE org_id = $1`},
		{"web push subscriptions", `SELECT count(*) FROM notification_web_push_subscriptions WHERE organization_id = $1`},
	} {
		if n := count(probe.sql, doomedOrg); n != 0 {
			t.Fatalf("%s left %d rows for the erased organization", probe.name, n)
		}
	}
	// Its request's attempts went with the request: the children are removed before the parent, and none survive.
	if n := count(`SELECT count(*) FROM notification_delivery_attempts WHERE notification_id = $1`, "purge-proof-a-request"); n != 0 {
		t.Fatalf("delivery attempts of the erased request = %d, want 0", n)
	}
	if n := count(`SELECT count(*) FROM notification_feed_projection_attempts WHERE notification_id = $1`, "purge-proof-a-request"); n != 0 {
		t.Fatalf("projection attempts of the erased request = %d, want 0", n)
	}

	// The kept organization is untouched, row for row. Its memberships include the shared user's kept membership, which the
	// purge of the other organization does not touch.
	for _, probe := range []struct {
		name string
		sql  string
		want int64
	}{
		{"requests", `SELECT count(*) FROM notification_requests WHERE organization_id = $1`, 1},
		{"feed items", `SELECT count(*) FROM notification_feed_items WHERE organization_id = $1`, 1},
		{"preferences", `SELECT count(*) FROM notification_preferences WHERE organization_id = $1`, 1},
		{"channel configs", `SELECT count(*) FROM notification_channel_configs WHERE org_id = $1`, 1},
		{"memberships", `SELECT count(*) FROM notification_subscriber_memberships WHERE organization_id = $1`, 2},
		{"subscribers", `SELECT count(*) FROM notification_subscribers WHERE org_id = $1`, 1},
		{"web push subscriptions", `SELECT count(*) FROM notification_web_push_subscriptions WHERE organization_id = $1`, 1},
	} {
		if n := count(probe.sql, keptOrg); n != probe.want {
			t.Fatalf("%s of the kept organization = %d, want %d", probe.name, n, probe.want)
		}
	}
	if n := count(`SELECT count(*) FROM notification_delivery_attempts WHERE notification_id = 'purge-proof-b-request'`); n != 1 {
		t.Fatalf("kept organization's delivery attempt = %d rows, want 1", n)
	}
	if n := count(`SELECT count(*) FROM notification_feed_projection_attempts WHERE notification_id = 'purge-proof-b-request'`); n != 1 {
		t.Fatalf("kept organization's projection attempt = %d rows, want 1", n)
	}

	// The shared user keeps its identity, detached from the erased organization with its role cleared. It still resolves in
	// the kept organization through its active membership there.
	if n := count(`SELECT count(*) FROM notification_subscribers WHERE user_id = $1 AND org_id IS NULL AND role = ''`, sharedUser); n != 1 {
		t.Fatalf("shared user's detached identity rows = %d, want 1", n)
	}
	if n := count(`SELECT count(*) FROM notification_subscriber_memberships WHERE organization_id = $1 AND user_id = $2 AND status = 'active'`, keptOrg, sharedUser); n != 1 {
		t.Fatalf("shared user's kept membership rows = %d, want 1", n)
	}
	resolved, err := subscribers.NewRepository(db.Pool).GetActiveForOrganization(ctx, keptOrg, sharedUser)
	if err != nil {
		t.Fatalf("kept organization no longer resolves the shared user: %v", err)
	}
	if resolved.OrgID != keptOrg {
		t.Fatalf("shared user resolved in org %q, want the kept organization", resolved.OrgID)
	}
	// The identities that belonged only to the erased organization are gone, with or without a cache.
	for _, userID := range []string{"purge-proof-a-user", "purge-proof-n-user"} {
		if n := count(`SELECT count(*) FROM notification_subscribers WHERE user_id = $1`, userID); n != 0 {
			t.Fatalf("identity of %s = %d rows after the erasure, want 0", userID, n)
		}
	}
	// The kept organization's identity and the quarantined identity, which no membership ties to any organization, survive.
	for _, userID := range []string{"purge-proof-b-user", "purge-proof-q-user"} {
		if n := count(`SELECT count(*) FROM notification_subscribers WHERE user_id = $1`, userID); n != 1 {
			t.Fatalf("identity of %s = %d rows after the erasure, want 1", userID, n)
		}
	}

	// Quarantined rows with no organization are untouched.
	for _, probe := range []struct {
		name string
		sql  string
	}{
		{"request", `SELECT count(*) FROM notification_requests WHERE id = 'purge-proof-q-request' AND organization_id IS NULL`},
		{"feed item", `SELECT count(*) FROM notification_feed_items WHERE id = 'purge-proof-q-feed' AND organization_id IS NULL`},
		{"preference", `SELECT count(*) FROM notification_preferences WHERE user_id = 'purge-proof-q-user' AND organization_id IS NULL`},
	} {
		if n := count(probe.sql); n != 1 {
			t.Fatalf("quarantined %s rows = %d after the erasure, want 1", probe.name, n)
		}
	}

	if defaultsAfter := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = '_default'`); defaultsAfter != defaultsBefore {
		t.Fatalf("_default channel configs = %d after the purge, want %d", defaultsAfter, defaultsBefore)
	}

	// A replayed erasure is a no-op: every table reports zero.
	replayed, err := repository.PurgeOrganization(ctx, doomedOrg)
	if err != nil {
		t.Fatalf("replayed PurgeOrganization() error = %v", err)
	}
	if len(replayed) != len(wantRemoved) {
		t.Fatalf("replayed purge reports %d tables, want %d", len(replayed), len(wantRemoved))
	}
	for table, n := range replayed {
		if n != 0 {
			t.Fatalf("replayed purge removed %d rows from %s, want zero", n, table)
		}
	}

	// The catalogue and an empty organization are refused, and the catalogue survives the attempt.
	for _, organizationID := range []string{"", "_default"} {
		if _, err := repository.PurgeOrganization(ctx, organizationID); err == nil {
			t.Fatalf("PurgeOrganization(%q) succeeded, want a refusal", organizationID)
		}
	}
	if defaultsAfter := count(`SELECT count(*) FROM notification_channel_configs WHERE org_id = '_default'`); defaultsAfter != defaultsBefore {
		t.Fatalf("refused purge changed _default channel configs to %d, want %d", defaultsAfter, defaultsBefore)
	}
}
