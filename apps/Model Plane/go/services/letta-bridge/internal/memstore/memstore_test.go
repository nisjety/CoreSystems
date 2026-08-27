package memstore

import (
	"fmt"
	"sync"
	"testing"
	"time"
)

func TestPutAndSearchRoundTrip(t *testing.T) {
	tests := []struct {
		name      string
		orgID     string
		threadID  string
		topic     string
		memoryID  string
		content   string
		query     string
		wantHits  int
		wantError bool
	}{
		{
			name:     "exact substring match",
			orgID:    "org-1",
			threadID: "thread-1",
			topic:    "MEMORY",
			memoryID: "mem-1",
			content:  "the quick brown fox jumps over the lazy dog",
			query:    "brown fox",
			wantHits: 1,
		},
		{
			name:     "case insensitive match",
			orgID:    "org-1",
			threadID: "thread-1",
			topic:    "MEMORY",
			memoryID: "mem-2",
			content:  "Hello World",
			query:    "hello",
			wantHits: 1,
		},
		{
			name:     "empty query matches all in scope",
			orgID:    "org-2",
			threadID: "thread-2",
			topic:    "NOTES",
			memoryID: "mem-3",
			content:  "anything goes",
			query:    "",
			wantHits: 1,
		},
		{
			name:     "no match returns empty",
			orgID:    "org-3",
			threadID: "thread-3",
			topic:    "MEMORY",
			memoryID: "mem-4",
			content:  "completely unrelated text",
			query:    "nonexistent",
			wantHits: 0,
		},
		{
			name:      "missing orgID returns put error",
			orgID:     "",
			threadID:  "thread-1",
			topic:     "MEMORY",
			memoryID:  "mem-5",
			content:   "should fail",
			wantError: true,
		},
		{
			name:      "missing memoryID returns put error",
			orgID:     "org-1",
			threadID:  "thread-1",
			topic:     "MEMORY",
			memoryID:  "",
			content:   "should fail",
			wantError: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewStore()
			_, err := s.Put(tt.orgID, tt.threadID, tt.topic, tt.memoryID, "user-1", tt.content)
			if tt.wantError {
				if err == nil {
					t.Fatal("expected Put to return an error, got nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("Put: unexpected error: %v", err)
			}

			hits := s.Search(tt.orgID, "", "", tt.query, nil, time.Time{}, 10)
			if len(hits) != tt.wantHits {
				t.Errorf("Search(%q): got %d hits, want %d", tt.query, len(hits), tt.wantHits)
			}

			// Verify content round-trip for matching cases.
			if tt.wantHits > 0 {
				if hits[0].Content != tt.content {
					t.Errorf("content mismatch: got %q, want %q", hits[0].Content, tt.content)
				}
				if hits[0].MemoryID != tt.memoryID {
					t.Errorf("memoryID mismatch: got %q, want %q", hits[0].MemoryID, tt.memoryID)
				}
			}
		})
	}
}

func TestTopicFiltering(t *testing.T) {
	tests := []struct {
		name        string
		records     []struct{ topic, memID, content string }
		topicFilter []string
		wantHits    int
	}{
		{
			name: "single topic filter",
			records: []struct{ topic, memID, content string }{
				{"MEMORY", "m1", "memory entry"},
				{"NOTES", "m2", "notes entry"},
				{"MEMORY", "m3", "another memory"},
			},
			topicFilter: []string{"MEMORY"},
			wantHits:    2,
		},
		{
			name: "multiple topic filter",
			records: []struct{ topic, memID, content string }{
				{"MEMORY", "m1", "memory entry"},
				{"NOTES", "m2", "notes entry"},
				{"LOG", "m3", "log entry"},
			},
			topicFilter: []string{"NOTES", "LOG"},
			wantHits:    2,
		},
		{
			name: "no filter returns all",
			records: []struct{ topic, memID, content string }{
				{"MEMORY", "m1", "memory entry"},
				{"NOTES", "m2", "notes entry"},
			},
			topicFilter: nil,
			wantHits:    2,
		},
		{
			name: "filter with no matches",
			records: []struct{ topic, memID, content string }{
				{"MEMORY", "m1", "memory entry"},
			},
			topicFilter: []string{"NONEXISTENT"},
			wantHits:    0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := NewStore()
			for _, rec := range tt.records {
				if _, err := s.Put("org-1", "thread-1", rec.topic, rec.memID, "user-1", rec.content); err != nil {
					t.Fatalf("Put: %v", err)
				}
			}

			hits := s.Search("org-1", "", "", "", tt.topicFilter, time.Time{}, 100)
			if len(hits) != tt.wantHits {
				t.Errorf("got %d hits, want %d", len(hits), tt.wantHits)
			}

			// Verify all returned hits have an allowed topic.
			if len(tt.topicFilter) > 0 {
				allowed := make(map[string]struct{}, len(tt.topicFilter))
				for _, topic := range tt.topicFilter {
					allowed[topic] = struct{}{}
				}
				for _, h := range hits {
					if _, ok := allowed[h.Topic]; !ok {
						t.Errorf("hit topic %q not in allowed set %v", h.Topic, tt.topicFilter)
					}
				}
			}
		})
	}
}

