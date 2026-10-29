package cost

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/nats-io/nats.go"
)

const testSubjectCostLedger = "dataplane.cost.ledger"

type recordingStore struct {
	calls [][]any
	err   error
}

func (s *recordingStore) Exec(_ context.Context, _ string, args ...any) (pgconn.CommandTag, error) {
	s.calls = append(s.calls, append([]any(nil), args...))
	return pgconn.NewCommandTag("INSERT 0 1"), s.err
}

type fakeSubscriber struct {
	handler nats.MsgHandler
	err     error
}

func (s *fakeSubscriber) Subscribe(_ string, handler nats.MsgHandler) (*nats.Subscription, error) {
	s.handler = handler
	return &nats.Subscription{}, s.err
}

type testProducer struct {
	privateKey *rsa.PrivateKey
	publicPEM  []byte
	issuer     string
	keyID      string
	scope      string
}

type signedEventOptions struct {
	issuer         string
	keyID          string
	audience       string
	scope          string
	claimOrgID     string
	payloadOrgID   string
	claimZDR       bool
	payloadZDR     bool
	eventType      string
	jti            string
	omitClaimZDR   bool
	omitPayloadZDR bool
	extraAudience  bool
}

func TestSignedConsumerRejectsUntrustedCostEvents(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	wrongKey := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	registry := newTestRegistry(t, embedding, retrieval)

	tests := []struct {
		name    string
		payload func(t *testing.T) []byte
	}{
		{
			name: "raw domain JSON",
			payload: func(_ *testing.T) []byte {
				return []byte(`{"event_type":"embedding","org_id":"org-a","zdr":false}`)
			},
		},
		{
			name: "oversized envelope",
			payload: func(_ *testing.T) []byte {
				return make([]byte, maxEnvelopeBytes+1)
			},
		},
		{
			name: "wrong signing key",
			payload: func(t *testing.T) []byte {
				return signTestEvent(t, wrongKey, validEmbeddingOptions())
			},
		},
		{
			name: "wrong issuer",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.issuer = "service:not-embedding"
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "wrong key id",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.keyID = "unexpected-key"
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "wrong audience",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.audience = "another-audience"
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "extra audience",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.extraAudience = true
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "wrong producer scope",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.scope = retrievalScope
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "claim and payload tenant conflict",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.payloadOrgID = "org-b"
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "claim and payload ZDR conflict",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.claimZDR = true
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "missing signed ZDR posture",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.omitClaimZDR = true
				return signTestEvent(t, embedding, opts)
			},
		},
		{
			name: "missing payload ZDR posture",
			payload: func(t *testing.T) []byte {
				opts := validEmbeddingOptions()
				opts.omitPayloadZDR = true
				return signTestEvent(t, embedding, opts)
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			store := &recordingStore{}
			consumer := NewSignedConsumer(store, nil, registry)
			if err := consumer.process(context.Background(), tt.payload(t)); err == nil {
				t.Fatal("untrusted event was accepted")
			}
			if len(store.calls) != 0 {
				t.Fatalf("untrusted event persisted %d rows", len(store.calls))
			}
		})
	}
}

func TestSignedConsumerRejectsReplay(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	consumer := NewSignedConsumer(&recordingStore{}, nil, newTestRegistry(t, embedding, retrieval))
	event := signTestEvent(t, embedding, validEmbeddingOptions())

	if err := consumer.process(context.Background(), event); err != nil {
		t.Fatalf("first delivery failed: %v", err)
	}
	if err := consumer.process(context.Background(), event); err == nil {
		t.Fatal("replayed signed event was accepted")
	}
}

func TestSignedConsumerSuppressesPersistenceForZDR(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	store := &recordingStore{}
	consumer := NewSignedConsumer(store, nil, newTestRegistry(t, embedding, retrieval))
	opts := validEmbeddingOptions()
	opts.claimZDR = true
	opts.payloadZDR = true

	if err := consumer.process(context.Background(), signTestEvent(t, embedding, opts)); err != nil {
		t.Fatalf("valid restrictive event failed: %v", err)
	}
	if len(store.calls) != 0 {
		t.Fatalf("zdr event persisted %d rows", len(store.calls))
	}
}

