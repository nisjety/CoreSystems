package cache

import (
	"sync"
	"time"
)

type Store struct {
	mu    sync.RWMutex
	items map[string]entry
}

type entry struct {
	expiresAt time.Time
	value     any
}

func New() *Store {
	return &Store{items: make(map[string]entry)}
}

func (s *Store) Get(key string) (any, bool) {
	s.mu.RLock()
	item, ok := s.items[key]
	s.mu.RUnlock()
	if !ok {
		return nil, false
	}
	if time.Now().After(item.expiresAt) {
		s.mu.Lock()
		delete(s.items, key)
		s.mu.Unlock()
		return nil, false
	}
	return item.value, true
}

func (s *Store) Set(key string, ttl time.Duration, value any) {
	s.mu.Lock()
	s.items[key] = entry{
		expiresAt: time.Now().Add(ttl),
		value:     value,
	}
	s.mu.Unlock()
}