func TestTimeRangeFiltering(t *testing.T) {
	s := NewStore()

	// Insert records with controlled timing.
	if _, err := s.Put("org-1", "thread-1", "MEMORY", "old-1", "user-1", "old record"); err != nil {
		t.Fatalf("Put: %v", err)
	}

	// Record the boundary time after the first insert.
	cutoff := time.Now().UTC()

	// Small delay to ensure the second record has a strictly later timestamp.
	time.Sleep(2 * time.Millisecond)

	if _, err := s.Put("org-1", "thread-1", "MEMORY", "new-1", "user-1", "new record"); err != nil {
		t.Fatalf("Put: %v", err)
	}

	tests := []struct {
		name         string
		updatedAfter time.Time
		wantHits     int
	}{
		{
			name:         "zero time returns all",
			updatedAfter: time.Time{},
			wantHits:     2,
		},
		{
			name:         "cutoff excludes old record",
			updatedAfter: cutoff,
			wantHits:     1,
		},
		{
			name:         "future time returns none",
			updatedAfter: time.Now().UTC().Add(time.Hour),
			wantHits:     0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			hits := s.Search("org-1", "", "", "", nil, tt.updatedAfter, 100)
			if len(hits) != tt.wantHits {
				t.Errorf("got %d hits, want %d", len(hits), tt.wantHits)
			}
		})
	}
}

func TestConcurrentAccess(t *testing.T) {
	s := NewStore()
	const goroutines = 50
	const opsPerGoroutine = 20

	var wg sync.WaitGroup
	wg.Add(goroutines)

	for g := range goroutines {
		go func(gIdx int) {
			defer wg.Done()
			for i := range opsPerGoroutine {
				memID := fmt.Sprintf("mem-%d-%d", gIdx, i)
				content := fmt.Sprintf("content from goroutine %d item %d", gIdx, i)

				_, err := s.Put("org-concurrent", "thread-concurrent", "MEMORY", memID, "user-1", content)
				if err != nil {
					t.Errorf("Put(%s): %v", memID, err)
					return
				}

				// Interleave reads with writes.
				s.Search("org-concurrent", "", "", "content", nil, time.Time{}, 10)
			}
		}(g)
	}

	wg.Wait()

	// After all goroutines complete, verify the total count.
	allHits := s.Search("org-concurrent", "", "", "", nil, time.Time{}, 0)
	expected := goroutines * opsPerGoroutine
	if len(allHits) != expected {
		t.Errorf("expected %d records, got %d", expected, len(allHits))
	}
}