func TestSignedConsumerAcceptsPinnedEmbeddingAndRetrievalProducers(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	store := &recordingStore{}
	consumer := NewSignedConsumer(store, nil, newTestRegistry(t, embedding, retrieval))

	if err := consumer.process(context.Background(), signTestEvent(t, embedding, validEmbeddingOptions())); err != nil {
		t.Fatalf("embedding cost failed: %v", err)
	}
	retrievalOptions := validEmbeddingOptions()
	retrievalOptions.issuer = retrieval.issuer
	retrievalOptions.keyID = retrieval.keyID
	retrievalOptions.scope = retrieval.scope
	retrievalOptions.eventType = "rerank"
	retrievalOptions.jti = "cost-retrieval-1"
	if err := consumer.process(context.Background(), signTestEvent(t, retrieval, retrievalOptions)); err != nil {
		t.Fatalf("retrieval cost failed: %v", err)
	}

	if len(store.calls) != 2 {
		t.Fatalf("persisted %d rows, want 2", len(store.calls))
	}
	for i, call := range store.calls {
		if got := call[2]; got != "org-a" {
			t.Fatalf("call %d org_id = %v, want signed org-a", i, got)
		}
	}
}

func TestRegistryFileLoadingFailsClosed(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	dir := t.TempDir()
	embeddingPath := filepath.Join(dir, "embedding.pub")
	retrievalPath := filepath.Join(dir, "retrieval.pub")
	if err := os.WriteFile(embeddingPath, embedding.publicPEM, 0o600); err != nil {
		t.Fatalf("write embedding public key: %v", err)
	}
	if _, err := LoadVerifierRegistryFromFiles(embeddingPath, retrievalPath, 32); err == nil {
		t.Fatal("registry loaded with missing retrieval key")
	}
	if err := os.WriteFile(retrievalPath, retrieval.publicPEM, 0o600); err != nil {
		t.Fatalf("write retrieval public key: %v", err)
	}
	if _, err := LoadVerifierRegistryFromFiles(embeddingPath, retrievalPath, 32); err != nil {
		t.Fatalf("complete registry failed: %v", err)
	}
}

func TestRegistryRejectsSharedProducerKey(t *testing.T) {
	producer := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	if _, err := NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: producer.publicPEM,
		RetrievalPublicKeyPEM: producer.publicPEM,
	}, 32); err == nil {
		t.Fatal("producer registry accepted a shared verification key")
	}
}

func TestSignedConsumerStartDispatchesVerifiedMessage(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	store := &recordingStore{}
	broker := &fakeSubscriber{}
	consumer := NewSignedConsumer(store, broker, newTestRegistry(t, embedding, retrieval))
	if _, err := consumer.Start(context.Background()); err != nil {
		t.Fatalf("start consumer: %v", err)
	}
	if broker.handler == nil {
		t.Fatal("consumer did not register a broker handler")
	}
	broker.handler(&nats.Msg{Data: signTestEvent(t, embedding, validEmbeddingOptions())})
	if len(store.calls) != 1 {
		t.Fatalf("broker delivery persisted %d rows, want 1", len(store.calls))
	}

	failingBroker := &fakeSubscriber{err: errors.New("subscribe unavailable")}
	if _, err := NewSignedConsumer(store, failingBroker, newTestRegistry(t, embedding, retrieval)).Start(context.Background()); err == nil {
		t.Fatal("consumer startup did not fail when subscription failed")
	}
}

func TestLegacyConsumerRemainsExplicitAndTenantScopesIdempotency(t *testing.T) {
	store := &recordingStore{}
	consumer := &Consumer{store: store}
	raw := []byte(`{"event_type":"embedding","model":"legacy","count":2,"estimated_tokens":8,"org_ids":["org-a","org-b"],"idempotency_key":"legacy-cost"}`)
	if err := consumer.processMessage(context.Background(), raw); err != nil {
		t.Fatalf("explicit legacy processing failed: %v", err)
	}
	if len(store.calls) != 2 {
		t.Fatalf("legacy rows = %d, want 2", len(store.calls))
	}
	if store.calls[0][6] != "org-a:legacy-cost" || store.calls[1][6] != "org-b:legacy-cost" {
		t.Fatalf("legacy idempotency keys were not tenant-scoped: %v / %v", store.calls[0][6], store.calls[1][6])
	}
	if err := consumer.processMessage(context.Background(), []byte(`{`)); err == nil {
		t.Fatal("malformed legacy event was accepted")
	}
	if got := NewLegacyConsumer(nil, nil); got == nil {
		t.Fatal("legacy constructor returned nil")
	}
}

func TestSignedConsumerReturnsPersistenceFailure(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	store := &recordingStore{err: errors.New("database unavailable")}
	consumer := NewSignedConsumer(store, nil, newTestRegistry(t, embedding, retrieval))
	event := signTestEvent(t, embedding, validEmbeddingOptions())
	if err := consumer.process(context.Background(), event); err == nil {
		t.Fatal("persistence failure was swallowed")
	}
	store.err = nil
	if err := consumer.process(context.Background(), event); err != nil {
		t.Fatalf("broker retry after persistence failure was rejected: %v", err)
	}
	if err := (&Consumer{}).process(context.Background(), nil); err == nil {
		t.Fatal("missing verifier was accepted")
	}
}

