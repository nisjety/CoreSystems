package subscribers

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
)

// This test is opt-in. It writes and deletes membership rows, so it needs a disposable database and the explicit mutation
// acknowledgement the other notification-core Postgres proofs use.
func TestListMembershipsForOrganizationAgainstRealPostgres(t *testing.T) {
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

	const doomedOrg = "erasure-list-proof-org-doomed"
	const keptOrg = "erasure-list-proof-org-kept"
	repository := NewRepository(db.Pool)
	reset := func() {
		if _, err := db.Pool.Exec(context.Background(),
			`DELETE FROM notification_subscriber_memberships WHERE organization_id IN ($1, $2)`, doomedOrg, keptOrg); err != nil {
			t.Fatalf("reset memberships: %v", err)
		}
	}
	reset()
	defer reset()

	occurredAt := time.Date(2026, time.November, 20, 9, 0, 0, 0, time.UTC)
	seed := func(organizationID, userID, status string) {
		if _, err := repository.UpsertMembership(ctx, MembershipParams{
			OrganizationID:       organizationID,
			UserID:               userID,
			ProviderSubscriberID: providerSubscriberID(organizationID, userID),
			Role:                 "member",
			Status:               status,
			OccurredAt:           occurredAt,
		}); err != nil {
			t.Fatalf("seed membership %s/%s: %v", organizationID, userID, err)
		}
	}
	seed(doomedOrg, "user-1", MembershipStatusActive)
	seed(doomedOrg, "user-2", MembershipStatusRemoved)
	seed(keptOrg, "user-1", MembershipStatusActive)

	rows, err := repository.ListMembershipsForOrganization(ctx, doomedOrg)
	if err != nil {
		t.Fatalf("ListMembershipsForOrganization() error = %v", err)
	}
	if len(rows) != 2 {
		t.Fatalf("ListMembershipsForOrganization() returned %d rows, want the two memberships of the doomed organization", len(rows))
	}
	statuses := map[string]bool{}
	for _, row := range rows {
		if row.OrganizationID != doomedOrg {
			t.Fatalf("row for organization %q leaked into the listing of %q", row.OrganizationID, doomedOrg)
		}
		if row.ProviderSubscriberID != providerSubscriberID(doomedOrg, row.UserID) {
			t.Fatalf("row for user %q carries provider id %q, want the doomed organization's own", row.UserID, row.ProviderSubscriberID)
		}
		statuses[row.Status] = true
	}
	if !statuses[MembershipStatusActive] || !statuses[MembershipStatusRemoved] {
		t.Fatalf("statuses = %v, want both the active and the removed membership", statuses)
	}
}

// This test is opt-in, as the organization listing proof is. It lists one user's memberships across organizations, active and
// removed, and leaves another user's rows out.
func TestListMembershipsForUserAgainstRealPostgres(t *testing.T) {
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

	const user = "erasure-user-proof-user"
	const other = "erasure-user-proof-other"
	const orgA = "erasure-user-proof-org-a"
	const orgB = "erasure-user-proof-org-b"
	repository := NewRepository(db.Pool)
	reset := func() {
		if _, err := db.Pool.Exec(context.Background(),
			`DELETE FROM notification_subscriber_memberships WHERE user_id IN ($1, $2)`, user, other); err != nil {
			t.Fatalf("reset memberships: %v", err)
		}
	}
	reset()
	defer reset()

	occurredAt := time.Date(2026, time.November, 20, 9, 0, 0, 0, time.UTC)
	seed := func(organizationID, userID, status string) {
		if _, err := repository.UpsertMembership(ctx, MembershipParams{
			OrganizationID:       organizationID,
			UserID:               userID,
			ProviderSubscriberID: providerSubscriberID(organizationID, userID),
			Role:                 "member",
			Status:               status,
			OccurredAt:           occurredAt,
		}); err != nil {
			t.Fatalf("seed membership %s/%s: %v", organizationID, userID, err)
		}
	}
	seed(orgA, user, MembershipStatusActive)
	seed(orgB, user, MembershipStatusRemoved)
	seed(orgA, other, MembershipStatusActive)

	rows, err := repository.ListMembershipsForUser(ctx, user)
	if err != nil {
		t.Fatalf("ListMembershipsForUser() error = %v", err)
	}
	if len(rows) != 2 {
		t.Fatalf("ListMembershipsForUser() returned %d rows, want the two memberships of the user", len(rows))
	}
	statuses := map[string]bool{}
	for _, row := range rows {
		if row.UserID != user {
			t.Fatalf("row for user %q leaked into the listing of %q", row.UserID, user)
		}
		if row.ProviderSubscriberID != providerSubscriberID(row.OrganizationID, user) {
			t.Fatalf("row in organization %q carries provider id %q, want that membership's own", row.OrganizationID, row.ProviderSubscriberID)
		}
		statuses[row.Status] = true
	}
	if !statuses[MembershipStatusActive] || !statuses[MembershipStatusRemoved] {
		t.Fatalf("statuses = %v, want both the active and the removed membership", statuses)
	}
}
