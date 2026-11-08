package webpush

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// sealer seals the subscription fields with AES-256-GCM. Each field is bound to its row (organization, user and
// endpoint hash) and to its column as additional data, so a ciphertext moved to another row or column does not
// open.
type sealer struct {
	aead cipher.AEAD
}

func newSealer(hexKey string) (*sealer, error) {
	key, err := hex.DecodeString(strings.TrimSpace(hexKey))
	if err != nil || len(key) != 32 {
		return nil, errors.New("NOTIFICATION_PUSH_SUBSCRIPTION_KEY must be 32 bytes in hex")
	}
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, err
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	return &sealer{aead: aead}, nil
}

// seal returns nonce || ciphertext for one field.
func (s *sealer) seal(plaintext, additional []byte) ([]byte, error) {
	nonce := make([]byte, s.aead.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return nil, err
	}
	return s.aead.Seal(nonce, nonce, plaintext, additional), nil
}

func (s *sealer) open(sealed, additional []byte) ([]byte, error) {
	nonceSize := s.aead.NonceSize()
	if len(sealed) < nonceSize {
		return nil, errors.New("sealed web push field is truncated")
	}
	return s.aead.Open(nil, sealed[:nonceSize], sealed[nonceSize:], additional)
}

func subscriptionAdditionalData(column, organizationID, userID, endpointHash string) []byte {
	return []byte("verevon.web-push.v1|" + column + "|" + organizationID + "|" + userID + "|" + endpointHash)
}

func endpointHashOf(endpoint string) string {
	digest := sha256.Sum256([]byte(endpoint))
	return hex.EncodeToString(digest[:])
}

func newSubscriptionID() (string, error) {
	raw := make([]byte, 16)
	if _, err := io.ReadFull(rand.Reader, raw); err != nil {
		return "", err
	}
	return "wps_" + hex.EncodeToString(raw), nil
}

// Store keeps subscriptions in notification_web_push_subscriptions. Every read and write is scoped to one
// organization and one user, so no caller can reach another recipient's endpoint.
type Store struct {
	pool   *pgxpool.Pool
	sealer *sealer
}

// NewStore opens the store with the 32-byte subscription key, given in hex.
func NewStore(pool *pgxpool.Pool, hexKey string) (*Store, error) {
	sealer, err := newSealer(hexKey)
	if err != nil {
		return nil, err
	}
	return &Store{pool: pool, sealer: sealer}, nil
}

// Upsert stores one endpoint for one recipient. An endpoint that is already stored for that recipient is
// refreshed in place and revoked-state is cleared, so re-subscribing after a revoke works.
func (s *Store) Upsert(ctx context.Context, organizationID, userID string, subscription Subscription, serviceClass string) (string, error) {
	hash := endpointHashOf(subscription.Endpoint)
	endpointCT, err := s.sealer.seal([]byte(subscription.Endpoint), subscriptionAdditionalData("endpoint", organizationID, userID, hash))
	if err != nil {
		return "", fmt.Errorf("seal web push endpoint: %w", err)
	}
	p256dhCT, err := s.sealer.seal([]byte(subscription.P256DH), subscriptionAdditionalData("p256dh", organizationID, userID, hash))
	if err != nil {
		return "", fmt.Errorf("seal web push key: %w", err)
	}
	authCT, err := s.sealer.seal([]byte(subscription.Auth), subscriptionAdditionalData("auth", organizationID, userID, hash))
	if err != nil {
		return "", fmt.Errorf("seal web push secret: %w", err)
	}
	id, err := newSubscriptionID()
	if err != nil {
		return "", err
	}
	var storedID string
	err = s.pool.QueryRow(ctx, `
INSERT INTO notification_web_push_subscriptions
    (id, organization_id, user_id, endpoint_hash, endpoint_ct, p256dh_ct, auth_ct, ua_class)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
ON CONFLICT (organization_id, user_id, endpoint_hash) DO UPDATE SET
    endpoint_ct = EXCLUDED.endpoint_ct,
    p256dh_ct   = EXCLUDED.p256dh_ct,
    auth_ct     = EXCLUDED.auth_ct,
    ua_class    = EXCLUDED.ua_class,
    revoked_at  = NULL
RETURNING id`, id, organizationID, userID, hash, endpointCT, p256dhCT, authCT, serviceClass).Scan(&storedID)
	if err != nil {
		return "", fmt.Errorf("store web push subscription: %w", err)
	}
	return storedID, nil
}

// ListActive opens the recipient's active subscriptions, most recently registered first.
func (s *Store) ListActive(ctx context.Context, organizationID, userID string, limit int) ([]Subscription, error) {
	rows, err := s.pool.Query(ctx, `
SELECT id, endpoint_hash, endpoint_ct, p256dh_ct, auth_ct
FROM notification_web_push_subscriptions
WHERE organization_id = $1 AND user_id = $2 AND revoked_at IS NULL
ORDER BY created_at DESC
LIMIT $3`, organizationID, userID, limit)
	if err != nil {
		return nil, fmt.Errorf("list web push subscriptions: %w", err)
	}
	defer rows.Close()

	subscriptions := make([]Subscription, 0, limit)
	for rows.Next() {
		var id, hash string
		var endpointCT, p256dhCT, authCT []byte
		if err := rows.Scan(&id, &hash, &endpointCT, &p256dhCT, &authCT); err != nil {
			return nil, fmt.Errorf("scan web push subscription: %w", err)
		}
		endpoint, err := s.sealer.open(endpointCT, subscriptionAdditionalData("endpoint", organizationID, userID, hash))
		if err != nil {
			return nil, fmt.Errorf("open web push endpoint: %w", err)
		}
		p256dh, err := s.sealer.open(p256dhCT, subscriptionAdditionalData("p256dh", organizationID, userID, hash))
		if err != nil {
			return nil, fmt.Errorf("open web push key: %w", err)
		}
		auth, err := s.sealer.open(authCT, subscriptionAdditionalData("auth", organizationID, userID, hash))
		if err != nil {
			return nil, fmt.Errorf("open web push secret: %w", err)
		}
		subscriptions = append(subscriptions, Subscription{
			ID:       id,
			Endpoint: string(endpoint),
			P256DH:   string(p256dh),
			Auth:     string(auth),
		})
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read web push subscriptions: %w", err)
	}
	return subscriptions, nil
}

// Revoke ends one active subscription of the recipient. A subscription that is not theirs, or already revoked,
// is ErrSubscriptionNotFound.
func (s *Store) Revoke(ctx context.Context, organizationID, userID, id string) error {
	tag, err := s.pool.Exec(ctx, `
UPDATE notification_web_push_subscriptions
SET revoked_at = NOW()
WHERE id = $1 AND organization_id = $2 AND user_id = $3 AND revoked_at IS NULL`, id, organizationID, userID)
	if err != nil {
		return fmt.Errorf("revoke web push subscription: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return ErrSubscriptionNotFound
	}
	return nil
}

// MarkDelivered records the last successful push for one subscription.
func (s *Store) MarkDelivered(ctx context.Context, organizationID, userID, id string) error {
	_, err := s.pool.Exec(ctx, `
UPDATE notification_web_push_subscriptions
SET last_ok_at = NOW()
WHERE id = $1 AND organization_id = $2 AND user_id = $3 AND revoked_at IS NULL`, id, organizationID, userID)
	if err != nil {
		return fmt.Errorf("mark web push subscription delivered: %w", err)
	}
	return nil
}
