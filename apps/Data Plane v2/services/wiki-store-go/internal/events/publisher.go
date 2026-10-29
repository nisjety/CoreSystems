// Package events implements §16.3.8 — wiki publish event publisher.
//
// When a wiki page/version transitions to `published`, we emit a NATS
// event that embedding-engine consumes and writes into the
// `wiki_block_embeddings` Qdrant collection. Same shape as
// documents-api-go's publisher so the wire format stays uniform.
package events

import (
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
)

const (
	SubjectWikiPublished = "dataplane.wiki.version.published"
	SubjectWikiDeleted   = "dataplane.wiki.page.deleted"
	WikiStream           = "DATAPLANE_WIKI"
)

type Publisher struct {
	js     jetStreamPublisher
	signer interface {
		Sign(string, []byte) ([]byte, error)
	}
}

type jetStreamPublisher interface {
	Publish(subject string, data []byte, opts ...nats.PubOpt) (*nats.PubAck, error)
}

// NewPublisher accepts a connected NATS client and producer-local signer.
// Missing dependencies fail publication rather than emitting raw content.
func NewPublisher(nc *nats.Conn, signer interface {
	Sign(string, []byte) ([]byte, error)
}) (*Publisher, error) {
	if nc == nil || signer == nil {
		return nil, fmt.Errorf("wiki publisher requires NATS and event signer")
	}
	js, err := nc.JetStream()
	if err != nil {
		return nil, fmt.Errorf("initialize acknowledged wiki publisher: %w", err)
	}
	info, err := js.StreamInfo(WikiStream)
	if errors.Is(err, nats.ErrStreamNotFound) {
		info, err = js.AddStream(&nats.StreamConfig{
			Name:     WikiStream,
			Subjects: []string{SubjectWikiPublished},
			// Interest, NOT WorkQueue. WorkQueue permits exactly one consumer
			// per subject, which is wrong here: `dataplane.wiki.version.published`
			// fans out to more than one reader (embedding-engine-rs's
			// `wiki_consumer` write-through, plus the lexical adapter). Under
			// WorkQueue the second reader is silently refused and its work is
			// simply never done. This matches the retention
			// `embedding-engine-rs/src/wiki_consumer.rs::ensure_wiki_stream`
			// asserts, and the same Interest-over-WorkQueue correction already
			// applied to DATAPLANE_DOCUMENTS and DATAPLANE_KNOWLEDGE.
			Retention: nats.InterestPolicy,
			Storage:   nats.FileStorage,
			MaxAge:    7 * 24 * time.Hour,
		})
		// Another replica may create the stream between StreamInfo and
		// AddStream. Accept that race only after re-reading and validating the
		// authoritative stream contract below.
		if err != nil {
			info, err = js.StreamInfo(WikiStream)
		}
	}
	if err != nil {
		return nil, fmt.Errorf("initialize wiki JetStream stream: %w", err)
	}
	// Retention is IMMUTABLE once a stream exists, so this check decides
	// whether the service can boot at all against an already-created stream.
	// It previously demanded WorkQueue and would fatal on the Interest stream
	// the wiki consumer legitimately creates — a latent crash that only
	// surfaced on the next restart, long after the retention was corrected.
	if info == nil || info.Config.Retention != nats.InterestPolicy ||
		!slices.Contains(info.Config.Subjects, SubjectWikiPublished) {
		return nil, fmt.Errorf(
			"wiki JetStream stream contract mismatch: want retention=interest with subject %q, got retention=%v subjects=%v",
			SubjectWikiPublished, info.Config.Retention, info.Config.Subjects,
		)
	}
	return &Publisher{js: js, signer: signer}, nil
}

type WikiVersionPublishedEvent struct {
	PageID      string `json:"page_id"`
	VersionID   string `json:"version_id"`
	OrgID       string `json:"org_id"`
	WorkspaceID string `json:"workspace_id"`
	Title       string `json:"title"`
	Path        string `json:"path"`
	Content     string `json:"content"`
	UserID      string `json:"user_id,omitempty"`
	ZDR         bool   `json:"zdr"`
}

func (p *Publisher) PublishWikiVersionPublished(evt WikiVersionPublishedEvent) error {
	return p.publishWikiVersionPublished(evt, "wiki-version-"+evt.VersionID)
}

func (p *Publisher) publishWikiVersionPublished(evt WikiVersionPublishedEvent, messageID string) error {
	if p == nil || p.js == nil || p.signer == nil {
		return fmt.Errorf("signed wiki publisher is not configured")
	}
	data, err := json.Marshal(evt)
	if err != nil {
		return fmt.Errorf("marshal wiki published event: %w", err)
	}
	envelope, err := p.signer.Sign(SubjectWikiPublished, data)
	if err != nil {
		return fmt.Errorf("sign wiki published event: %w", err)
	}
	if strings.TrimSpace(messageID) == "" {
		return fmt.Errorf("stable wiki message id is required")
	}
	ack, err := p.js.Publish(SubjectWikiPublished, envelope, nats.MsgId(messageID))
	if err != nil {
		return fmt.Errorf("wiki JetStream publish acknowledgement: %w", err)
	}
	if ack == nil || ack.Stream != WikiStream {
		return fmt.Errorf("wiki JetStream returned invalid acknowledgement")
	}
	return nil
}

func (p *Publisher) PublishAcknowledged(eventType string, payload []byte, messageID string) error {
	if eventType != SubjectWikiPublished {
		return fmt.Errorf("unsupported wiki outbox event type")
	}
	var event WikiVersionPublishedEvent
	if err := json.Unmarshal(payload, &event); err != nil {
		return fmt.Errorf("decode wiki outbox payload: %w", err)
	}
	return p.publishWikiVersionPublished(event, messageID)
}