func TestVerifierRegistryRejectsInvalidConfigurationAndCapacityExhaustion(t *testing.T) {
	embedding := newTestProducer(t, embeddingIssuer, embeddingKeyID, embeddingScope)
	retrieval := newTestProducer(t, retrievalIssuer, retrievalKeyID, retrievalScope)
	if _, err := NewVerifierRegistry(RegistryConfig{}, 0); err == nil {
		t.Fatal("empty producer registry was accepted")
	}
	if _, err := NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: []byte("not a public key"),
		RetrievalPublicKeyPEM: retrieval.publicPEM,
	}, 1); err == nil {
		t.Fatal("invalid producer key was accepted")
	}

	registry, err := NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: embedding.publicPEM,
		RetrievalPublicKeyPEM: retrieval.publicPEM,
	}, 1)
	if err != nil {
		t.Fatalf("registry: %v", err)
	}
	first := validEmbeddingOptions()
	if _, err := registry.Verify(testSubjectCostLedger, signTestEvent(t, embedding, first)); err != nil {
		t.Fatalf("first event: %v", err)
	}
	second := validEmbeddingOptions()
	second.jti = "cost-embedding-2"
	if _, err := registry.Verify(testSubjectCostLedger, signTestEvent(t, embedding, second)); err == nil {
		t.Fatal("full replay cache accepted a new event")
	}
}

func newTestRegistry(t *testing.T, embedding, retrieval testProducer) *VerifierRegistry {
	t.Helper()
	registry, err := NewVerifierRegistry(RegistryConfig{
		EmbeddingPublicKeyPEM: embedding.publicPEM,
		RetrievalPublicKeyPEM: retrieval.publicPEM,
	}, 32)
	if err != nil {
		t.Fatalf("new verifier registry: %v", err)
	}
	return registry
}

func newTestProducer(t *testing.T, issuer, keyID, scope string) testProducer {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	publicDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	return testProducer{
		privateKey: key,
		publicPEM:  pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER}),
		issuer:     issuer,
		keyID:      keyID,
		scope:      scope,
	}
}

func validEmbeddingOptions() signedEventOptions {
	return signedEventOptions{
		issuer:       embeddingIssuer,
		keyID:        embeddingKeyID,
		audience:     eventAudience,
		scope:        embeddingScope,
		claimOrgID:   "org-a",
		payloadOrgID: "org-a",
		eventType:    "embedding",
		jti:          "cost-embedding-1",
	}
}

func signTestEvent(t *testing.T, producer testProducer, opts signedEventOptions) []byte {
	t.Helper()
	now := time.Now().UTC()
	payloadFields := map[string]any{
		"event_type":       opts.eventType,
		"model":            "test-model",
		"count":            2,
		"estimated_tokens": 64,
		"org_id":           opts.payloadOrgID,
		"user_id":          "user-a",
		"zdr":              opts.payloadZDR,
		"idempotency_key":  opts.jti,
	}
	if opts.omitPayloadZDR {
		delete(payloadFields, "zdr")
	}
	payload, err := json.Marshal(payloadFields)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	digest := sha256.Sum256(payload)
	claims := jwt.MapClaims{
		"iss":            opts.issuer,
		"sub":            opts.issuer,
		"aud":            opts.audience,
		"principal_type": "service",
		"org_id":         opts.claimOrgID,
		"user_id":        "user-a",
		"scopes":         []string{opts.scope},
		"zdr":            opts.claimZDR,
		"event_type":     testSubjectCostLedger,
		"payload_sha256": hex.EncodeToString(digest[:]),
		"jti":            opts.jti,
		"iat":            now.Unix(),
		"nbf":            now.Unix(),
		"exp":            now.Add(2 * time.Minute).Unix(),
	}
	if opts.extraAudience {
		claims["aud"] = []string{opts.audience, "another-audience"}
	}
	if opts.omitClaimZDR {
		delete(claims, "zdr")
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = opts.keyID
	signed, err := token.SignedString(producer.privateKey)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}
	wire, err := json.Marshal(map[string]string{
		"authorization": fmt.Sprintf("Bearer %s", signed),
		"data":          base64.RawURLEncoding.EncodeToString(payload),
	})
	if err != nil {
		t.Fatalf("marshal envelope: %v", err)
	}
	return wire
}
