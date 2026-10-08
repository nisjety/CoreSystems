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
	// graph-index's own issuer, key and scope (CR-T10): it emits usage events and nothing
	// else. Its key pair is provisioned beside the other event keys, never under
	// .secrets/event-keys in production (the preflight rejects that path).
	graphIssuer = "service:graph-index-rs"
	graphKeyID  = "graph-events-v1"
	graphScope  = "events:graph:publish"

	maxEventTTL = 5 * time.Minute
	clockSkew   = 30 * time.Second

	// maxBacklogAge is how long after its expiry an AUTHENTIC envelope is still accepted
	// when it reaches us through the durable stream. An envelope lives two minutes and
	// the stream keeps events for seven days (streamMaxAge), so anything that waited in
	// the stream longer than the lifetime, a consumer restart or an outage, is expired
	// when it is read. Rejecting it as forged would lose real spend: the same mistake
	// discarded 1,044 of 1,164 chunks in the embedding engine until it learned to tell
	// stale from forged. One day over the stream's age, so the oldest retained event is
	// still readable.
	maxBacklogAge = streamMaxAge + 24*time.Hour

	// Bounds of a v2 event. Tokens and units are stored in BIGINT columns and the item
	// count in an INTEGER one; the producer saturates at the same values
	// (usage-ledger-rs MAX_COUNTER and MAX_COUNT).
	maxCounter = int64(1) << 40
	maxCount   = int64(1)<<31 - 1

	// schemaVersionUsage is the version of the event usage-ledger-rs writes. Version 1 is
	// the count-only event of the producers that predate it: it has no schema_version field.
	schemaVersionUsage = 2
)

var (
	errInvalidEnvelope = errors.New("invalid signed cost event")
	errReplay          = errors.New("signed cost event replay rejected")
)

type RegistryConfig struct {
	EmbeddingPublicKeyPEM []byte
	RetrievalPublicKeyPEM []byte
	GraphPublicKeyPEM     []byte
}

type VerifierRegistry struct {
	byKeyID        map[string]producerVerifier
	replayCapacity int
	replayMu       sync.Mutex
	replay         map[string]int64
	// now is the clock. It is the real one outside tests; a test that replays a recorded
	// envelope sets it to the moment the envelope was signed.
	now func() time.Time
}

// producerKind is one thing a producer may report: the kind of provider call and the
// product feature it was made for. The pair is closed per producer, because the feature
// is what cost-core reports spend by and what the budget exclusion keys on, so a producer
// must not be able to put its spend under another feature's name.
type producerKind struct {
	eventType string
	feature   string
}

type producerVerifier struct {
	publicKey *rsa.PublicKey
	issuer    string
	keyID     string
	scope     string
	// v1Types are the event types a schema_version 1 payload (count and estimated tokens
	// only, no feature) may carry. Empty for a producer that never emitted v1.
	v1Types map[string]struct{}
	// kinds are the (event type, feature) pairs a schema_version 2 payload may carry. The
	// same table, per producer, as usage-ledger-rs's feature.rs.
	kinds map[producerKind]struct{}
}

