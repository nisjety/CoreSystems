// Package otp provides a Redis-backed store for one-time password codes used
// during the agent-signup onboarding flow.
package otp

import (
	"context"
	"crypto/rand"
	"fmt"
	"math/big"
	"strings"
	"time"

	"github.com/redis/go-redis/v9"
)

const (
	otpTTL       = 10 * time.Minute
	otpKeyPrefix = "otp:"
)

// Store persists and verifies OTP codes in Redis.
type Store struct {
	rdb *redis.Client
}

// NewStore creates a Store backed by the given Redis client.
func NewStore(rdb *redis.Client) *Store {
	return &Store{rdb: rdb}
}

// GenerateCode returns a random 6-digit numeric string.
func GenerateCode() (string, error) {
	max := big.NewInt(1_000_000)
	n, err := rand.Int(rand.Reader, max)
	if err != nil {
		return "", fmt.Errorf("otp: generate code: %w", err)
	}
	return fmt.Sprintf("%06d", n.Int64()), nil
}

// NormalizeEmail lowercases and trims the address.
func NormalizeEmail(email string) string {
	return strings.ToLower(strings.TrimSpace(email))
}

// Store persists the OTP code for email with a 10-minute expiry. Any previous
// code for the same email is overwritten.
func (s *Store) Store(ctx context.Context, email, code string) error {
	key := otpKeyPrefix + NormalizeEmail(email)
	if err := s.rdb.Set(ctx, key, code, otpTTL).Err(); err != nil {
		return fmt.Errorf("otp: store: %w", err)
	}
	return nil
}

// Verify checks whether code matches the stored value for email. On a
// successful match the code is deleted (single-use). Returns false (no error)
// when the code is wrong or has expired.
func (s *Store) Verify(ctx context.Context, email, code string) (bool, error) {
	key := otpKeyPrefix + NormalizeEmail(email)
	stored, err := s.rdb.Get(ctx, key).Result()
	if err == redis.Nil {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("otp: verify get: %w", err)
	}
	if stored != code {
		return false, nil
	}
	// Delete on first successful use.
	_ = s.rdb.Del(ctx, key)
	return true, nil
}
