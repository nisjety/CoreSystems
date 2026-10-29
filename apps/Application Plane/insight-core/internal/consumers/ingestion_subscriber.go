package consumers

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"log"
	"strings"
	"time"

	"github.com/nats-io/nats.go"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

const (
	ingestionSubscriberDurable = "insight-core-ingestion-subscriber"
	ingestionStream            = "VEREVON_INGESTION"
	ingestionSubject           = "verevon.ingestion.>"
	ingestionMetricSource      = "ingestion-plane"
)

// ingestionEvent is the narrow, content-free intersection of Import Core's
// existing import/crawl event contract. We project counts and identifiers only:
// document text, titles, URLs, tokens, and provider credentials never enter
// Insight Core.
type ingestionEvent struct {
	ID    string `json:"id,omitempty"`
	OrgID string `json:"org_id,omitempty"`
	// Integration Core's established event envelope uses camelCase. Keep both
	// shapes at this boundary so each upstream stays contract-faithful without
	// a lossy cross-plane wrapper.
	OrganizationID string  `json:"organizationId,omitempty"`
	UserID         string  `json:"user_id,omitempty"`
	ActorUserID    string  `json:"actor_user_id,omitempty"`
	EnvelopeUserID string  `json:"userId,omitempty"`
	ImportID       string  `json:"import_id,omitempty"`
	CrawlID        string  `json:"crawl_id,omitempty"`
	Source         string  `json:"source,omitempty"`
	DocumentCount  float64 `json:"document_count,omitempty"`
	PageCount      float64 `json:"page_count,omitempty"`
	Timestamp      string  `json:"timestamp,omitempty"`
	CreatedAt      string  `json:"createdAt,omitempty"`
}

type ingestionMetric struct {
	source  string
	surface string
	metric  string
	value   float64
}

// IngestionSubscriber consumes the existing Application Plane ingestion stream
// through a provisioned durable. It never creates JetStream topology at runtime.
type IngestionSubscriber struct {
	consumer *DurableConsumer
	recorder MetricRecorder
}

func NewIngestionSubscriber(js nats.JetStreamContext, recorder MetricRecorder) *IngestionSubscriber {
	return &IngestionSubscriber{
		consumer: NewDurableConsumer(js, "ingestion-subscriber"),
		recorder: recorder,
	}
}

func (s *IngestionSubscriber) Start(_ context.Context) error {
	return s.consumer.BindProvisioned(ingestionSubject, ingestionStream, ingestionSubscriberDurable, s.handle)
}

func (s *IngestionSubscriber) Stop() { s.consumer.Stop() }

func (s *IngestionSubscriber) handle(msg *nats.Msg) {
	var ev ingestionEvent
	if err := json.Unmarshal(msg.Data, &ev); err != nil {
		log.Printf("[insight-core/ingestion-subscriber] decode %s: %v", msg.Subject, err)
		_ = msg.Ack()
		return
	}
	if s.process(context.Background(), msg.Subject, ev) == outcomeRetry {
		if err := msg.Nak(); err != nil {
			log.Printf("[insight-core/ingestion-subscriber] nak: %v", err)
		}
		return
	}
	if err := msg.Ack(); err != nil {
		log.Printf("[insight-core/ingestion-subscriber] ack: %v", err)
	}
}

