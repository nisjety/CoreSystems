package nats

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"
)

// SharedPublisher handles publishing events to the shared velion-nats broker.
// Implements graceful degradation: if shared NATS unavailable, service continues normally.
type SharedPublisher struct {
	conn   *nats.Conn
	js     jetstream.JetStream
	client string
}

// NewSharedPublisher creates a new shared NATS publisher.
// Returns nil, nil if NATS_SHARED_URL is empty (graceful disabling).
// Returns nil, error only on actual connection failure.
func NewSharedPublisher(sharedURL, token, clientName string) (*SharedPublisher, error) {
	if sharedURL == "" {
		log.Printf("[%s] Shared NATS disabled (NATS_SHARED_URL empty)", clientName)
		return nil, nil
	}

	// Connect to shared NATS
	nc, err := nats.Connect(
		sharedURL,
		nats.Token(token),
		nats.Name(clientName),
		nats.ConnectHandler(func(*nats.Conn) {
			log.Printf("[%s] Connected to shared NATS at %s", clientName, sharedURL)
		}),
		nats.DisconnectErrHandler(func(_ *nats.Conn, err error) {
			if err != nil {
				log.Printf("[%s] Disconnected from shared NATS: %v", clientName, err)
			}
		}),
		nats.ReconnectHandler(func(*nats.Conn) {
			log.Printf("[%s] Reconnected to shared NATS", clientName)
		}),
	)

	if err != nil {
		log.Printf("[%s] Failed to connect to shared NATS: %v", clientName, err)
		return nil, fmt.Errorf("shared nats connection: %w", err)
	}

	// Get JetStream context
	js, err := jetstream.New(nc)
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("jetstream context: %w", err)
	}

	// Create stream if it doesn't exist
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_, err = js.CreateStream(ctx, jetstream.StreamConfig{
		Name:     "VELION_INGESTION",
		Subjects: []string{"velion.ingestion.>"},
		MaxAge:   14 * 24 * time.Hour, // 14-day retention
		MaxMsgs:  100_000,
	})

	if err != nil && !isStreamExistsError(err) {
		log.Printf("[%s] Warning: could not ensure stream exists: %v", clientName, err)
	}

	log.Printf("[%s] Shared NATS publisher initialized", clientName)

	return &SharedPublisher{
		conn:   nc,
		js:     js,
		client: clientName,
	}, nil
}

// publish is the internal publish method (fire-and-forget to JetStream).
// Never blocks the caller; returns immediately.
func (p *SharedPublisher) publish(ctx context.Context, subject string, payload interface{}) error {
	if p == nil || p.js == nil {
		return nil
	}

	data, err := json.Marshal(payload)
	if err != nil {
		log.Printf("[%s] Marshal failed for %s: %v", p.client, subject, err)
		return nil // Don't block caller
	}

	// Async publish (non-blocking)
	_, err = p.js.PublishAsync(subject, data)
	if err != nil {
		log.Printf("[%s] PublishAsync failed for %s: %v", p.client, subject, err)
		return nil // Don't block caller
	}

	log.Printf("[%s] Published: %s (size: %d bytes)", p.client, subject, len(data))
	return nil
}

// PublishCrawlStarted publishes a crawl started event.
// Subject: velion.ingestion.crawl.started
func (p *SharedPublisher) PublishCrawlStarted(
	ctx context.Context,
	orgID, url, crawlID string,
	metadata map[string]interface{},
) error {
	if p == nil {
		return nil // Graceful no-op
	}

	payload := map[string]interface{}{
		"org_id":   orgID,
		"url":      url,
		"crawl_id": crawlID,
		"service":  p.client,
		"metadata": metadata,
	}

	return p.publish(ctx, "velion.ingestion.crawl.started", payload)
}

// PublishCrawlCompleted publishes a crawl completed event.
// Subject: velion.ingestion.crawl.completed
func (p *SharedPublisher) PublishCrawlCompleted(
	ctx context.Context,
	orgID, url, crawlID string,
	pageCount int,
	metadata map[string]interface{},
) error {
	if p == nil {
		return nil // Graceful no-op
	}

	payload := map[string]interface{}{
		"org_id":     orgID,
		"url":        url,
		"crawl_id":   crawlID,
		"page_count": pageCount,
		"service":    p.client,
		"metadata":   metadata,
	}

	return p.publish(ctx, "velion.ingestion.crawl.completed", payload)
}

