package server

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	bridgeauth "github.com/triodelab/model-plane/services/bridge-core/internal/authz"
	"github.com/triodelab/model-plane/services/bridge-core/internal/channel"
	"github.com/triodelab/model-plane/services/bridge-core/internal/delivery"
	"github.com/triodelab/model-plane/services/bridge-core/internal/session"
)

type bridgeTestClaims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id,omitempty"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes,omitempty"`
	jwt.RegisteredClaims
}

type bridgeAuthFixture struct {
	key      *rsa.PrivateKey
	verifier *authctx.Verifier
}

func newBridgeAuthFixture(t *testing.T) *bridgeAuthFixture {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{Audiences: []string{bridgeauth.Audience}, Issuer: "https://auth.test/issuer", PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der})})
	if err != nil {
		t.Fatal(err)
	}
	return &bridgeAuthFixture{key: key, verifier: verifier}
}

func (f *bridgeAuthFixture) token(t *testing.T, audience, org, actor, principalType string, scopes ...string) string {
	t.Helper()
	claims := bridgeTestClaims{OrgID: org, PrincipalType: principalType, Scopes: scopes, RegisteredClaims: jwt.RegisteredClaims{Audience: jwt.ClaimStrings{audience}, Issuer: "https://auth.test/issuer", Subject: actor, IssuedAt: jwt.NewNumericDate(time.Now().Add(-time.Minute)), NotBefore: jwt.NewNumericDate(time.Now().Add(-time.Minute)), ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour))}}
	if principalType == "service" {
		claims.ServiceID = actor
	} else {
		claims.UserID = actor
	}
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(f.key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func doRequest(t *testing.T, client *http.Client, method, url, bearer string, body io.Reader) *http.Response {
	t.Helper()
	req, err := http.NewRequest(method, url, body)
	if err != nil {
		t.Fatal(err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if bearer != "" {
		req.Header.Set("Authorization", "Bearer "+bearer)
	}
	response, err := client.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	return response
}

// newTestServer builds a Server with a webhook adapter on the "web" channel
// backed by an in-memory outbox, and returns the server, store, and an httptest
// server.
func newTestServer(t *testing.T) (*httptest.Server, delivery.Store, *bridgeAuthFixture) {
	t.Helper()
	reg := session.NewRegistry()
	adapters := channel.NewAdapterRegistry()
	store := delivery.NewMemoryStore()
	wa, err := channel.NewWebhookAdapter(channel.WebhookConfig{
		ChannelName: "web", Destination: "http://example.invalid/hook", MaxAttempts: 3,
	}, store)
	if err != nil {
		t.Fatal(err)
	}
	adapters.Register("web", wa)
	srv := NewServer(reg, adapters)
	auth := newBridgeAuthFixture(t)
	ts := httptest.NewServer(srv.Handler(auth.verifier))
	t.Cleanup(ts.Close)
	return ts, store, auth
}

func TestIngest_RegisterThenIngestEnqueuesDelivery(t *testing.T) {
	ts, store, auth := newTestServer(t)
	bearer := auth.token(t, bridgeauth.Audience, "o1", "u1", "user")

	// Register a "web" session.
	regBody, _ := json.Marshal(map[string]string{"org_id": "o1", "user_id": "u1", "channel": "web"})
	resp := doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions", bearer, bytes.NewReader(regBody))
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("register status: %d", resp.StatusCode)
	}
	var sess session.Session
	_ = json.NewDecoder(resp.Body).Decode(&sess)
	_ = resp.Body.Close()
	if sess.ID == "" {
		t.Fatal("no session id returned")
	}

	// Ingest a payload (Go []byte JSON = base64 string).
	ingBody, _ := json.Marshal(map[string][]byte{"payload": []byte(`{"prompt":"hello"}`)})
	resp2 := doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions/"+sess.ID+"/ingest", bearer, bytes.NewReader(ingBody))
	if resp2.StatusCode != http.StatusOK {
		t.Fatalf("ingest status: %d", resp2.StatusCode)
	}
	var ingResp struct {
		SessionID string `json:"session_id"`
		Delivered bool   `json:"delivered"`
	}
	_ = json.NewDecoder(resp2.Body).Decode(&ingResp)
	_ = resp2.Body.Close()

	if ingResp.SessionID != sess.ID {
		t.Fatalf("ingest echoed wrong session: %s", ingResp.SessionID)
	}
	if !ingResp.Delivered {
		t.Fatal("expected delivered=true (enqueue succeeded)")
	}

	// The webhook adapter should have enqueued exactly one durable delivery.
	n, _ := store.PendingCount(context.Background())
	if n != 1 {
		t.Fatalf("expected 1 pending outbox record, got %d", n)
	}
}

func TestIngest_UnknownSession404(t *testing.T) {
	ts, _, auth := newTestServer(t)
	ingBody, _ := json.Marshal(map[string][]byte{"payload": []byte("x")})
	bearer := auth.token(t, bridgeauth.Audience, "o1", "u1", "user")
	resp := doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions/nope/ingest", bearer, bytes.NewReader(ingBody))
	defer func() { _ = resp.Body.Close() }()
	if resp.StatusCode != http.StatusNotFound {
		t.Fatalf("expected 404, got %d", resp.StatusCode)
	}
}

func TestSensitiveRoutesRequireAuthWhileHealthRemainsPublic(t *testing.T) {
	ts, _, auth := newTestServer(t)
	for _, path := range []string{"/healthz", "/readyz"} {
		response := doRequest(t, ts.Client(), http.MethodGet, ts.URL+path, "", nil)
		if response.StatusCode != http.StatusOK {
			t.Fatalf("%s status = %d", path, response.StatusCode)
		}
		_ = response.Body.Close()
	}

	for _, tc := range []struct{ name, bearer string }{
		{name: "missing"},
		{name: "malformed", bearer: "not-a-jwt"},
		{name: "wrong audience", bearer: auth.token(t, "model-gateway", "o1", "u1", "user")},
	} {
		t.Run(tc.name, func(t *testing.T) {
			response := doRequest(t, ts.Client(), http.MethodGet, ts.URL+"/api/v1/sessions", tc.bearer, nil)
			defer response.Body.Close()
			if response.StatusCode != http.StatusUnauthorized {
				t.Fatalf("status = %d, want 401", response.StatusCode)
			}
		})
	}
}

func TestSessionIdentityComesFromClaimsAndByIDAccessIsContained(t *testing.T) {
	ts, _, auth := newTestServer(t)
	ownerToken := auth.token(t, bridgeauth.Audience, "o1", "u1", "user")

	conflicting, _ := json.Marshal(map[string]string{"org_id": "o2", "user_id": "u2", "channel": "web"})
	response := doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions", ownerToken, bytes.NewReader(conflicting))
	if response.StatusCode != http.StatusForbidden {
		t.Fatalf("conflicting identity status = %d, want 403", response.StatusCode)
	}
	_ = response.Body.Close()

	matching, _ := json.Marshal(map[string]string{"org_id": "o1", "user_id": "u1", "channel": "web"})
	response = doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions", ownerToken, bytes.NewReader(matching))
	if response.StatusCode != http.StatusCreated {
		t.Fatalf("create status = %d", response.StatusCode)
	}
	var created session.Session
	if err := json.NewDecoder(response.Body).Decode(&created); err != nil {
		t.Fatal(err)
	}
	_ = response.Body.Close()
	if created.OrgID != "o1" || created.UserID != "u1" {
		t.Fatalf("created identity = %#v", created)
	}

	ownerGet := doRequest(t, ts.Client(), http.MethodGet, ts.URL+"/api/v1/sessions/"+created.ID, ownerToken, nil)
	if ownerGet.StatusCode != http.StatusOK {
		t.Fatalf("owner get status = %d", ownerGet.StatusCode)
	}
	_ = ownerGet.Body.Close()
	ownerList := doRequest(t, ts.Client(), http.MethodGet, ts.URL+"/api/v1/sessions", ownerToken, nil)
	if ownerList.StatusCode != http.StatusOK {
		t.Fatalf("owner list status = %d", ownerList.StatusCode)
	}
	var listed []session.Session
	if err := json.NewDecoder(ownerList.Body).Decode(&listed); err != nil {
		t.Fatal(err)
	}
	_ = ownerList.Body.Close()
	if len(listed) != 1 || listed[0].ID != created.ID {
		t.Fatalf("listed = %#v", listed)
	}

	for _, tc := range []struct{ name, token string }{
		{name: "wrong tenant", token: auth.token(t, bridgeauth.Audience, "o2", "u1", "user")},
		{name: "wrong user", token: auth.token(t, bridgeauth.Audience, "o1", "u2", "user")},
	} {
		t.Run(tc.name, func(t *testing.T) {
			response := doRequest(t, ts.Client(), http.MethodGet, ts.URL+"/api/v1/sessions/"+created.ID, tc.token, nil)
			defer response.Body.Close()
			if response.StatusCode != http.StatusNotFound {
				t.Fatalf("get status = %d, want 404", response.StatusCode)
			}
		})
	}

	queryMismatch := doRequest(t, ts.Client(), http.MethodGet, ts.URL+"/api/v1/sessions?org_id=o2", ownerToken, nil)
	defer queryMismatch.Body.Close()
	if queryMismatch.StatusCode != http.StatusForbidden {
		t.Fatalf("query mismatch status = %d, want 403", queryMismatch.StatusCode)
	}

	invalidIngest := doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions/"+created.ID+"/ingest", ownerToken, bytes.NewBufferString("{"))
	if invalidIngest.StatusCode != http.StatusBadRequest {
		t.Fatalf("invalid ingest status = %d", invalidIngest.StatusCode)
	}
	_ = invalidIngest.Body.Close()

	closed := doRequest(t, ts.Client(), http.MethodDelete, ts.URL+"/api/v1/sessions/"+created.ID, ownerToken, nil)
	if closed.StatusCode != http.StatusOK {
		t.Fatalf("close status = %d", closed.StatusCode)
	}
	_ = closed.Body.Close()
	closedAgain := doRequest(t, ts.Client(), http.MethodDelete, ts.URL+"/api/v1/sessions/"+created.ID, ownerToken, nil)
	if closedAgain.StatusCode != http.StatusConflict {
		t.Fatalf("second close status = %d", closedAgain.StatusCode)
	}
	_ = closedAgain.Body.Close()
}

func TestRegisterRejectsMalformedBodyAndUnsupportedChannel(t *testing.T) {
	ts, _, auth := newTestServer(t)
	bearer := auth.token(t, bridgeauth.Audience, "o1", "u1", "user")
	for _, tc := range []struct{ name, body string }{
		{name: "malformed", body: "{"},
		{name: "unsupported channel", body: `{"channel":"irc"}`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			response := doRequest(t, ts.Client(), http.MethodPost, ts.URL+"/api/v1/sessions", bearer, bytes.NewBufferString(tc.body))
			defer response.Body.Close()
			if response.StatusCode != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400", response.StatusCode)
			}
		})
	}
}
