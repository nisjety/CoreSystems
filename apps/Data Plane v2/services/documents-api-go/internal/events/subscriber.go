package events

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
)

const SubjectQuarryCrawled = "quarry.documents.crawled"

type Subscriber struct {
	sharedNC  *nats.Conn
	localNC   *nats.Conn
	repo      *repo.DocumentRepo
	publisher *Publisher
}

func NewSubscriber(sharedNC, localNC *nats.Conn, r *repo.DocumentRepo, p *Publisher) *Subscriber {
	return &Subscriber{sharedNC: sharedNC, localNC: localNC, repo: r, publisher: p}
}

func (s *Subscriber) SubscribeQuarryCrawl(ctx context.Context) error {
	_, err := s.sharedNC.Subscribe(SubjectQuarryCrawled, func(msg *nats.Msg) {
		var evt struct {
			DocumentID string          `json:"document_id"`
			OrgID      string          `json:"org_id"`
			Source     string          `json:"source"`
			Type       string          `json:"type"`
			Title      string          `json:"title"`
			Content    string          `json:"content"`
			Metadata   json.RawMessage `json:"metadata,omitempty"`
		}
		if err := json.Unmarshal(msg.Data, &evt); err != nil {
			log.Error().Err(err).Msg("invalid quarry crawl event")
			return
		}

		if evt.OrgID == "" || evt.Content == "" {
			log.Warn().Str("document_id", evt.DocumentID).Msg("quarry event missing org_id or content, skipping")
			return
		}

		// Use Quarry's document_id (or source URL fallback) as idempotency key.
		// Re-crawls of the same URL are common; without this we'd accumulate
		// duplicate documents on every crawl pass.
		idempKey := evt.DocumentID
		if idempKey == "" {
			idempKey = evt.Source
		}

		result, err := s.repo.Create(context.Background(), model.CreateDocumentInput{
			OrgID:          evt.OrgID,
			Source:         evt.Source,
			Type:           evt.Type,
			Title:          evt.Title,
			Content:        evt.Content,
			Metadata:       evt.Metadata,
			IdempotencyKey: idempKey,
		})
		if err != nil {
			log.Error().Err(err).Str("source", evt.Source).Msg("failed to create document from quarry event")
			return
		}

		if result.Reused {
			log.Info().Str("document_id", result.Document.DocumentID).Str("source", evt.Source).Msg("quarry document already exists, skipping")
			return
		}

		_ = s.publisher.PublishDocumentCreated(DocumentCreatedEvent{
			DocumentID: result.Document.DocumentID,
			OrgID:      result.Document.OrgID,
			Source:     result.Document.Source,
			Type:       result.Document.Type,
			Title:      result.Document.Title,
		})

		log.Info().Str("document_id", result.Document.DocumentID).Str("source", evt.Source).Msg("document created from quarry crawl")
	})
	if err != nil {
		return fmt.Errorf("subscribe quarry crawl: %w", err)
	}

	log.Info().Str("subject", SubjectQuarryCrawled).Msg("subscribed to shared NATS for quarry events")
	return nil
}