func TestSearchResultOrder(t *testing.T) {
	// The store returns results in map iteration order (nondeterministic),
	// but the scoring mechanism should give higher scores to prefix matches.
	s := NewStore()

	// Record whose content starts with the query gets score 1.0.
	if _, err := s.Put("org-1", "thread-1", "MEMORY", "prefix", "user-1", "hello world greeting"); err != nil {
		t.Fatalf("Put: %v", err)
	}
	// Record whose content contains but does not start with the query gets score 0.5.
	if _, err := s.Put("org-1", "thread-1", "MEMORY", "contains", "user-1", "say hello to the world"); err != nil {
		t.Fatalf("Put: %v", err)
	}

	hits := s.Search("org-1", "", "", "hello", nil, time.Time{}, 100)
	if len(hits) != 2 {
		t.Fatalf("expected 2 hits, got %d", len(hits))
	}

	// Verify that scores are assigned according to prefix matching.
	scoreByID := make(map[string]float32, len(hits))
	for _, h := range hits {
		scoreByID[h.MemoryID] = h.Score
	}

	if scoreByID["prefix"] != 1.0 {
		t.Errorf("prefix match score: got %f, want 1.0", scoreByID["prefix"])
	}
	if scoreByID["contains"] != 0.5 {
		t.Errorf("contains match score: got %f, want 0.5", scoreByID["contains"])
	}
}

func TestThreadIDScoping(t *testing.T) {
	s := NewStore()

	if _, err := s.Put("org-1", "thread-A", "MEMORY", "m1", "user-1", "shared content"); err != nil {
		t.Fatalf("Put: %v", err)
	}
	if _, err := s.Put("org-1", "thread-B", "MEMORY", "m2", "user-1", "shared content"); err != nil {
		t.Fatalf("Put: %v", err)
	}

	// Search scoped to thread-A should only return 1 hit.
	hits := s.Search("org-1", "thread-A", "", "", nil, time.Time{}, 100)
	if len(hits) != 1 {
		t.Errorf("thread-A: got %d hits, want 1", len(hits))
	}
	if len(hits) > 0 && hits[0].ThreadID != "thread-A" {
		t.Errorf("expected thread-A, got %q", hits[0].ThreadID)
	}

	// Search without thread filter should return both.
	allHits := s.Search("org-1", "", "", "", nil, time.Time{}, 100)
	if len(allHits) != 2 {
		t.Errorf("unscoped: got %d hits, want 2", len(allHits))
	}
}

func TestPutOverwritesExistingRecord(t *testing.T) {
	s := NewStore()

	if _, err := s.Put("org-1", "thread-1", "MEMORY", "m1", "user-1", "original"); err != nil {
		t.Fatalf("Put: %v", err)
	}
	if _, err := s.Put("org-1", "thread-1", "MEMORY", "m1", "user-1", "updated"); err != nil {
		t.Fatalf("Put: %v", err)
	}

	hits := s.Search("org-1", "", "", "", nil, time.Time{}, 100)
	if len(hits) != 1 {
		t.Fatalf("expected 1 record after overwrite, got %d", len(hits))
	}
	if hits[0].Content != "updated" {
		t.Errorf("expected updated content, got %q", hits[0].Content)
	}
}

func TestTopKLimit(t *testing.T) {
	s := NewStore()
	for i := range 10 {
		memID := fmt.Sprintf("m-%d", i)
		if _, err := s.Put("org-1", "thread-1", "MEMORY", memID, "user-1", "common content"); err != nil {
			t.Fatalf("Put: %v", err)
		}
	}

	hits := s.Search("org-1", "", "", "", nil, time.Time{}, 3)
	if len(hits) != 3 {
		t.Errorf("expected topK=3 to return 3 hits, got %d", len(hits))
	}

	// topK=0 should return all.
	allHits := s.Search("org-1", "", "", "", nil, time.Time{}, 0)
	if len(allHits) != 10 {
		t.Errorf("expected topK=0 to return all 10, got %d", len(allHits))
	}
}

