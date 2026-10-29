package handoff

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
)

func TestDataPlaneCreateDocumentMintsOrgScopedBearerAndCachesIt(t *testing.T) {
	var mu sync.Mutex
	mints := map[string]int{}
	requests := map[string]int{}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/data-plane/internal-token":
			if r.Header.Get("X-Service-ID") != "integration-corev2" {
				t.Fatalf("X-Service-ID = %q", r.Header.Get("X-Service-ID"))
			}
			if r.Header.Get("X-Service-API-Key") != "integration-key" {
				t.Fatalf("X-Service-API-Key missing")
			}
			var body struct {
				OrgID  string   `json:"orgId"`
				Scopes []string `json:"scopes"`
				Reason string   `json:"reason"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Fatalf("decode mint body: %v", err)
			}
			if len(body.Scopes) != 1 || body.Scopes[0] != "documents:write" || body.Reason == "" {
				t.Fatalf("mint body = %#v", body)
			}
			mu.Lock()
			mints[body.OrgID]++
			mu.Unlock()
			writeTokenBundle(t, w, "token-"+body.OrgID, time.Now().Add(5*time.Minute), 300)
		case "/v1/documents":
			var body DataPlaneDocumentRequest
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				t.Fatalf("decode document body: %v", err)
			}
			if got := r.Header.Get("Authorization"); got != "Bearer token-"+body.OrgID {
				t.Fatalf("Authorization = %q for org %q", got, body.OrgID)
			}
			for _, header := range []string{"X-Internal-Api-Key", "X-Api-Key", "X-Org-ID", "X-User-ID", "X-Service-API-Key"} {
				if got := r.Header.Get(header); got != "" {
					t.Fatalf("%s = %q, want absent", header, got)
				}
			}
			mu.Lock()
			requests[body.OrgID]++
			mu.Unlock()
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(fmt.Sprintf(`{"document_id":"doc-%s","org_id":"%s"}`, body.OrgID, body.OrgID)))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: server.URL, AuthCoreURL: server.URL, ServiceID: "integration-corev2",
		ServiceAPIKey: "integration-key", HTTPClient: server.Client(),
	})
	for _, orgID := range []string{"org-1", "org-1", "org-2"} {
		_, err := client.CreateDocument(context.Background(), DataPlaneDocumentRequest{
			OrgID: orgID, Source: "github", Type: "repository", Title: "repo", Content: "real content",
		})
		if err != nil {
			t.Fatalf("CreateDocument(%s): %v", orgID, err)
		}
	}
	if mints["org-1"] != 1 || mints["org-2"] != 1 {
		t.Fatalf("mints = %#v, want one per org", mints)
	}
	if requests["org-1"] != 2 || requests["org-2"] != 1 {
		t.Fatalf("requests = %#v", requests)
	}
}

func TestDataPlaneCreateDocumentRefreshesNearExpiry(t *testing.T) {
	now := time.Date(2026, time.July, 12, 12, 0, 0, 0, time.UTC)
	mints := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/data-plane/internal-token" {
			mints++
			writeTokenBundle(t, w, fmt.Sprintf("token-%d", mints), now.Add(5*time.Minute), 300)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"document_id":"doc-1"}`))
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: server.URL, AuthCoreURL: server.URL, ServiceID: "integration-corev2",
		ServiceAPIKey: "integration-key", HTTPClient: server.Client(), now: func() time.Time { return now },
	})
	input := DataPlaneDocumentRequest{OrgID: "org-1", Source: "github", Type: "repository", Title: "repo", Content: "real content"}
	if _, err := client.CreateDocument(context.Background(), input); err != nil {
		t.Fatal(err)
	}
	now = now.Add(4*time.Minute + 31*time.Second)
	if _, err := client.CreateDocument(context.Background(), input); err != nil {
		t.Fatal(err)
	}
	if mints != 2 {
		t.Fatalf("mints = %d, want refresh near expiry", mints)
	}
}

