package cost

import (
	"bytes"
	"crypto/rsa"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	DefaultReplayCapacity = 10_000
	maxEnvelopeBytes      = 128 * 1024
	maxPayloadBytes       = 64 * 1024
	maxIdentityBytes      = 256

	eventAudience   = "dataplane-events"
	embeddingIssuer = "service:embedding-engine-rs"
	embeddingKeyID  = "embedding-events-v1"
	embeddingScope  = "events:embedding:publish"
	retrievalIssuer = "service:retrieval-engine-rs"
	retrievalKeyID  = "retrieval-events-v1"
	retrievalScope  = "events:retrieval:publish"

	maxEventTTL = 5 * time.Minute
	clockSkew   = 30 * time.Second
)

var (
	errInvalidEnvelope = errors.New("invalid signed cost event")
	errReplay          = errors.New("signed cost event replay rejected")
)

type RegistryConfig struct {
	EmbeddingPublicKeyPEM []byte
	RetrievalPublicKeyPEM []byte
}

type VerifierRegistry struct {
	byKeyID        map[string]producerVerifier
	replayCapacity int
	replayMu       sync.Mutex
	replay         map[string]int64
}

type producerVerifier struct {
	publicKey *rsa.PublicKey
	issuer    string
	keyID     string
	scope     string
	eventType string
}

type eventClaims struct {
	PrincipalType string   `json:"principal_type"`
	OrgID         string   `json:"org_id"`
	UserID        *string  `json:"user_id,omitempty"`
	Scopes        []string `json:"scopes"`
	ZDR           *bool    `json:"zdr"`
	EventType     string   `json:"event_type"`
	PayloadSHA256 string   `json:"payload_sha256"`
	jwt.RegisteredClaims
}

type wireEnvelope struct {
	Authorization string `json:"authorization"`
	Data          string `json:"data"`
}

type tokenHeader struct {
	Algorithm string `json:"alg"`
	KeyID     string `json:"kid"`
	Type      string `json:"typ,omitempty"`
}

type eventPayload struct {
	EventType       string  `json:"event_type"`
	Model           string  `json:"model"`
	Provider        string  `json:"provider,omitempty"`
	Count           int     `json:"count"`
	EstimatedTokens int64   `json:"estimated_tokens"`
	OrgID           string  `json:"org_id"`
	UserID          *string `json:"user_id,omitempty"`
	ZDR             *bool   `json:"zdr"`
	IdempotencyKey  string  `json:"idempotency_key"`
}

type VerifiedCostEvent struct {
	Event           Event
	ZDR             bool
	replayKey       string
	replayExpiresAt int64
}

func NewVerifierRegistry(cfg RegistryConfig, replayCapacity int) (*VerifierRegistry, error) {
	if replayCapacity <= 0 || len(cfg.EmbeddingPublicKeyPEM) == 0 || len(cfg.RetrievalPublicKeyPEM) == 0 {
		return nil, errors.New("invalid signed cost event configuration")
	}
	embedding, err := newProducerVerifier(
		cfg.EmbeddingPublicKeyPEM,
		embeddingIssuer,
		embeddingKeyID,
		embeddingScope,
		"embedding",
	)
	if err != nil {
		return nil, err
	}
	retrieval, err := newProducerVerifier(
		cfg.RetrievalPublicKeyPEM,
		retrievalIssuer,
		retrievalKeyID,
		retrievalScope,
		"rerank",
	)
	if err != nil {
		return nil, err
	}
	if embedding.publicKey.Equal(retrieval.publicKey) {
		return nil, errors.New("signed cost producers must use separate verification keys")
	}
	return &VerifierRegistry{
		byKeyID: map[string]producerVerifier{
			embedding.keyID: embedding,
			retrieval.keyID: retrieval,
		},
		replayCapacity: replayCapacity,
		replay:         make(map[string]int64),
	}, nil
}