// v1DefaultFeature is the feature a version 1 event is recorded under: it carries none,
// and the two kinds it can be map onto the feature of the same name.
var v1DefaultFeature = map[string]string{
	"embedding": "embedding",
	"rerank":    "rerank",
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

// usagePayloadV2 is the schema_version 2 event (usage-ledger-rs event.rs). It is decoded
// strictly: a field this struct does not name, such as a user id, a document id or any text,
// makes the event invalid, because a producer that starts sending one must be noticed and not
// have it stored by accident.
type usagePayloadV2 struct {
	SchemaVersion   int64  `json:"schema_version"`
	EventType       string `json:"event_type"`
	Feature         string `json:"feature"`
	ModelUsed       string `json:"model_used"`
	ProviderUsed    string `json:"provider_used"`
	Count           int64  `json:"count"`
	InputTokens     int64  `json:"input_tokens"`
	OutputTokens    int64  `json:"output_tokens"`
	CacheReadTokens int64  `json:"cache_read_tokens"`
	Units           *int64 `json:"units,omitempty"`
	CostBasis       string `json:"cost_basis"`
	Estimated       *bool  `json:"estimated"`
	RequestDigest   string `json:"request_digest"`
	OrgID           string `json:"org_id"`
	ZDR             *bool  `json:"zdr"`
	IdempotencyKey  string `json:"idempotency_key"`
}

type VerifiedCostEvent struct {
	Event Event
	ZDR   bool
	// Stale is true when the envelope was authentic but its own lifetime had passed (only
	// possible through VerifyDurable). It is not a defect of the event.
	Stale           bool
	replayKey       string
	replayExpiresAt int64
}

func NewVerifierRegistry(cfg RegistryConfig, replayCapacity int) (*VerifierRegistry, error) {
	if replayCapacity <= 0 || len(cfg.EmbeddingPublicKeyPEM) == 0 || len(cfg.RetrievalPublicKeyPEM) == 0 ||
		len(cfg.GraphPublicKeyPEM) == 0 {
		return nil, errors.New("invalid signed cost event configuration")
	}
	// What each producer may report. One table, mirrored by usage-ledger-rs/src/feature.rs;
	// the cross-language fixture test fails when the two sides disagree.
	embedding, err := newProducerVerifier(
		cfg.EmbeddingPublicKeyPEM,
		embeddingIssuer,
		embeddingKeyID,
		embeddingScope,
		[]string{"embedding"},
		[]producerKind{
			{eventType: "embedding", feature: "embedding"},
			{eventType: "llm", feature: "contextual-retrieval"},
		},
	)
	if err != nil {
		return nil, err
	}
	retrieval, err := newProducerVerifier(
		cfg.RetrievalPublicKeyPEM,
		retrievalIssuer,
		retrievalKeyID,
		retrievalScope,
		[]string{"rerank"},
		[]producerKind{
			{eventType: "rerank", feature: "rerank"},
			{eventType: "rerank", feature: "web-rerank"},
		},
	)
	if err != nil {
		return nil, err
	}
	graph, err := newProducerVerifier(
		cfg.GraphPublicKeyPEM,
		graphIssuer,
		graphKeyID,
		graphScope,
		nil,
		[]producerKind{
			{eventType: "llm", feature: "graph-extraction"},
			{eventType: "llm", feature: "graph-community-summary"},
		},
	)
	if err != nil {
		return nil, err
	}
	producers := []producerVerifier{embedding, retrieval, graph}
	for i := range producers {
		for j := i + 1; j < len(producers); j++ {
			if producers[i].publicKey.Equal(producers[j].publicKey) {
				return nil, errors.New("signed cost producers must use separate verification keys")
			}
		}
	}
	return &VerifierRegistry{
		byKeyID: map[string]producerVerifier{
			embedding.keyID: embedding,
			retrieval.keyID: retrieval,
			graph.keyID:     graph,
		},
		replayCapacity: replayCapacity,
		replay:         make(map[string]int64),
		now:            time.Now,
	}, nil
}

func LoadVerifierRegistryFromFiles(embeddingPath, retrievalPath, graphPath string, replayCapacity int) (*VerifierRegistry, error) {
	embeddingPEM, err := os.ReadFile(embeddingPath)
	if err != nil {
		return nil, errors.New("read embedding cost producer public key")
	}
	retrievalPEM, err := os.ReadFile(retrievalPath)
	if err != nil {
		return nil, errors.New("read retrieval cost producer public key")
	}
	graphPEM, err := os.ReadFile(graphPath)
	if err != nil {
		return nil, errors.New("read graph cost producer public key")
	}
	return NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: embeddingPEM,
		RetrievalPublicKeyPEM: retrievalPEM,
		GraphPublicKeyPEM:     graphPEM,
	}, replayCapacity)
}

