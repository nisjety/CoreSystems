package oauth

import (
	"context"
	"errors"
	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type algoliaTransport func(*http.Request) (*http.Response, error)

func (f algoliaTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func TestAlgoliaPublicResourceBoundOAuth(t *testing.T) {
	path := filepath.Join(t.TempDir(), "client.json")
	_ = os.WriteFile(path, []byte(`{"version":1,"clientId":"our-client","redirectUri":"http://localhost:3026/api/v1/providers/algolia/callback","resource":"https://mcp.algolia.com/mcp"}`), 0600)
	cfg := config.Config{PublicBaseURL: "http://localhost:3026", AlgoliaMCPOAuthClientFile: path}
	client := NewAlgoliaOAuthClient(cfg, &http.Client{Transport: algoliaTransport(func(r *http.Request) (*http.Response, error) {
		if r.URL.String() != "https://dashboard.algolia.com/2/oauth/token" {
			t.Fatal("unfixed egress")
		}
		_ = r.ParseForm()
		if _, ok := r.Form["client_secret"]; ok {
			t.Fatal("public client sent secret")
		}
		if r.Form.Get("resource") != "https://mcp.algolia.com/mcp" || r.Form.Get("client_id") != "our-client" {
			t.Fatal("resource/client not pinned")
		}
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(`{"access_token":"provider-access","refresh_token":"provider-refresh","token_type":"Bearer","expires_in":3600,"scope":"public"}`)), Header: http.Header{}}, nil
	})})
	raw, err := client.AuthorizationURL("state", "http://localhost:3026/api/v1/providers/algolia/callback", "verifier", []string{"public"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	u, _ := url.Parse(raw)
	if u.Query().Get("scope") != "public" || u.Query().Get("resource") != "https://mcp.algolia.com/mcp" || u.Query().Get("code_challenge_method") != "S256" {
		t.Fatal("unsafe authorization")
	}
	token, err := client.ExchangeCode(context.Background(), "code", cfg.PublicBaseURL+"/api/v1/providers/algolia/callback", "verifier", nil, nil)
	if err != nil || token.AccessToken != "provider-access" || time.Until(token.ExpiresAt) < time.Minute {
		t.Fatalf("exchange: %v", err)
	}
	if _, err = client.Refresh(context.Background(), "refresh", nil, nil); err != nil {
		t.Fatal(err)
	}
	if _, err = client.AuthorizationURL("state", cfg.PublicBaseURL+"/wrong", "verifier", []string{"public"}, nil); err == nil {
		t.Fatal("redirect not pinned")
	}
	if _, err = client.AuthorizationURL("state", cfg.PublicBaseURL+"/api/v1/providers/algolia/callback", "verifier", []string{"keys:manage"}, nil); err == nil {
		t.Fatal("write scope accepted")
	}
}

func TestAlgoliaGrantsNeverOverwriteOrLease(t *testing.T) {
	repo := store.NewMemoryRepository()
	vault, _ := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	s := NewService(testOAuthConfig(), repo, vault, nil)
	session := store.ConnectSession{ID: "cs_one", ProviderKey: "algolia", ConnectorType: "algolia", OrganizationID: "org-1", UserID: "user-1", Scopes: []string{"public"}, Capabilities: []string{"search.analytics.read"}}
	token := TokenResult{AccessToken: "private-access", RefreshToken: "private-refresh", ExpiresAt: time.Now().Add(time.Hour)}
	one, err := s.persistConnection(context.Background(), session, token)
	if err != nil {
		t.Fatal(err)
	}
	session.ID = "cs_two"
	session.UserID = "other-user"
	two, err := s.persistConnection(context.Background(), session, token)
	if err != nil || one.ID == two.ID || one.ProviderAccountID != "" {
		t.Fatal("unidentified grant overwritten or fabricated")
	}
	if _, err = s.AccessTokenForConnection(context.Background(), one.ID); err != ErrResourceBoundToken {
		t.Fatal("generic connection lease allowed")
	}
	if _, err = s.AccessToken(context.Background(), "org-1", "algolia"); err != ErrResourceBoundToken {
		t.Fatal("generic connector lease allowed")
	}
	if _, err = s.AlgoliaAccessToken(context.Background(), "wrong-org", one.ID); err == nil {
		t.Fatal("cross-org internal access allowed")
	}
}

func TestAlgoliaTokenFailuresAreBoundedAndSanitized(t *testing.T) {
	path := filepath.Join(t.TempDir(), "client.json")
	_ = os.WriteFile(path, []byte(`{"version":1,"clientId":"owned-client","redirectUri":"http://localhost:3026/api/v1/providers/algolia/callback","resource":"https://mcp.algolia.com/mcp"}`), 0600)
	cfg := config.Config{PublicBaseURL: "http://localhost:3026", AlgoliaMCPOAuthClientFile: path}
	for _, tc := range []struct {
		body         string
		status       int
		invalidGrant bool
	}{
		{`{"error":"invalid_grant","error_description":"never-forward-this"}`, 400, true},
		{`{"error":"invalid_client","error_description":"never-forward-this"}`, 400, false},
		{`{"error":"provider_failure"}`, 503, false},
		{`{"access_token":"secret","token_type":"Bearer","expires_in":3600,"scope":"keys:manage"}`, 200, false},
		{`{"access_token":"one","access_token":"two"}`, 200, false},
		{strings.Repeat(" ", 65537), 200, false},
		{`not-json`, 200, false},
	} {
		client := NewAlgoliaOAuthClient(cfg, &http.Client{Transport: algoliaTransport(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: tc.status, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(tc.body))}, nil
		})})
		_, err := client.Refresh(context.Background(), "refresh", nil, nil)
		if err == nil || strings.Contains(err.Error(), "never-forward") || IsAuthorizationRefreshRequired(err) != tc.invalidGrant {
			t.Fatalf("unsafe token failure %v", err)
		}
	}
	client := NewAlgoliaOAuthClient(cfg, &http.Client{Transport: algoliaTransport(func(*http.Request) (*http.Response, error) { return nil, errors.New("provider-secret-in-url") })})
	if _, err := client.Refresh(context.Background(), "refresh", nil, nil); err != errAlgoliaOAuth {
		t.Fatal("network error leaked")
	}
	client = NewAlgoliaOAuthClient(config.Config{}, nil)
	if _, err := client.Refresh(context.Background(), "refresh", nil, nil); err == nil {
		t.Fatal("unconfigured client refreshed")
	}
	if _, err := client.ExchangeCode(context.Background(), "code", "wrong", "v", nil, nil); err == nil {
		t.Fatal("unconfigured client exchanged")
	}
	if err := client.Revoke(context.Background(), "token", nil); err != ErrRevocationUnsupported {
		t.Fatal("unverified revocation claimed")
	}
}

