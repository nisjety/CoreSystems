package webpush

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
)

// This test is opt-in. It writes rows, so it needs a disposable database and the explicit mutation
// acknowledgement the other notification-core Postgres proofs use.
func TestWebPushSubscriptionsAgainstRealPostgres(t *testing.T) {
	databaseURL := strings.TrimSpace(os.Getenv("NOTIFICATION_CORE_TEST_DATABASE_URL"))
	if databaseURL == "" || os.Getenv("NOTIFICATION_CORE_ALLOW_DB_MUTATION") != "1" {
		t.Skip("set NOTIFICATION_CORE_TEST_DATABASE_URL and NOTIFICATION_CORE_ALLOW_DB_MUTATION=1 for disposable Postgres proof")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, databaseURL)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatal(err)
	}

	key := make([]byte, 32)
	if _, err := rand.Read(key); err != nil {
		t.Fatal(err)
	}
	store, err := NewStore(db.Pool, hex.EncodeToString(key))
	if err != nil {
		t.Fatal(err)
	}
	const organizationID = "webpush-proof-org"
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `DELETE FROM notification_web_push_subscriptions WHERE organization_id = $1`, organizationID)
	}()

	browser := newBrowserStandIn(t)
	endpoint := "https://fcm.googleapis.com/fcm/send/webpush-proof"
	first, err := store.Upsert(ctx, organizationID, "webpush-proof-user", Subscription{Endpoint: endpoint, P256DH: browser.p256dh, Auth: browser.auth}, "google")
	if err != nil {
		t.Fatalf("Upsert() error = %v", err)
	}
	again, err := store.Upsert(ctx, organizationID, "webpush-proof-user", Subscription{Endpoint: endpoint, P256DH: browser.p256dh, Auth: browser.auth}, "google")
	if err != nil {
		t.Fatalf("second Upsert() error = %v", err)
	}
	if again != first {
		t.Fatalf("re-subscribing the same endpoint returned %q, want the stored id %q", again, first)
	}

	active, err := store.ListActive(ctx, organizationID, "webpush-proof-user", 10)
	if err != nil {
		t.Fatalf("ListActive() error = %v", err)
	}
	if len(active) != 1 || active[0].Endpoint != endpoint || active[0].Auth != browser.auth {
		t.Fatalf("ListActive() = %+v, want the one endpoint opened intact", active)
	}

	if err := store.Revoke(ctx, organizationID, "webpush-proof-other-user", first); !errors.Is(err, ErrSubscriptionNotFound) {
		t.Fatalf("Revoke() by another user error = %v, want ErrSubscriptionNotFound", err)
	}
	if err := store.Revoke(ctx, organizationID, "webpush-proof-user", first); err != nil {
		t.Fatalf("Revoke() by the owner error = %v", err)
	}
	if active, _ := store.ListActive(ctx, organizationID, "webpush-proof-user", 10); len(active) != 0 {
		t.Fatalf("revoked subscription still listed: %+v", active)
	}
	if _, err := store.Upsert(ctx, organizationID, "webpush-proof-user", Subscription{Endpoint: endpoint, P256DH: browser.p256dh, Auth: browser.auth}, "google"); err != nil {
		t.Fatalf("re-subscribe after revoke error = %v", err)
	}
	if active, _ := store.ListActive(ctx, organizationID, "webpush-proof-user", 10); len(active) != 1 {
		t.Fatalf("re-subscribed endpoint not listed: %+v", active)
	}
}