func TestDeleteScopedByOwner(t *testing.T) {
	s := NewStore()
	if _, err := s.Put("org-1", "thread-1", "MEMORY", "m1", "user-a", "user a's memory"); err != nil {
		t.Fatalf("Put: %v", err)
	}

	// Wrong user cannot delete another user's record, and the record survives.
	if deleted := s.Delete("org-1", "user-b", "m1"); deleted {
		t.Fatal("Delete with the wrong userID reported success")
	}
	if hits := s.Search("org-1", "", "", "", nil, time.Time{}, 100); len(hits) != 1 {
		t.Fatalf("record should survive a wrong-owner delete attempt, got %d hits", len(hits))
	}

	// Wrong org, correct user: still not found (org isolation).
	if deleted := s.Delete("org-2", "user-a", "m1"); deleted {
		t.Fatal("Delete with the wrong orgID reported success")
	}

	// Nonexistent memoryID: false, no error, no panic.
	if deleted := s.Delete("org-1", "user-a", "does-not-exist"); deleted {
		t.Fatal("Delete of a nonexistent memoryID reported success")
	}

	// Correct owner can delete, and the record is actually gone afterward.
	if deleted := s.Delete("org-1", "user-a", "m1"); !deleted {
		t.Fatal("Delete with the correct owner reported failure")
	}
	if hits := s.Search("org-1", "", "", "", nil, time.Time{}, 100); len(hits) != 0 {
		t.Fatalf("record should be gone after owner delete, got %d hits", len(hits))
	}

	// Deleting again is a clean, non-erroring false -- not a repeat success.
	if deleted := s.Delete("org-1", "user-a", "m1"); deleted {
		t.Fatal("second Delete of an already-removed record reported success")
	}
}

// TestSearchScopesUserOwnedMemories pins the per-user visibility rule.
//
// Before `userID` reached Search, the only scoping was org + optional thread, so
// a search with no thread filter returned every user's personal memories in the
// org. That was safe in production only because the single caller
// (session-core's SearchMemory) always passed a thread id and a thread has one
// owner — the boundary itself enforced nothing. This asserts the boundary.
//
// The rule mirrors session-core's durable query (`scope = 'user' AND owner = $3`):
// your own user-owned memories, plus everything owned by nobody.
func TestSearchScopesUserOwnedMemories(t *testing.T) {
	s := NewStore()
	// Two users' personal memories plus one org-level memory (no owner).
	s.Put("org-1", "thread-a", "USER", "mem-alice", "alice", "alice prefers metric units")
	s.Put("org-1", "thread-b", "USER", "mem-bob", "bob", "bob prefers imperial units")
	s.Put("org-1", "thread-c", "POLICY", "mem-org", "", "invoices are approved by finance")

	// No thread filter — the case the old code answered org-wide.
	hits := s.Search("org-1", "", "alice", "prefers", nil, time.Time{}, 100)
	ids := make([]string, 0, len(hits))
	for _, h := range hits {
		ids = append(ids, h.MemoryID)
	}
	if len(ids) != 1 || ids[0] != "mem-alice" {
		t.Fatalf("alice must see only her own personal memory, got %v", ids)
	}

	// Org-level memories are owned by nobody and stay visible to everyone.
	orgHits := s.Search("org-1", "", "alice", "invoices", nil, time.Time{}, 100)
	if len(orgHits) != 1 || orgHits[0].MemoryID != "mem-org" {
		t.Fatalf("an unowned org memory must remain visible, got %v", orgHits)
	}

	// And the regression itself: without a user filter the old behaviour returns.
	unscoped := s.Search("org-1", "", "", "prefers", nil, time.Time{}, 100)
	if len(unscoped) != 2 {
		t.Fatalf("an empty user id is documented as no filter (org-wide); got %d hits, want 2", len(unscoped))
	}
}
