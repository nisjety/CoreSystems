package bearerrefresh

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func fixture(t *testing.T, handler http.HandlerFunc) (Config, *http.Client, time.Time) {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	directory := t.TempDir()
	if err := os.Chmod(directory, 0700); err != nil {
		t.Fatal(err)
	}
	key := filepath.Join(directory, "service-key")
	if err := os.WriteFile(key, []byte("synthetic-service-key-for-tests"), 0600); err != nil {
		t.Fatal(err)
	}
	return Config{AuthCoreURL: server.URL, OrganizationID: "org-aquatiq", ServiceKeyFile: key, TokenFile: filepath.Join(directory, "bearer")}, server.Client(), time.Date(2026, 10, 21, 10, 0, 0, 0, time.UTC)
}

func tokenFor(t *testing.T, now time.Time, org string) string {
	t.Helper()
	header, _ := json.Marshal(map[string]any{"alg": "RS256", "kid": "test-key"})
	claims, _ := json.Marshal(map[string]any{"iss": "http://localhost:3011/api/convex-auth", "aud": "ingestion", "sub": "service:marketing-core", "service_id": "service:marketing-core", "principal_type": "service", "org_id": org, "scopes": []string{"integration:read"}, "iat": now.Unix(), "nbf": now.Add(-5 * time.Second).Unix(), "exp": now.Add(300 * time.Second).Unix()})
	return base64.RawURLEncoding.EncodeToString(header) + "." + base64.RawURLEncoding.EncodeToString(claims) + ".synthetic-signature"
}

func writeBundle(t *testing.T, w http.ResponseWriter, now time.Time, org string) {
	t.Helper()
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]any{"token": tokenFor(t, now, org), "expiresAt": now.Add(300 * time.Second).Format(time.RFC3339), "expiresInSeconds": 300, "issuer": "http://localhost:3011/api/convex-auth", "audience": "ingestion"}); err != nil {
		t.Error(err)
	}
}

