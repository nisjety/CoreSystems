// Package memstore provides an in-memory, goroutine-safe store for agent
// memory blocks. It backs the letta-bridge stub implementation prior to
// real block-sync wiring.
package memstore

import (
	"errors"
	"strings"
	"sync"
	"time"
)

// Record is a single indexed memory block.
type Record struct {
	OrgID     string
	ThreadID  string
	Topic     string
	MemoryID  string
	Content   string
	UpdatedAt time.Time
}

// Hit is a search result.
type Hit struct {
	MemoryID  string
	ThreadID  string
	Topic     string
	Score     float32
	Content   string
	UpdatedAt time.Time
}

// Store is a goroutine-safe in-memory record store keyed by
// (orgID, threadID, memoryID).
type Store struct {
	mu      sync.Mutex
	records map[string]*Record
}

// NewStore constructs an empty Store.
func NewStore() *Store {
	return &Store{records: make(map[string]*Record)}
}

func key(orgID, threadID, memoryID string) string {
	return orgID + "|" + threadID + "|" + memoryID
}

// Put inserts or replaces a record. Returns an error if required
// identifiers are empty.
func (s *Store) Put(orgID, threadID, topic, memoryID, content string) (*Record, error) {
	if orgID == "" || threadID == "" || topic == "" || memoryID == "" {
		return nil, errors.New("orgID, threadID, topic, and memoryID are required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	rec := &Record{
		OrgID:     orgID,
		ThreadID:  threadID,
		Topic:     topic,
		MemoryID:  memoryID,
		Content:   content,
		UpdatedAt: time.Now().UTC(),
	}
	s.records[key(orgID, threadID, memoryID)] = rec
	return rec, nil
}

// Search returns up to topK hits whose content contains the query substring
// (case-insensitive), scoped to the given org and optional thread/topic filters.
// An empty query matches all records in scope. If topK <= 0, all matches are returned.
func (s *Store) Search(orgID, threadID, query string, topicFilter []string, updatedAfter time.Time, topK int32) []Hit {
	s.mu.Lock()
	defer s.mu.Unlock()
	hits := make([]Hit, 0)
	q := strings.ToLower(query)
	allowedTopics := make(map[string]struct{}, len(topicFilter))
	for _, topic := range topicFilter {
		allowedTopics[topic] = struct{}{}
	}
	for _, r := range s.records {
		if r.OrgID != orgID {
			continue
		}
		if threadID != "" && r.ThreadID != threadID {
			continue
		}
		if len(allowedTopics) > 0 {
			if _, ok := allowedTopics[r.Topic]; !ok {
				continue
			}
		}
		if !updatedAfter.IsZero() && r.UpdatedAt.Before(updatedAfter) {
			continue
		}
		content := strings.ToLower(r.Content)
		if q != "" && !strings.Contains(content, q) {
			continue
		}
		score := float32(1.0)
		if q != "" && !strings.HasPrefix(content, q) {
			score = 0.5
		}
		hits = append(hits, Hit{MemoryID: r.MemoryID, ThreadID: r.ThreadID, Topic: r.Topic, Score: score, Content: r.Content, UpdatedAt: r.UpdatedAt})
		if topK > 0 && int32(len(hits)) >= topK {
			break
		}
	}
	return hits
}