func TestAlgoliaInternalTokenChecksProtectedRegistration(t *testing.T) {
	path := filepath.Join(t.TempDir(), "client.json")
	_ = os.WriteFile(path, []byte(`{"version":1,"clientId":"owned-client","redirectUri":"http://localhost:3026/api/v1/providers/algolia/callback","resource":"https://mcp.algolia.com/mcp"}`), 0600)
	cfg := config.Config{PublicBaseURL: "http://localhost:3026", AlgoliaMCPOAuthClientFile: path}
	repo := store.NewMemoryRepository()
	vault, _ := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	s := NewService(cfg, repo, vault, nil)
	encrypted, _ := vault.Encrypt("protected-token", []byte("conn_one"))
	c := store.Connection{ID: "conn_one", OrganizationID: "org-1", ProviderKey: "algolia", Status: "active", EncryptedAccessToken: encrypted, AccessTokenExpiresAt: time.Now().Add(time.Hour), ProviderContext: map[string]string{"algolia_client_id": "owned-client", "algolia_resource": config.AlgoliaMCPResource}}
	_, _ = repo.UpsertConnection(context.Background(), c)
	if _, err := s.AlgoliaAccessToken(context.Background(), "org-1", "conn_one"); err != nil {
		t.Fatal(err)
	}
	c.ProviderContext = map[string]string{"algolia_client_id": "another-client", "algolia_resource": config.AlgoliaMCPResource}
	_, _ = repo.UpsertConnection(context.Background(), c)
	if _, err := s.AlgoliaAccessToken(context.Background(), "org-1", "conn_one"); err == nil {
		t.Fatal("foreign client grant reused")
	}
}