func newProducerVerifier(publicPEM []byte, issuer, keyID, scope string, v1Types []string, kinds []producerKind) (producerVerifier, error) {
	key, err := jwt.ParseRSAPublicKeyFromPEM(publicPEM)
	if err != nil {
		return producerVerifier{}, errors.New("invalid signed cost producer public key")
	}
	producer := producerVerifier{
		publicKey: key,
		issuer:    issuer,
		keyID:     keyID,
		scope:     scope,
		v1Types:   make(map[string]struct{}, len(v1Types)),
		kinds:     make(map[producerKind]struct{}, len(kinds)),
	}
	for _, eventType := range v1Types {
		producer.v1Types[eventType] = struct{}{}
	}
	for _, kind := range kinds {
		producer.kinds[kind] = struct{}{}
	}
	return producer, nil
}

// Verify verifies an envelope that arrived live (a core subscription, or a test): its own
// lifetime is enforced, so an expired envelope is invalid.
func (r *VerifierRegistry) Verify(subject string, envelope []byte) (VerifiedCostEvent, error) {
	return r.verify(subject, envelope, 0)
}

// VerifyDurable verifies an envelope that was delivered by the durable stream. Everything
// Verify checks is checked, and one thing is relaxed: an envelope whose lifetime has passed
// is accepted, up to maxBacklogAge after its expiry, because waiting in a stream is what
// expires it, and it is Stale on the result. Signature, key id, issuer, audience, scope,
// organization, payload digest and every field of the payload are verified exactly as for a
// live one, so a forged envelope is never reachable through this door; only time is
// relaxed. A replay cache cannot remember a stale envelope (it forgets at expiry): its
// duplicates are harmless because the row's idempotency key is unique.
func (r *VerifierRegistry) VerifyDurable(subject string, envelope []byte) (VerifiedCostEvent, error) {
	return r.verify(subject, envelope, maxBacklogAge)
}

func (r *VerifierRegistry) verify(subject string, envelope []byte, staleAllowance time.Duration) (VerifiedCostEvent, error) {
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
		// The library checks the signature and the algorithm. Every claim, time included, is
		// checked below, explicitly, so that the one thing VerifyDurable relaxes (the
		// expiry) is relaxed in one visible place and nothing else is.
		jwt.WithoutClaimsValidation(),
	)
	if err != nil || token == nil || !token.Valid {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	payload, err := base64.RawURLEncoding.DecodeString(wire.Data)
	if err != nil || len(payload) == 0 || len(payload) > maxPayloadBytes {
		return VerifiedCostEvent{}, errInvalidEnvelope
	}
	now := r.clock()
	stale, err := producer.validateClaims(claims, payload, now, staleAllowance)
	if err != nil {
		return VerifiedCostEvent{}, err
	}
	event, err := producer.decodePayload(claims, payload)
	if err != nil {
		return VerifiedCostEvent{}, err
	}
	verified := VerifiedCostEvent{Event: event, ZDR: *claims.ZDR, Stale: stale}
	if stale {
		return verified, nil
	}
	replayKey := producer.issuer + ":" + claims.ID
	replayExpiresAt := claims.ExpiresAt.Unix()
	if err := r.consumeReplay(replayKey, replayExpiresAt, now); err != nil {
		return VerifiedCostEvent{}, err
	}
	verified.replayKey = replayKey
	verified.replayExpiresAt = replayExpiresAt
	return verified, nil
}

func (r *VerifierRegistry) clock() time.Time {
	if r.now == nil {
		return time.Now().UTC()
	}
	return r.now().UTC()
}

