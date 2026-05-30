// Package idempotency provides blake3 key derivation and a deduplication interface.
package idempotency

import (
	"context"
	"encoding/hex"
	"sync"

	"github.com/zeebo/blake3"
)

// DeriveHash computes the idempotency hash: blake3(producer|event_type|resource_ref|key).
func DeriveHash(producer, eventType, resourceRef, key string) string {
	h := blake3.New()
	h.Write([]byte(producer))
	h.Write([]byte("|"))
	h.Write([]byte(eventType))
	h.Write([]byte("|"))
	h.Write([]byte(resourceRef))
	h.Write([]byte("|"))
	h.Write([]byte(key))
	return hex.EncodeToString(h.Sum(nil))
}

// Deduplicator checks whether an idempotency key has been seen.
type Deduplicator interface {
	// IsDuplicate returns true if the key was already processed.
	IsDuplicate(ctx context.Context, hash string) (bool, error)

	// MarkProcessed records the key as processed.
	MarkProcessed(ctx context.Context, hash string) error
}

// InMemoryDeduplicator is a thread-safe in-memory implementation for tests.
type InMemoryDeduplicator struct {
	mu   sync.Mutex
	seen map[string]struct{}
}

// NewInMemoryDeduplicator creates a new in-memory deduplicator.
func NewInMemoryDeduplicator() *InMemoryDeduplicator {
	return &InMemoryDeduplicator{seen: make(map[string]struct{})}
}

// IsDuplicate checks the in-memory set.
func (d *InMemoryDeduplicator) IsDuplicate(_ context.Context, hash string) (bool, error) {
	d.mu.Lock()
	defer d.mu.Unlock()
	_, exists := d.seen[hash]
	return exists, nil
}

// MarkProcessed adds the hash to the in-memory set.
func (d *InMemoryDeduplicator) MarkProcessed(_ context.Context, hash string) error {
	d.mu.Lock()
	defer d.mu.Unlock()
	d.seen[hash] = struct{}{}
	return nil
}