// PublishCrawlFailed publishes a crawl failed event.
// Subject: velion.ingestion.crawl.failed
func (p *SharedPublisher) PublishCrawlFailed(
	ctx context.Context,
	orgID, url, crawlID, errorMsg string,
	metadata map[string]interface{},
) error {
	if p == nil {
		return nil // Graceful no-op
	}

	payload := map[string]interface{}{
		"org_id":   orgID,
		"url":      url,
		"crawl_id": crawlID,
		"error":    errorMsg,
		"service":  p.client,
		"metadata": metadata,
	}

	return p.publish(ctx, "velion.ingestion.crawl.failed", payload)
}

// PublishNotificationCrawlCompleted publishes a notification event to notification-core
// via plain NATS core (not JetStream) so the plain subscriber can receive it.
// Subject: notifications.crawl.completed
func (p *SharedPublisher) PublishNotificationCrawlCompleted(
	ctx context.Context,
	userID, jobID, siteURL, orgID string,
	pageCount int,
) error {
	if p == nil || p.conn == nil {
		return nil
	}

	payload := map[string]interface{}{
		"subscriberId":  userID,
		"jobId":         jobID,
		"siteUrl":       siteURL,
		"pageCount":     pageCount,
		"documentCount": pageCount,
		"orgId":         orgID,
		"status":        "success",
	}

	data, err := json.Marshal(payload)
	if err != nil {
		log.Printf("[%s] Failed to marshal notification payload: %v", p.client, err)
		return nil
	}

	if err := p.conn.Publish("velion.notifications.crawl.completed", data); err != nil {
		log.Printf("[%s] Failed to publish velion.notifications.crawl.completed: %v", p.client, err)
	}
	return nil
}

// PublishCrawlIndexed publishes a crawl indexed event once all pages have been
// submitted to the data plane for embedding. This is the definitive "done" signal
// that transitions the Convex ingestJob from "indexing" → "completed".
// Subject: velion.ingestion.crawl.indexed
func (p *SharedPublisher) PublishCrawlIndexed(
	ctx context.Context,
	orgID, crawlID string,
	ingestedCount int,
) error {
	if p == nil {
		return nil
	}

	payload := map[string]interface{}{
		"org_id":         orgID,
		"crawl_id":       crawlID,
		"ingested_count": ingestedCount,
		"service":        p.client,
	}

	return p.publish(ctx, "velion.ingestion.crawl.indexed", payload)
}

// PublishJobProgress publishes a crawl/ingest progress event.
// Subject: velion.ingestion.crawl.progress
// Called periodically during data-plane ingestion so the UI can show live progress.
func (p *SharedPublisher) PublishJobProgress(
	ctx context.Context,
	orgID, jobID string,
	progress, completed, total int,
	message string,
) error {
	if p == nil {
		return nil
	}

	payload := map[string]interface{}{
		"org_id":    orgID,
		"crawl_id":  jobID,
		"progress":  progress,
		"completed": completed,
		"total":     total,
		"message":   message,
		"service":   p.client,
	}

	return p.publish(ctx, "velion.ingestion.crawl.progress", payload)
}

// Close gracefully closes the NATS connection.
func (p *SharedPublisher) Close() error {
	if p == nil || p.conn == nil {
		return nil
	}

	p.conn.Close()
	log.Printf("[%s] Closed shared NATS connection", p.client)
	return nil
}

// PublishUsageRecorded publishes a usage/metering event for billing reconciliation.
// Subject: velion.ingestion.usage.recorded
func (p *SharedPublisher) PublishUsageRecorded(
	ctx context.Context,
	orgID, metric string,
	units int64,
	metadata map[string]interface{},
) error {
	if p == nil {
		return nil
	}

	payload := map[string]interface{}{
		"org_id":   orgID,
		"metric":   metric,
		"units":    units,
		"service":  p.client,
		"metadata": metadata,
	}

	return p.publish(ctx, "velion.ingestion.usage.recorded", payload)
}

// PublishQuotaExceeded publishes a quota exceeded event so other services
// (e.g. imports-core) can react by pausing work for this org.
// Subject: velion.ingestion.quota.exceeded
func (p *SharedPublisher) PublishQuotaExceeded(
	ctx context.Context,
	orgID, metric string,
	limit, current int64,
) error {
	if p == nil {
		return nil
	}

	payload := map[string]interface{}{
		"org_id":  orgID,
		"metric":  metric,
		"limit":   limit,
		"current": current,
		"service": p.client,
	}

	return p.publish(ctx, "velion.ingestion.quota.exceeded", payload)
}

// isStreamExistsError checks if error is due to stream already existing.
func isStreamExistsError(err error) bool {
	if err == nil {
		return false
	}
	return err.Error() == "stream already exists"
}