// validateClaims checks every claim of the token against the producer it claims to be. It
// reports whether the envelope is authentic but past its own lifetime, and refuses one that
// is past it by more than staleAllowance (zero: any expiry is refused).
func (p producerVerifier) validateClaims(claims *eventClaims, payload []byte, now time.Time, staleAllowance time.Duration) (bool, error) {
	if claims.Issuer != p.issuer || claims.Subject != p.issuer || claims.PrincipalType != "service" ||
		len(claims.Audience) != 1 || claims.Audience[0] != eventAudience ||
		claims.EventType != SubjectCostLedger || claims.OrgID == "" || claims.OrgID != strings.TrimSpace(claims.OrgID) ||
		len(claims.OrgID) > maxIdentityBytes || claims.UserID != nil && len(*claims.UserID) > maxIdentityBytes ||
		claims.ZDR == nil || len(claims.Scopes) != 1 || claims.Scopes[0] != p.scope ||
		claims.ID == "" || claims.ID != strings.TrimSpace(claims.ID) || len(claims.ID) > maxIdentityBytes || claims.ExpiresAt == nil ||
		claims.IssuedAt == nil || claims.NotBefore == nil || claims.NotBefore.Before(claims.IssuedAt.Time) ||
		!claims.ExpiresAt.After(claims.NotBefore.Time) || claims.ExpiresAt.Sub(claims.IssuedAt.Time) > maxEventTTL ||
		claims.IssuedAt.After(now.Add(clockSkew)) || claims.NotBefore.After(now.Add(clockSkew)) {
		return false, errInvalidEnvelope
	}
	digest := sha256.Sum256(payload)
	if claims.PayloadSHA256 != hex.EncodeToString(digest[:]) {
		return false, errInvalidEnvelope
	}
	// Expiry last, once everything else has passed, so that "stale" can only ever describe
	// an envelope that is authentic in every other respect.
	if claims.ExpiresAt.Add(clockSkew).Before(now) {
		if staleAllowance <= 0 || now.Sub(claims.ExpiresAt.Time) > staleAllowance {
			return false, errInvalidEnvelope
		}
		return true, nil
	}
	return false, nil
}

// decodePayload reads the verified payload into an Event: version 1 (no schema_version, the
// count-only event) or version 2 (usage-ledger-rs). Both are decoded strictly.
func (p producerVerifier) decodePayload(claims *eventClaims, payload []byte) (Event, error) {
	var head struct {
		SchemaVersion *int64 `json:"schema_version"`
	}
	// Not strict: this reads one field to choose the strict decoder that reads them all.
	if err := json.Unmarshal(payload, &head); err != nil {
		return Event{}, errInvalidEnvelope
	}
	if head.SchemaVersion == nil {
		return p.decodeV1(claims, payload)
	}
	if *head.SchemaVersion != schemaVersionUsage {
		return Event{}, errInvalidEnvelope
	}
	return p.decodeV2(claims, payload)
}

func (p producerVerifier) decodeV1(claims *eventClaims, payload []byte) (Event, error) {
	var decoded eventPayload
	if err := decodeStrict(payload, &decoded); err != nil || decoded.ZDR == nil {
		return Event{}, errInvalidEnvelope
	}
	if _, ok := p.v1Types[decoded.EventType]; !ok || strings.TrimSpace(decoded.Model) == "" || len(decoded.Model) > maxIdentityBytes ||
		len(decoded.Provider) > maxIdentityBytes ||
		decoded.Count <= 0 || decoded.EstimatedTokens < 0 || decoded.OrgID != claims.OrgID ||
		*decoded.ZDR != *claims.ZDR || decoded.IdempotencyKey == "" ||
		decoded.IdempotencyKey != strings.TrimSpace(decoded.IdempotencyKey) || len(decoded.IdempotencyKey) > 256 ||
		!equalOptionalString(decoded.UserID, claims.UserID) {
		return Event{}, errInvalidEnvelope
	}

	return Event{
		SchemaVersion:   1,
		EventType:       decoded.EventType,
		Feature:         v1DefaultFeature[decoded.EventType],
		Model:           decoded.Model,
		ProviderUsed:    decoded.Provider,
		Count:           decoded.Count,
		EstimatedTokens: decoded.EstimatedTokens,
		OrgID:           decoded.OrgID,
		UserID:          optionalString(decoded.UserID),
		IdempotencyKey:  decoded.IdempotencyKey,
	}, nil
}

