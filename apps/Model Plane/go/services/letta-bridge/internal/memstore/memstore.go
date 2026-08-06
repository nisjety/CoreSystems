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
	UserID    string
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
func (s *Store) Put(orgID, threadID, topic, memoryID, userID, content string) (*Record, error) {
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
		UserID:    userID,
		Content:   content,
		UpdatedAt: time.Now().UTC(),
	}
	s.records[key(orgID, threadID, memoryID)] = rec
	return rec, nil
}

// Delete removes the record matching orgID + memoryID whose UserID equals
// userID, returning whether a record was found and removed. Scoping by
// userID is what makes this safe for DSAR erasure -- without it, a delete
// request for one user's memory_id could remove another user's record it
// happens to collide with (memory_id is caller-supplied, not guaranteed
// globally unique across users within an org). Callers with the wrong
// userID or a nonexistent memoryID both get (false, nil): "not found" and
// "found but not yours" are indistinguishable from the outside by design --
// a delete for someone else's memory must not leak whether it exists.
//
// There is no threadID parameter (the Store interface's Delete doesn't take
// one), so this scans every record for the org+memoryID pair regardless of
// thread. O(n) is acceptable here: this backend is the in-process dev/test
// tier, never the production durable store.
func (s *Store) Delete(orgID, userID, memoryID string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	for k, r := range s.records {
		if r.OrgID == orgID && r.MemoryID == memoryID && r.UserID == userID {
			delete(s.records, k)
			return true
		}
	}
	return false
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
		// "updated after" is a strict lower bound. Equal timestamps can occur on
		// coarse clocks and must not leak into the next incremental page.
		if !updatedAfter.IsZero() && !r.UpdatedAt.After(updatedAfter) {
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
