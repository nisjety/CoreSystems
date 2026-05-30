// Package events implements §16.3.8 — wiki publish event publisher.
//
// When a wiki page/version transitions to `published`, we emit a NATS
// event that embedding-engine consumes and writes into the
// `wiki_block_embeddings` Qdrant collection. Same shape as
// documents-api-go's publisher so the wire format stays uniform.
package events

import (
	"encoding/json"
	"fmt"

	"github.com/nats-io/nats.go"
)

const (
	SubjectWikiPublished = "dataplane.wiki.version.published"
	SubjectWikiDeleted   = "dataplane.wiki.page.deleted"
)

type Publisher struct {
	nc *nats.Conn
}

// NewPublisher accepts a connected NATS client; pass nil to disable
// publishing (useful in tests). Disabled publishers no-op all calls.
func NewPublisher(nc *nats.Conn) *Publisher {
	return &Publisher{nc: nc}
}

type WikiVersionPublishedEvent struct {
	PageID      string `json:"page_id"`
	VersionID   string `json:"version_id"`
	OrgID       string `json:"org_id"`
	WorkspaceID string `json:"workspace_id"`
	Title       string `json:"title"`
	Path        string `json:"path"`
	Content     string `json:"content"`
}

func (p *Publisher) PublishWikiVersionPublished(evt WikiVersionPublishedEvent) error {
	if p == nil || p.nc == nil {
		return nil
	}
	data, err := json.Marshal(evt)
	if err != nil {
		return fmt.Errorf("marshal wiki published event: %w", err)
	}
	return p.nc.Publish(SubjectWikiPublished, data)
}