func (p producerVerifier) decodeV2(claims *eventClaims, payload []byte) (Event, error) {
	var decoded usagePayloadV2
	if err := decodeStrict(payload, &decoded); err != nil || decoded.Estimated == nil || decoded.ZDR == nil {
		return Event{}, errInvalidEnvelope
	}
	// A usage event is the organization's: it names no user, in the claims or the payload
	// (the struct has no field for one). Background spend booked to a person would count
	// against that person's chat budget.
	if claims.UserID != nil {
		return Event{}, errInvalidEnvelope
	}
	if _, ok := p.kinds[producerKind{eventType: decoded.EventType, feature: decoded.Feature}]; !ok {
		return Event{}, errInvalidEnvelope
	}
	if decoded.SchemaVersion != schemaVersionUsage ||
		!validName(decoded.ModelUsed) || !validName(decoded.ProviderUsed) ||
		decoded.Count < 1 || decoded.Count > maxCount ||
		!inCounterRange(decoded.InputTokens) || !inCounterRange(decoded.OutputTokens) || !inCounterRange(decoded.CacheReadTokens) ||
		decoded.Units != nil && !inCounterRange(*decoded.Units) ||
		decoded.OrgID != claims.OrgID || *decoded.ZDR != *claims.ZDR {
		return Event{}, errInvalidEnvelope
	}
	// The two spellings of "these numbers are not exact" must agree, and the key is the
	// one the producer derives from the digest: a signed event cannot name a key that
	// collides with another event's.
	if decoded.CostBasis != "metered" && decoded.CostBasis != "estimated" ||
		*decoded.Estimated != (decoded.CostBasis == "estimated") ||
		!isLowerHexSHA256(decoded.RequestDigest) ||
		decoded.IdempotencyKey != "usage:"+decoded.Feature+":"+decoded.RequestDigest {
		return Event{}, errInvalidEnvelope
	}
	return Event{
		SchemaVersion: schemaVersionUsage,
		EventType:     decoded.EventType,
		Feature:       decoded.Feature,
		Model:         decoded.ModelUsed,
		ProviderUsed:  decoded.ProviderUsed,
		Count:         int(decoded.Count),
		// The total, so that the Data Plane's own summary (which has always summed this
		// column) keeps meaning "tokens" for rows of both versions. The exact parts are in
		// their own columns.
		EstimatedTokens: decoded.InputTokens + decoded.OutputTokens,
		InputTokens:     decoded.InputTokens,
		OutputTokens:    decoded.OutputTokens,
		CacheReadTokens: decoded.CacheReadTokens,
		Units:           decoded.Units,
		CostBasis:       decoded.CostBasis,
		Estimated:       *decoded.Estimated,
		RequestDigest:   decoded.RequestDigest,
		OrgID:           decoded.OrgID,
		IdempotencyKey:  decoded.IdempotencyKey,
	}, nil
}

func validName(value string) bool {
	return value != "" && value == strings.TrimSpace(value) && len(value) <= maxIdentityBytes
}

func inCounterRange(value int64) bool {
	return value >= 0 && value <= maxCounter
}

func isLowerHexSHA256(value string) bool {
	if len(value) != 64 {
		return false
	}
	for _, character := range value {
		if !(character >= '0' && character <= '9' || character >= 'a' && character <= 'f') {
			return false
		}
	}
	return true
}

func (r *VerifierRegistry) consumeReplay(key string, expiresAt int64, at time.Time) error {
	r.replayMu.Lock()
	defer r.replayMu.Unlock()
	now := at.Unix()
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
