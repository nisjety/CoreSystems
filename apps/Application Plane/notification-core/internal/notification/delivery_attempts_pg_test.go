package notification

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/feed"
)

// This test is intentionally opt-in. It requires a disposable database URL
// and an explicit mutation acknowledgement so a developer cannot accidentally
// run migrations or cleanup against the shared dev database.
func TestDeliveryAttemptsLeaseAndReceiptAgainstRealPostgres(t *testing.T) {
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

	requestID := "delivery-proof-request"
	_, err = db.Pool.Exec(ctx, `
INSERT INTO notification_requests (id, organization_id, idempotency_key, request_sha256, retention_mode,
  recipient_kind, recipient_id, type, payload, source, status, provider, created_at, updated_at)
VALUES ($1, 'delivery-proof-org', 'delivery-proof-key', repeat('a', 64), 'standard',
  'user', 'delivery-proof-user', 'notification.created', '{"title":"proof"}', 'test', 'accepted', 'novu', now(), now())
ON CONFLICT (id) DO NOTHING`, requestID)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `DELETE FROM notification_requests WHERE id = $1`, requestID)
		_, _ = db.Pool.Exec(context.Background(), `DELETE FROM notification_feed_items WHERE id = $1`, requestID)
	}()

	repository := NewRepository(db.Pool)
	now := time.Date(2026, time.August, 16, 21, 0, 0, 0, time.UTC)
	queued, err := repository.EnqueueDeliveryAttempt(ctx, DeliveryAttemptParams{
		ID: "delivery-proof-attempt-1", NotificationID: requestID, AttemptNumber: 1, OccurredAt: now,
	})
	if err != nil || queued.Status != DeliveryAttemptPending {
		t.Fatalf("enqueue = %#v, %v", queued, err)
	}
	claimed, err := repository.ClaimDeliveryAttempt(ctx, "worker-a", now, time.Minute)
	if err != nil || claimed.Status != DeliveryAttemptClaimed || claimed.WorkerID != "worker-a" {
		t.Fatalf("claim = %#v, %v", claimed, err)
	}
	if _, err := repository.ClaimDeliveryAttempt(ctx, "worker-b", now, time.Minute); err != ErrNotFound {
		t.Fatalf("live lease claim error = %v, want ErrNotFound", err)
	}
	if _, err := repository.MarkDeliverySubmitted(ctx, claimed.ID, "worker-b", "provider-wrong-worker", now); err != ErrNotFound {
		t.Fatalf("stale worker submission error = %v, want ErrNotFound", err)
	}
	submitted, err := repository.MarkDeliverySubmitted(ctx, claimed.ID, "worker-a", "provider-proof-1", now)
	if err != nil || submitted.Status != DeliveryAttemptSentUnconfirmed {
		t.Fatalf("submission = %#v, %v", submitted, err)
	}
	var projectionStatus, projectionDelivery string
	if err := db.Pool.QueryRow(ctx, `
SELECT status, delivery_status
FROM notification_feed_projection_attempts
WHERE attempt_id = $1`, claimed.ID).Scan(&projectionStatus, &projectionDelivery); err != nil {
		t.Fatalf("feed projection outbox lookup: %v", err)
	}
	if projectionStatus != string(FeedProjectionPending) || projectionDelivery != FeedDeliverySubmitted {
		t.Fatalf("feed projection = (%q, %q), want pending/submitted", projectionStatus, projectionDelivery)
	}
	acknowledged, err := repository.MarkDeliveryAcknowledged(ctx, claimed.ID, "provider-proof-1", "receipt-proof-1", now.Add(time.Second))
	if err != nil || acknowledged.Status != DeliveryAttemptAcknowledged {
		t.Fatalf("acknowledgement = %#v, %v", acknowledged, err)
	}
	if err := db.Pool.QueryRow(ctx, `
SELECT status, delivery_status, provider_receipt_digest
FROM notification_feed_projection_attempts
WHERE attempt_id = $1`, claimed.ID).Scan(&projectionStatus, &projectionDelivery, new(string)); err != nil {
		t.Fatalf("feed projection acknowledgement lookup: %v", err)
	}
	if projectionStatus != string(FeedProjectionPending) || projectionDelivery != FeedDeliveryDelivered {
		t.Fatalf("feed projection after acknowledgement = (%q, %q), want pending/delivered", projectionStatus, projectionDelivery)
	}
	feedClaimed, err := repository.ClaimFeedProjection(ctx, "feed-worker-a", now.Add(2*time.Second), time.Minute)
	if err != nil || feedClaimed.Status != FeedProjectionClaimed || feedClaimed.WorkerID != "feed-worker-a" {
		t.Fatalf("feed claim = %#v, %v", feedClaimed, err)
	}
	if _, err := repository.ClaimFeedProjection(ctx, "feed-worker-b", now.Add(2*time.Second), time.Minute); err != ErrNotFound {
		t.Fatalf("live feed lease claim error = %v, want ErrNotFound", err)
	}
	if _, err := repository.MarkFeedProjectionProjected(ctx, feedClaimed.ID, "feed-worker-b", now.Add(2*time.Second)); err != ErrNotFound {
		t.Fatalf("stale feed projection receipt error = %v, want ErrNotFound", err)
	}
	if _, err := repository.MarkFeedProjectionUnknown(ctx, feedClaimed.ID, "feed-worker-a", "projection-worker-crash", now.Add(2*time.Second)); err != nil {
		t.Fatalf("feed projection unknown transition: %v", err)
	}

	feedRepository := feed.NewRepository(db.Pool)
	feedWorker := NewFeedProjectionWorker(repository, repository, func(ctx context.Context, params FeedSinkParams) error {
		_, err := feedRepository.Create(ctx, feed.CreateParams{
			ID:                    params.RequestID,
			OrganizationID:        params.OrganizationID,
			RecipientID:           params.RecipientID,
			EventType:             params.Type,
			Channel:               feed.ChannelInApp,
			Title:                 params.Title,
			Body:                  params.Body,
			Payload:               params.Payload,
			Provider:              params.Provider,
			ProviderTransactionID: params.ProviderTransactionID,
			Source:                params.Source,
			DeliveryStatus:        params.DeliveryStatus,
			SubmittedAt:           params.SubmittedAt,
			DeliveredAt:           params.DeliveredAt,
		})
		return err
	}, WithFeedProjectionWorkerClock(func() time.Time { return now.Add(8 * time.Second) }))
	processed, err := feedWorker.RunOnce(ctx, "feed-worker-proof")
	if err != nil || !processed {
		t.Fatalf("feed projection worker = (%v, %v), want one processed projection", processed, err)
	}
	var feedStatus string
	if err := db.Pool.QueryRow(ctx, `
SELECT delivery_status
FROM notification_feed_items
WHERE id = $1 AND organization_id = $2 AND recipient_id = $3`, requestID, "delivery-proof-org", "delivery-proof-user").Scan(&feedStatus); err != nil {
		t.Fatalf("feed item lookup: %v", err)
	}
	if feedStatus != feed.DeliveryDelivered {
		t.Fatalf("feed delivery_status = %q, want delivered", feedStatus)
	}
	if _, err := feedRepository.Create(ctx, feed.CreateParams{
		ID:                    requestID,
		OrganizationID:        "delivery-proof-org",
		RecipientID:           "delivery-proof-user",
		EventType:             "notification.created",
		Channel:               feed.ChannelInApp,
		Payload:               map[string]any{"title": "proof"},
		Provider:              "novu",
		ProviderTransactionID: "provider-proof-1",
		Source:                "test",
		DeliveryStatus:        feed.DeliveryDelivered,
		SubmittedAt:           now,
		DeliveredAt:           func() *time.Time { value := now.Add(time.Second); return &value }(),
	}); err != nil {
		t.Fatalf("idempotent feed replay: %v", err)
	}
	if err := db.Pool.QueryRow(ctx, `
SELECT status
FROM notification_feed_projection_attempts
WHERE attempt_id = $1`, claimed.ID).Scan(&projectionStatus); err != nil {
		t.Fatalf("feed projection terminal lookup: %v", err)
	}
	if projectionStatus != string(FeedProjectionProjected) {
		t.Fatalf("feed projection status = %q, want projected", projectionStatus)
	}
	if _, err := repository.MarkDeliveryAcknowledged(ctx, claimed.ID, "provider-proof-1", "receipt-proof-duplicate", now.Add(2*time.Second)); err != ErrNotFound {
		t.Fatalf("terminal acknowledgement error = %v, want ErrNotFound", err)
	}

	replayKey := "delivery-proof-callback-nonce"
	defer func() {
		_, _ = db.Pool.Exec(context.Background(), `DELETE FROM notification_delivery_callback_replays WHERE nonce = $1`, replayKey)
	}()
	first, err := repository.Claim(ctx, replayKey, time.Now().UTC().Add(5*time.Minute))
	if err != nil || !first {
		t.Fatalf("first callback replay claim = %v, %v", first, err)
	}
	second, err := repository.Claim(ctx, replayKey, time.Now().UTC().Add(5*time.Minute))
	if err != nil || second {
		t.Fatalf("duplicate callback replay claim = %v, %v", second, err)
	}
	if _, err := db.Pool.Exec(ctx, `UPDATE notification_delivery_callback_replays SET expires_at = NOW() WHERE nonce = $1`, replayKey); err != nil {
		t.Fatal(err)
	}
	reclaimed, err := repository.Claim(ctx, replayKey, time.Now().UTC().Add(5*time.Minute))
	if err != nil || !reclaimed {
		t.Fatalf("expired callback replay claim = %v, %v", reclaimed, err)
	}
}