func TestAlgoliaObservedRejectionCannotPoisonRotatedCredential(t *testing.T) {
	repo := store.NewMemoryRepository()
	vault, _ := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
	s := NewService(testOAuthConfig(), repo, vault, nil)
	ctx := context.Background()
	encrypted, _ := vault.Encrypt("new-access", []byte("conn_one"))
	c := store.Connection{ID: "conn_one", OrganizationID: "org-1", ProviderKey: "algolia", Status: "active", EncryptedAccessToken: encrypted, AccessTokenExpiresAt: time.Now().Add(time.Hour)}
	_, _ = repo.UpsertConnection(ctx, c)
	_ = s.InvalidateAlgoliaAccessToken(ctx, "wrong-org", "conn_one", "new-access")
	got, _ := repo.GetConnection(ctx, "conn_one")
	if got.Status != "active" {
		t.Fatal("cross-org rejection changed grant")
	}
	_ = s.InvalidateAlgoliaAccessToken(ctx, "org-1", "conn_one", "old-access")
	got, _ = repo.GetConnection(ctx, "conn_one")
	if got.Status != "active" {
		t.Fatal("old failed bearer poisoned replacement")
	}
	_ = s.InvalidateAlgoliaAccessToken(ctx, "org-1", "conn_one", "new-access")
	got, _ = repo.GetConnection(ctx, "conn_one")
	if got.Status != "needs_refresh" {
		t.Fatal("confirmed rejected credential remains active")
	}
}

func TestAlgoliaRejectionRechecksAfterJoiningConcurrentRefresh(t *testing.T) {
	for _, nextToken := range []string{"same-rejected", "replacement"} {
		t.Run(nextToken, func(t *testing.T) {
			ctx := context.Background()
			repo := store.NewMemoryRepository()
			vault, _ := secretcrypto.NewVault([]byte("12345678901234567890123456789012"))
			s := NewService(testOAuthConfig(), repo, vault, nil)
			encrypted, _ := vault.Encrypt("same-rejected", []byte("conn_one"))
			connection := store.Connection{ID: "conn_one", OrganizationID: "org-1", ProviderKey: "algolia", Status: "active", EncryptedAccessToken: encrypted}
			_, _ = repo.UpsertConnection(ctx, connection)
			started, release, refreshDone := make(chan struct{}), make(chan struct{}), make(chan struct{})
			go func() {
				defer close(refreshDone)
				_, _, _ = s.refreshSingleflight(ctx, "conn_one", func(context.Context) (store.Connection, string, error) {
					close(started)
					<-release
					c, _ := repo.GetConnection(ctx, "conn_one")
					c.EncryptedAccessToken, _ = vault.Encrypt(nextToken, []byte(c.ID))
					saved, err := repo.UpdateConnectionCredentials(ctx, c)
					return saved, nextToken, err
				})
			}()
			<-started
			invalidated := make(chan error, 1)
			go func() { invalidated <- s.InvalidateAlgoliaAccessToken(ctx, "org-1", "conn_one", "same-rejected") }()
			// The blocked refresh keeps this invocation in its singleflight join path.
			time.Sleep(20 * time.Millisecond)
			close(release)
			<-refreshDone
			if err := <-invalidated; err != nil {
				t.Fatal(err)
			}
			got, _ := repo.GetConnection(ctx, "conn_one")
			want := "active"
			if nextToken == "same-rejected" {
				want = "needs_refresh"
			}
			if got.Status != want {
				t.Fatalf("joined refresh token %s left status %s want %s", nextToken, got.Status, want)
			}
		})
	}
}