func TestDataPlaneTokenProviderSingleflightsConcurrentMintPerOrg(t *testing.T) {
	var mints int
	var mu sync.Mutex
	var startedOnce sync.Once
	started := make(chan struct{})
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		mu.Lock()
		mints++
		mu.Unlock()
		startedOnce.Do(func() { close(started) })
		<-release
		writeTokenBundle(t, w, "shared-token", time.Now().Add(5*time.Minute), 300)
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: "http://data.invalid", AuthCoreURL: server.URL, ServiceID: "integration-corev2",
		ServiceAPIKey: "integration-key", HTTPClient: server.Client(),
	})
	const callers = 32
	start := make(chan struct{})
	errs := make(chan error, callers)
	for range callers {
		go func() {
			<-start
			token, err := client.tokens.token(context.Background(), "org-1")
			if err == nil && token != "shared-token" {
				err = fmt.Errorf("token = %q", token)
			}
			errs <- err
		}()
	}
	close(start)
	<-started
	close(release)
	for range callers {
		if err := <-errs; err != nil {
			t.Fatal(err)
		}
	}
	mu.Lock()
	defer mu.Unlock()
	if mints != 1 {
		t.Fatalf("mints = %d, want one per-key in-flight mint", mints)
	}
}

func TestDataPlaneTokenProviderBoundsCacheAndConditionallyInvalidates(t *testing.T) {
	mints := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		mints++
		writeTokenBundle(t, w, fmt.Sprintf("token-%d", mints), time.Now().Add(5*time.Minute), 300)
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: "http://data.invalid", AuthCoreURL: server.URL, ServiceID: "integration-corev2",
		ServiceAPIKey: "integration-key", HTTPClient: server.Client(),
	})
	client.tokens.maxCacheEntries = 2
	oldToken, err := client.tokens.token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.tokens.token(context.Background(), "org-2"); err != nil {
		t.Fatal(err)
	}
	client.tokens.invalidate("org-1", oldToken)
	newToken, err := client.tokens.token(context.Background(), "org-1")
	if err != nil {
		t.Fatal(err)
	}
	client.tokens.invalidate("org-1", oldToken)
	if got, err := client.tokens.token(context.Background(), "org-1"); err != nil || got != newToken {
		t.Fatalf("stale invalidation evicted newer token: token=%q err=%v", got, err)
	}
	if _, err := client.tokens.token(context.Background(), "org-3"); err != nil {
		t.Fatal(err)
	}
	client.tokens.mu.Lock()
	defer client.tokens.mu.Unlock()
	if len(client.tokens.cache) != 2 {
		t.Fatalf("cache entries = %d, want bounded capacity 2", len(client.tokens.cache))
	}
	if _, ok := client.tokens.cache["org-2"]; ok {
		t.Fatal("least recently used org-2 entry was not evicted")
	}
}

func TestValidateTokenBundleRejectsInvalidIssuerContracts(t *testing.T) {
	now := time.Date(2026, time.July, 12, 12, 0, 0, 0, time.UTC)
	valid := dataPlaneTokenBundle{
		Token: "token", Issuer: "issuer", Audience: dataPlaneAudience,
		ExpiresAt: now.Add(5 * time.Minute).Format(time.RFC3339), ExpiresInSeconds: 300,
	}
	tests := []struct {
		name   string
		mutate func(dataPlaneTokenBundle) dataPlaneTokenBundle
	}{
		{name: "missing token", mutate: func(bundle dataPlaneTokenBundle) dataPlaneTokenBundle { bundle.Token = ""; return bundle }},
		{name: "wrong audience", mutate: func(bundle dataPlaneTokenBundle) dataPlaneTokenBundle { bundle.Audience = "model"; return bundle }},
		{name: "excessive lifetime", mutate: func(bundle dataPlaneTokenBundle) dataPlaneTokenBundle { bundle.ExpiresInSeconds = 601; return bundle }},
		{name: "invalid expiry", mutate: func(bundle dataPlaneTokenBundle) dataPlaneTokenBundle { bundle.ExpiresAt = "invalid"; return bundle }},
		{name: "expired", mutate: func(bundle dataPlaneTokenBundle) dataPlaneTokenBundle {
			bundle.ExpiresAt = now.Add(-time.Second).Format(time.RFC3339)
			return bundle
		}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := validateTokenBundle(tt.mutate(valid), now); err == nil {
				t.Fatal("invalid token bundle was accepted")
			}
		})
	}
}

