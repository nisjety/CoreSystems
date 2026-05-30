package platform

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/redis/go-redis/v9"
)

type IdempotencyState string

const (
	IdempotencyInProgress IdempotencyState = "in_progress"
	IdempotencyCompleted  IdempotencyState = "completed"
)

type IdempotencyRecord struct {
	Scope       string           `json:"scope"`
	Key         string           `json:"key"`
	RequestHash string           `json:"requestHash"`
	State       IdempotencyState `json:"state"`
	StatusCode  int              `json:"statusCode,omitempty"`
	ContentType string           `json:"contentType,omitempty"`
	Body        []byte           `json:"body,omitempty"`
	CreatedAt   time.Time        `json:"createdAt"`
	ExpiresAt   time.Time        `json:"expiresAt"`
}

type IdempotencyStore interface {
	Reserve(ctx context.Context, scope, key, requestHash string, ttl time.Duration) (*IdempotencyRecord, bool, error)
	Commit(ctx context.Context, scope, key string, statusCode int, contentType string, body []byte, ttl time.Duration) (*IdempotencyRecord, error)
	Release(ctx context.Context, scope, key string) error
}

type memoryIdempotencyStore struct {
	mu      sync.Mutex
	records map[string]*IdempotencyRecord
}

type redisIdempotencyStore struct {
	client *redis.Client
}

func NewIdempotencyStore(redisURL string) IdempotencyStore {
	if strings.TrimSpace(redisURL) != "" {
		if opts, err := redis.ParseURL(redisURL); err == nil {
			client := redis.NewClient(opts)
			if client.Ping(context.Background()).Err() == nil {
				return &redisIdempotencyStore{client: client}
			}
			_ = client.Close()
		}
	}
	return &memoryIdempotencyStore{
		records: make(map[string]*IdempotencyRecord),
	}
}

func (s *memoryIdempotencyStore) Reserve(_ context.Context, scope, key, requestHash string, ttl time.Duration) (*IdempotencyRecord, bool, error) {
	storeKey := buildIdempotencyStoreKey(scope, key)
	now := time.Now().UTC()

	s.mu.Lock()
	defer s.mu.Unlock()

	if existing, ok := s.records[storeKey]; ok {
		if now.Before(existing.ExpiresAt) {
			return cloneIdempotencyRecord(existing), false, nil
		}
		delete(s.records, storeKey)
	}

	record := &IdempotencyRecord{
		Scope:       scope,
		Key:         key,
		RequestHash: requestHash,
		State:       IdempotencyInProgress,
		CreatedAt:   now,
		ExpiresAt:   now.Add(ttl),
	}
	s.records[storeKey] = cloneIdempotencyRecord(record)
	return cloneIdempotencyRecord(record), true, nil
}

func (s *memoryIdempotencyStore) Commit(_ context.Context, scope, key string, statusCode int, contentType string, body []byte, ttl time.Duration) (*IdempotencyRecord, error) {
	storeKey := buildIdempotencyStoreKey(scope, key)
	now := time.Now().UTC()

	s.mu.Lock()
	defer s.mu.Unlock()

	record, ok := s.records[storeKey]
	if !ok || record == nil {
		return nil, fmt.Errorf("idempotency key is not reserved")
	}
	record.State = IdempotencyCompleted
	record.StatusCode = statusCode
	record.ContentType = contentType
	record.Body = append([]byte(nil), body...)
	record.ExpiresAt = now.Add(ttl)
	s.records[storeKey] = cloneIdempotencyRecord(record)
	return cloneIdempotencyRecord(record), nil
}

func (s *memoryIdempotencyStore) Release(_ context.Context, scope, key string) error {
	s.mu.Lock()
	delete(s.records, buildIdempotencyStoreKey(scope, key))
	s.mu.Unlock()
	return nil
}

func (s *redisIdempotencyStore) Reserve(ctx context.Context, scope, key, requestHash string, ttl time.Duration) (*IdempotencyRecord, bool, error) {
	storeKey := buildIdempotencyStoreKey(scope, key)
	now := time.Now().UTC()
	record := &IdempotencyRecord{
		Scope:       scope,
		Key:         key,
		RequestHash: requestHash,
		State:       IdempotencyInProgress,
		CreatedAt:   now,
		ExpiresAt:   now.Add(ttl),
	}
	payload, err := json.Marshal(record)
	if err != nil {
		return nil, false, err
	}
	acquired, err := s.client.SetNX(ctx, storeKey, payload, ttl).Result()
	if err != nil {
		return nil, false, err
	}
	if acquired {
		return cloneIdempotencyRecord(record), true, nil
	}
	existing, err := s.get(ctx, storeKey)
	if err != nil {
		return nil, false, err
	}
	return existing, false, nil
}

func (s *redisIdempotencyStore) Commit(ctx context.Context, scope, key string, statusCode int, contentType string, body []byte, ttl time.Duration) (*IdempotencyRecord, error) {
	storeKey := buildIdempotencyStoreKey(scope, key)
	record, err := s.get(ctx, storeKey)
	if err != nil {
		return nil, err
	}
	if record == nil {
		return nil, fmt.Errorf("idempotency key is not reserved")
	}
	record.State = IdempotencyCompleted
	record.StatusCode = statusCode
	record.ContentType = contentType
	record.Body = append([]byte(nil), body...)
	record.ExpiresAt = time.Now().UTC().Add(ttl)
	payload, err := json.Marshal(record)
	if err != nil {
		return nil, err
	}
	if err := s.client.Set(ctx, storeKey, payload, ttl).Err(); err != nil {
		return nil, err
	}
	return cloneIdempotencyRecord(record), nil
}

func (s *redisIdempotencyStore) Release(ctx context.Context, scope, key string) error {
	return s.client.Del(ctx, buildIdempotencyStoreKey(scope, key)).Err()
}

func (s *redisIdempotencyStore) get(ctx context.Context, storeKey string) (*IdempotencyRecord, error) {
	raw, err := s.client.Get(ctx, storeKey).Bytes()
	if err != nil {
		if err == redis.Nil {
			return nil, nil
		}
		return nil, err
	}
	var record IdempotencyRecord
	if err := json.Unmarshal(raw, &record); err != nil {
		return nil, err
	}
	return &record, nil
}

func buildIdempotencyStoreKey(scope, key string) string {
	return "idempotency:" + strings.TrimSpace(scope) + ":" + strings.TrimSpace(key)
}

func cloneIdempotencyRecord(record *IdempotencyRecord) *IdempotencyRecord {
	if record == nil {
		return nil
	}
	cloned := *record
	cloned.Body = append([]byte(nil), record.Body...)
	return &cloned
}