// process maps only imported/crawled lifecycle events that have a proven
// contract. Unknown events are ignored rather than guessed into a metric.
func (s *IngestionSubscriber) process(ctx context.Context, subject string, ev ingestionEvent) outcome {
	orgID := firstNonEmpty(ev.OrgID, ev.OrganizationID)
	if orgID == "" {
		return outcomeAck
	}
	occurredAt, err := time.Parse(time.RFC3339, firstNonEmpty(ev.Timestamp, ev.CreatedAt))
	if err != nil {
		log.Printf("[insight-core/ingestion-subscriber] invalid timestamp for %s: %v", subject, err)
		return outcomeAck
	}

	metrics := ingestionMetrics(subject, ev)
	if len(metrics) == 0 {
		return outcomeAck
	}
	for _, metric := range metrics {
		if _, err := s.recorder.RecordMetricEvent(ctx, insights.IngestMetricEventInput{
			ID:          ingestionMetricEventID(subject, ev, metric.metric),
			OrgID:       orgID,
			ActorUserID: firstNonEmpty(ev.ActorUserID, ev.UserID, ev.EnvelopeUserID),
			Surface:     firstNonEmpty(metric.surface, insights.SurfaceIngestion),
			Metric:      metric.metric,
			Value:       metric.value,
			Unit:        "count",
			Source:      firstNonEmpty(metric.source, ingestionMetricSource),
			OccurredAt:  occurredAt.UTC(),
		}); err != nil {
			log.Printf("[insight-core/ingestion-subscriber] record %s for org %s: %v", metric.metric, orgID, err)
			return outcomeRetry
		}
	}
	return outcomeAck
}

func ingestionMetrics(subject string, ev ingestionEvent) []ingestionMetric {
	switch subject {
	case "verevon.ingestion.import.started":
		return []ingestionMetric{{metric: "imports_started", value: 1}}
	case "verevon.ingestion.import.completed":
		metrics := []ingestionMetric{{metric: "imports_completed", value: 1}}
		if ev.DocumentCount > 0 {
			metrics = append(metrics, ingestionMetric{metric: "documents_imported", value: ev.DocumentCount})
		}
		return metrics
	case "verevon.ingestion.crawl.started":
		return []ingestionMetric{{metric: "crawls_started", value: 1}}
	case "verevon.ingestion.crawl.completed":
		metrics := []ingestionMetric{{metric: "crawls_completed", value: 1}}
		if ev.PageCount > 0 {
			metrics = append(metrics, ingestionMetric{metric: "pages_crawled", value: ev.PageCount})
		}
		return metrics
	case "verevon.ingestion.crawl.failed":
		return []ingestionMetric{{metric: "crawls_failed", value: 1}}
	case "verevon.ingestion.integration.sync.started":
		return []ingestionMetric{{metric: "syncs_started", value: 1}}
	case "verevon.ingestion.integration.sync.handoff":
		return []ingestionMetric{{metric: "syncs_handed_off", value: 1}}
	case "verevon.ingestion.integration.sync.completed":
		return []ingestionMetric{{metric: "syncs_completed", value: 1}}
	case "verevon.ingestion.integration.sync.failed":
		return []ingestionMetric{{metric: "syncs_failed", value: 1}}
	case "verevon.ingestion.integration.sync.cancelled":
		return []ingestionMetric{{metric: "syncs_cancelled", value: 1}}
	case "verevon.ingestion.integration.sync.checkpoint":
		return []ingestionMetric{{metric: "sync_checkpoints", value: 1}}
	case "verevon.ingestion.knowledge.document.created":
		return []ingestionMetric{{surface: insights.SurfaceKnowledge, source: "data-plane", metric: "documents_created", value: 1}}
	case "verevon.ingestion.knowledge.document.updated":
		return []ingestionMetric{{surface: insights.SurfaceKnowledge, source: "data-plane", metric: "documents_updated", value: 1}}
	case "verevon.ingestion.knowledge.document.deleted":
		return []ingestionMetric{{surface: insights.SurfaceKnowledge, source: "data-plane", metric: "documents_deleted", value: 1}}
	default:
		return nil
	}
}

func ingestionMetricEventID(subject string, ev ingestionEvent, metric string) string {
	if eventID := strings.TrimSpace(ev.ID); eventID != "" {
		return metricEventID(eventID, metric)
	}
	identity := strings.TrimSpace(ev.ImportID)
	if identity == "" {
		identity = strings.TrimSpace(ev.CrawlID)
	}
	if identity == "" {
		return ""
	}
	value := strings.Join([]string{subject, identity, metric, firstNonEmpty(ev.Timestamp, ev.CreatedAt)}, "|")
	sum := sha1.Sum([]byte(value))
	return "ins_evt_ingestion_" + hex.EncodeToString(sum[:10])
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if normalized := strings.TrimSpace(value); normalized != "" {
			return normalized
		}
	}
	return ""
}