func TestDataPlaneCreateDocumentRetriesOnceOnUnauthorized(t *testing.T) {
	mints := 0
	documentRequests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/data-plane/internal-token" {
			mints++
			writeTokenBundle(t, w, fmt.Sprintf("token-%d", mints), time.Now().Add(5*time.Minute), 300)
			return
		}
		documentRequests++
		if documentRequests == 1 {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		if got := r.Header.Get("Authorization"); got != "Bearer token-2" {
			t.Fatalf("retry Authorization = %q", got)
		}
		_, _ = w.Write([]byte(`{"document_id":"doc-1"}`))
	}))
	defer server.Close()

	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: server.URL, AuthCoreURL: server.URL, ServiceID: "integration-corev2",
		ServiceAPIKey: "integration-key", HTTPClient: server.Client(),
	})
	_, err := client.CreateDocument(context.Background(), DataPlaneDocumentRequest{
		OrgID: "org-1", Source: "github", Type: "repository", Title: "repo", Content: "real content",
	})
	if err != nil {
		t.Fatalf("CreateDocument: %v", err)
	}
	if mints != 2 || documentRequests != 2 {
		t.Fatalf("mints/requests = %d/%d, want 2/2", mints, documentRequests)
	}
}

func TestDataPlaneTokenMintRefusesRedirectWithoutForwardingCredential(t *testing.T) {
	forwardedCredential := ""
	redirectTarget := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		forwardedCredential = r.Header.Get("X-Service-API-Key")
		w.WriteHeader(http.StatusOK)
	}))
	defer redirectTarget.Close()
	issuer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, redirectTarget.URL, http.StatusTemporaryRedirect)
	}))
	defer issuer.Close()

	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: "http://data.invalid", AuthCoreURL: issuer.URL,
		ServiceID: "integration-corev2", ServiceAPIKey: "must-not-cross-redirect", HTTPClient: issuer.Client(),
	})
	_, err := client.CreateDocument(context.Background(), DataPlaneDocumentRequest{OrgID: "org-1", Content: "real content"})
	if err == nil {
		t.Fatal("redirected token issuance unexpectedly succeeded")
	}
	if forwardedCredential != "" {
		t.Fatal("durable service credential was forwarded across a redirect")
	}
}

func TestDataPlaneCreateDocumentReturnsErrNotConfiguredOrUnscoped(t *testing.T) {
	tests := []struct {
		name   string
		client *DataPlaneDocumentsClient
		orgID  string
	}{
		{name: "nil client", client: nil, orgID: "org-1"},
		{name: "missing base URL", client: NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{AuthCoreURL: "http://auth", ServiceID: "integration-corev2", ServiceAPIKey: "key"}), orgID: "org-1"},
		{name: "missing auth core", client: NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{BaseURL: "http://data", ServiceID: "integration-corev2", ServiceAPIKey: "key"}), orgID: "org-1"},
		{name: "missing service key", client: NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{BaseURL: "http://data", AuthCoreURL: "http://auth", ServiceID: "integration-corev2"}), orgID: "org-1"},
		{name: "missing verified org", client: NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{BaseURL: "http://data", AuthCoreURL: "http://auth", ServiceID: "integration-corev2", ServiceAPIKey: "key"})},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := tt.client.CreateDocument(context.Background(), DataPlaneDocumentRequest{OrgID: tt.orgID, Content: "real content"})
			if !errors.Is(err, ErrNotConfigured) && tt.orgID != "" {
				t.Fatalf("CreateDocument error = %v, want ErrNotConfigured", err)
			}
			if tt.orgID == "" && err == nil {
				t.Fatal("empty org must fail closed")
			}
		})
	}
}

func TestDataPlaneDocumentsClientConfigured(t *testing.T) {
	client := NewDataPlaneDocumentsClient(DataPlaneDocumentsConfig{
		BaseURL: "http://data", AuthCoreURL: "http://auth", ServiceID: "integration-corev2", ServiceAPIKey: "key",
	})
	if !client.Configured() {
		t.Fatal("fully configured client reported false")
	}
}

func TestNewDataPlaneDocumentsClientFromConfigMapsServiceCredential(t *testing.T) {
	client := NewDataPlaneDocumentsClientFromConfig(config.Config{
		DataPlaneDocumentsURL: "http://data", AuthCoreURL: "http://auth",
		IntegrationServiceID: "integration-corev2", IntegrationServiceAPIKey: "key",
	}, nil)
	if !client.Configured() || client.tokens.serviceID != "integration-corev2" {
		t.Fatalf("client = %#v", client)
	}
}

func writeTokenBundle(t *testing.T, w http.ResponseWriter, token string, expiresAt time.Time, expiresIn int) {
	t.Helper()
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]any{
		"token": token, "expiresAt": expiresAt.Format(time.RFC3339), "expiresInSeconds": expiresIn,
		"issuer": "http://auth-core/api/convex-auth", "audience": "data-plane",
	}); err != nil {
		t.Fatalf("encode token bundle: %v", err)
	}
}