func LoadVerifierRegistryFromFiles(embeddingPath, retrievalPath string, replayCapacity int) (*VerifierRegistry, error) {
	embeddingPEM, err := os.ReadFile(embeddingPath)
	if err != nil {
		return nil, errors.New("read embedding cost producer public key")
	}
	retrievalPEM, err := os.ReadFile(retrievalPath)
	if err != nil {
		return nil, errors.New("read retrieval cost producer public key")
	}
	return NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: embeddingPEM,
		RetrievalPublicKeyPEM: retrievalPEM,
	}, replayCapacity)
}

func newProducerVerifier(publicPEM []byte, issuer, keyID, scope, eventType string) (producerVerifier, error) {
	key, err := jwt.ParseRSAPublicKeyFromPEM(publicPEM)
	if err != nil {
		return producerVerifier{}, errors.New("invalid signed cost producer public key")
	}
	return producerVerifier{
		publicKey: key,
		issuer:    issuer,
		keyID:     keyID,
		scope:     scope,
		eventType: eventType,
	}, nil
}

func (r *VerifierRegistry) Verify(subject string, envelope []byte) (VerifiedCostEvent, error) {
	if r == nil || subject != SubjectCostLedger || len(envelope) == 0 || len(envelope) > maxEnvelopeBytes {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	var wire wireEnvelope
	if err := decodeStrict(envelope, &wire); err != nil {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	tokenString, ok := strings.CutPrefix(wire.Authorization, "Bearer ")
	if !ok || tokenString == "" {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	header, err := parseTokenHeader(tokenString)
	if err != nil || header.Algorithm != jwt.SigningMethodRS256.Alg() {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	producer, ok := r.byKeyID[header.KeyID]
	if !ok {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}

	claims := &eventClaims{}
	token, err := jwt.ParseWithClaims(
		tokenString,
		claims,
		func(token *jwt.Token) (any, error) {
			if token.Method != jwt.SigningMethodRS256 || token.Header["kid"] != producer.keyID {
				return nil, errInvalidEnvelope
			}
			return producer.publicKey, nil
		},
		jwt.WithValidMethods([]string{jwt.SigningMethodRS256.Alg()}),
		jwt.WithAudience(eventAudience),
		jwt.WithIssuer(producer.issuer),
		jwt.WithExpirationRequired(),
		jwt.WithIssuedAt(),
		jwt.WithLeeway(clockSkew),
	)
	if err != nil || token == nil || !token.Valid {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	payload, err := base64.RawURLEncoding.DecodeString(wire.Data)
	if err != nil || len(payload) == 0 || len(payload) > maxPayloadBytes {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	event, err := producer.validateClaimsAndPayload(claims, payload)
	if err != nil {
		return VerifiedCostEvent{}, err
	}
	replayKey := producer.issuer + ":" + claims.ID
	replayExpiresAt := claims.ExpiresAt.Unix()
	if err := r.consumeReplay(replayKey, replayExpiresAt); err != nil {
		return VerifiedCostEvent{}, err
	}
	return VerifiedCostEvent{
		Event:           event,
		ZDR:             *claims.ZDR,
		replayKey:       replayKey,
		replayExpiresAt: replayExpiresAt,
	}, nil
}

func (p producerVerifier) validateClaimsAndPayload(claims *eventClaims, payload []byte) (Event, error) {
	now := time.Now().UTC()
	if claims.Issuer != p.issuer || claims.Subject != p.issuer || claims.PrincipalType != "service" ||
		len(claims.Audience) != 1 || claims.Audience[0] != eventAudience ||
		claims.EventType != SubjectCostLedger || claims.OrgID == "" || claims.OrgID != strings.TrimSpace(claims.OrgID) ||
		len(claims.OrgID) > maxIdentityBytes || claims.UserID != nil && len(*claims.UserID) > maxIdentityBytes ||
		claims.ZDR == nil || len(claims.Scopes) != 1 || claims.Scopes[0] != p.scope ||
		claims.ID == "" || claims.ID != strings.TrimSpace(claims.ID) || len(claims.ID) > maxIdentityBytes || claims.ExpiresAt == nil ||
		claims.IssuedAt == nil || claims.NotBefore == nil || claims.NotBefore.Before(claims.IssuedAt.Time) ||
		!claims.ExpiresAt.After(claims.NotBefore.Time) || claims.ExpiresAt.Sub(claims.IssuedAt.Time) > maxEventTTL ||
		claims.IssuedAt.After(now.Add(clockSkew)) {
		return Event{}, errInvalidEnvelope
	}
	digest := sha256.Sum256(payload)
	if claims.PayloadSHA256 != hex.EncodeToString(digest[:]) {
		return Event{}, errInvalidEnvelope
	}

	var decoded eventPayload
	if err := decodeStrict(payload, &decoded); err != nil || decoded.ZDR == nil {
		return Event{}, errInvalidEnvelope
	}
	if decoded.EventType != p.eventType || strings.TrimSpace(decoded.Model) == "" || len(decoded.Model) > maxIdentityBytes ||
		len(decoded.Provider) > maxIdentityBytes ||
		decoded.Count <= 0 || decoded.EstimatedTokens < 0 || decoded.OrgID != claims.OrgID ||
		*decoded.ZDR != *claims.ZDR || decoded.IdempotencyKey == "" ||
		decoded.IdempotencyKey != strings.TrimSpace(decoded.IdempotencyKey) || len(decoded.IdempotencyKey) > 256 ||
		!equalOptionalString(decoded.UserID, claims.UserID) {
		return Event{}, errInvalidEnvelope
	}

	return Event{
		EventType:       decoded.EventType,
		Model:           decoded.Model,
		Count:           decoded.Count,
		EstimatedTokens: decoded.EstimatedTokens,
		OrgID:           decoded.OrgID,
		UserID:          optionalString(decoded.UserID),
		IdempotencyKey:  decoded.IdempotencyKey,
	}, nil
}

func (r *VerifierRegistry) consumeReplay(key string, expiresAt int64) error {
	r.replayMu.Lock()
	defer r.replayMu.Unlock()
	now := time.Now().UTC().Unix()
	next := make(map[string]int64, len(r.replay)+1)
	for existingKey, expiry := range r.replay {
		if expiry+int64(clockSkew.Seconds()) >= now {
			next[existingKey] = expiry
		}
	}
	if _, exists := next[key]; exists {
		return errReplay
	}
	if len(next) >= r.replayCapacity {
		return errors.New("signed cost event replay cache unavailable")
	}
	next[key] = expiresAt
	r.replay = next
	return nil
}

func (r *VerifierRegistry) releaseReplay(key string, expiresAt int64) {
	r.replayMu.Lock()
	defer r.replayMu.Unlock()
	if current, ok := r.replay[key]; !ok || current != expiresAt {
		return
	}
	next := make(map[string]int64, len(r.replay)-1)
	for existingKey, expiry := range r.replay {
		if existingKey != key {
			next[existingKey] = expiry
		}
	}
	r.replay = next
}

func parseTokenHeader(token string) (tokenHeader, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return tokenHeader{}, errInvalidEnvelope
	}
	raw, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return tokenHeader{}, errInvalidEnvelope
	}
	var header tokenHeader
	if err := json.Unmarshal(raw, &header); err != nil || header.KeyID == "" {
		return tokenHeader{}, errInvalidEnvelope
	}
	return header, nil
}

func decodeStrict(raw []byte, destination any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(destination); err != nil {
		return err
	}
	if err := decoder.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		return fmt.Errorf("trailing JSON data")
	}
	return nil
}

func equalOptionalString(left, right *string) bool {
	if left == nil || right == nil {
		return left == nil && right == nil
	}
	return *left == *right && *left != "" && *left == strings.TrimSpace(*left)
}

func optionalString(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}