func TestRefreshWritesPrivateTokenAndExpiryState(t *testing.T) {
	var now time.Time
	cfg, client, fixed := fixture(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost || r.URL.Path != "/api/ingestion/internal-token" {
			t.Errorf("unexpected request %s %s", r.Method, r.URL.Path)
		}
		if r.Header.Get("x-service-id") != "marketing-core" || r.Header.Get("x-service-api-key") != "synthetic-service-key-for-tests" {
			t.Error("wrong service principal headers")
		}
		var body struct {
			OrgID  string   `json:"orgId"`
			Scopes []string `json:"scopes"`
			Reason string   `json:"reason"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		if body.OrgID != "org-aquatiq" || len(body.Scopes) != 1 || body.Scopes[0] != "integration:read" || !strings.Contains(body.Reason, "Pulse") {
			t.Errorf("unexpected issuance body %+v", body)
		}
		writeBundle(t, w, now, "org-aquatiq")
	})
	now = fixed
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	state, err := refresher.Refresh(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !state.ExpiresAt.Equal(now.Add(300 * time.Second)) {
		t.Fatalf("wrong expiry %v", state)
	}
	contents, err := os.ReadFile(cfg.TokenFile)
	if err != nil || strings.TrimSpace(string(contents)) != tokenFor(t, now, "org-aquatiq") {
		t.Fatal("token was not written correctly")
	}
	info, err := os.Stat(cfg.TokenFile)
	if err != nil || info.Mode().Perm() != 0600 {
		t.Fatal("token is not private")
	}
	status, err := os.ReadFile(cfg.TokenFile + ".status.json")
	if err != nil || strings.Contains(string(status), "synthetic-signature") || strings.Contains(string(status), "service-key") {
		t.Fatal("status absent or leaks secrets")
	}
	if _, err := refresher.Check(); err != nil {
		t.Fatal(err)
	}
	now = now.Add(290 * time.Second)
	if _, err := refresher.Check(); err == nil {
		t.Fatal("nearly expired token considered healthy")
	}
	now = now.Add(11 * time.Second)
	if _, err := refresher.Check(); err == nil {
		t.Fatal("expired token considered ready")
	}
}

func TestRefreshRejectsMalformedAndOversizedResponses(t *testing.T) {
	for _, response := range []string{"{}", "not-json", strings.Repeat("x", 16385)} {
		t.Run(response[:min(len(response), 10)], func(t *testing.T) {
			cfg, client, now := fixture(t, func(w http.ResponseWriter, r *http.Request) { _, _ = w.Write([]byte(response)) })
			refresher, err := New(cfg, client, func() time.Time { return now })
			if err != nil {
				t.Fatal(err)
			}
			if _, err := refresher.Refresh(context.Background()); err == nil {
				t.Fatal("invalid upstream response accepted")
			}
		})
	}
}

func TestRefreshRefusesSymlinkOutputAndInvalidKeyContent(t *testing.T) {
	var now time.Time
	cfg, client, fixed := fixture(t, func(w http.ResponseWriter, r *http.Request) { writeBundle(t, w, now, "org-aquatiq") })
	now = fixed
	if err := os.Symlink(cfg.ServiceKeyFile, cfg.TokenFile); err != nil {
		t.Fatal(err)
	}
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	if _, err := refresher.Refresh(context.Background()); err == nil {
		t.Fatal("symlink output accepted")
	}
	if err := os.WriteFile(cfg.ServiceKeyFile, []byte("invalid key content with spaces"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := refresher.Refresh(context.Background()); err == nil {
		t.Fatal("invalid key content accepted")
	}
}

func TestRunAndRequestCancellation(t *testing.T) {
	cfg, client, now := fixture(t, func(w http.ResponseWriter, r *http.Request) {})
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := refresher.Run(ctx, nil); err != context.Canceled {
		t.Fatalf("Run cancellation = %v", err)
	}
	if _, err := refresher.Refresh(ctx); err == nil {
		t.Fatal("canceled issuance accepted")
	}
	if err := waitContext(ctx, time.Hour); err != context.Canceled {
		t.Fatalf("wait cancellation = %v", err)
	}
	if err := waitContext(context.Background(), 0); err != nil {
		t.Fatal(err)
	}
}

func TestCheckRejectsAbsentMalformedAndUnsafeToken(t *testing.T) {
	cfg, client, now := fixture(t, func(w http.ResponseWriter, r *http.Request) {})
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	if _, err := refresher.Check(); err == nil {
		t.Fatal("absent token considered ready")
	}
	for _, token := range []string{"", "not-a-jwt", "%%%%.%%%%.sig", "eyJhbGciOiJub25lIn0.e30.sig", "eyJhbGciOiJSUzI1NiIsImtpZCI6IngifQ.%%%%.sig", "eyJhbGciOiJSUzI1NiIsImtpZCI6IngifQ.e30.sig"} {
		if err := os.WriteFile(cfg.TokenFile, []byte(token), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := refresher.Check(); err == nil {
			t.Fatal("invalid token considered ready")
		}
	}
	if err := os.Chmod(cfg.TokenFile, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := refresher.Check(); err == nil {
		t.Fatal("unsafe token considered ready")
	}
}

func TestRefreshRefusesRedirectAndPreservesExistingToken(t *testing.T) {
	var destinationCalls int
	destination := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { destinationCalls++ }))
	defer destination.Close()
	cfg, client, now := fixture(t, func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, destination.URL, http.StatusTemporaryRedirect)
	})
	if err := os.WriteFile(cfg.TokenFile, []byte("existing"), 0600); err != nil {
		t.Fatal(err)
	}
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	if _, err = refresher.Refresh(context.Background()); err == nil {
		t.Fatal("redirect accepted")
	}
	contents, _ := os.ReadFile(cfg.TokenFile)
	if string(contents) != "existing" || destinationCalls != 0 {
		t.Fatal("redirect followed or existing token replaced")
	}
}

func TestRefreshRejectsWrongTenantAndSanitizesErrors(t *testing.T) {
	var now time.Time
	cfg, client, fixed := fixture(t, func(w http.ResponseWriter, r *http.Request) { writeBundle(t, w, now, "another-org") })
	now = fixed
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	if _, err = refresher.Refresh(context.Background()); err == nil || strings.Contains(err.Error(), "synthetic") {
		t.Fatal("invalid token accepted or error leaks secret")
	}
	if _, err = os.Stat(cfg.TokenFile); !os.IsNotExist(err) {
		t.Fatal("invalid token persisted")
	}
}

func TestConfigRejectsUnsafeDestinationsAndFiles(t *testing.T) {
	cfg, client, now := fixture(t, func(w http.ResponseWriter, r *http.Request) {})
	for _, endpoint := range []string{"http://example.com", "http://auth-core:3000", "https://user:secret@example.com", "https://example.com/path", "https://example.com?q=1", "https://example.com#fragment", "file:///tmp/auth"} {
		t.Run(endpoint, func(t *testing.T) {
			candidate := cfg
			candidate.AuthCoreURL = endpoint
			if _, err := New(candidate, client, func() time.Time { return now }); err == nil {
				t.Fatal("unsafe destination accepted")
			}
		})
	}
	if err := os.Chmod(cfg.ServiceKeyFile, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := New(cfg, client, func() time.Time { return now }); err == nil {
		t.Fatal("world-readable key accepted")
	}
	if err := os.Chmod(cfg.ServiceKeyFile, 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(filepath.Dir(cfg.TokenFile), 0755); err != nil {
		t.Fatal(err)
	}
	if _, err := New(cfg, client, func() time.Time { return now }); err == nil {
		t.Fatal("public token directory accepted")
	}
}

func TestRunUsesExpiryScheduleAndBoundedRetries(t *testing.T) {
	var now time.Time
	var calls int
	cfg, client, fixed := fixture(t, func(w http.ResponseWriter, r *http.Request) {
		calls++
		if calls == 1 {
			writeBundle(t, w, now, "org-aquatiq")
			return
		}
		http.Error(w, "synthetic-service-key-for-tests", http.StatusServiceUnavailable)
	})
	now = fixed
	refresher, err := New(cfg, client, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	var delays []time.Duration
	refresher.wait = func(ctx context.Context, delay time.Duration) error {
		delays = append(delays, delay)
		now = now.Add(delay)
		return nil
	}
	if err := refresher.Run(context.Background(), nil); err == nil || strings.Contains(err.Error(), "synthetic") {
		t.Fatal("expected sanitized bounded retry failure")
	}
	if calls != 6 || len(delays) != 12 || delays[0] != 30*time.Second {
		t.Fatalf("calls=%d delays=%v", calls, delays)
	}
}

func TestRenewalWaitRechecksWallClockAfterForwardJump(t *testing.T) {
	_, client, fixed := fixture(t, func(w http.ResponseWriter, r *http.Request) {})
	r := &Refresher{client: client}
	now := fixed
	r.now = func() time.Time { return now }
	calls := 0
	r.wait = func(_ context.Context, d time.Duration) error {
		calls++
		if d > 30*time.Second {
			t.Fatal("long sleep ignored absolute expiry")
		}
		now = now.Add(10 * time.Minute)
		return nil
	}
	if err := r.waitUntil(t.Context(), now.Add(4*time.Minute)); err != nil || calls != 1 {
		t.Fatal("clock jump left renewal asleep", err, calls)
	}
}
