package workspace

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const syntheticBearer = "synthetic.eyJvcmdfaWQiOiJvcmctb25lIn0.signature"

func workspaceRegistry(t *testing.T, discover websites.Discoverer) *websites.Service {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	return websites.NewService(repo, discover, nil)
}

func TestWorkspaceUsesPerSiteMappingsWithoutChangingSharedConfig(t *testing.T) {
	var calls atomic.Int64
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if r.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("missing scoped bearer")
		}
		id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/api/v1/connections/"), "/actions")
		var action struct {
			Operation string         `json:"operation"`
			Params    map[string]any `json:"params"`
		}
		if err := json.NewDecoder(r.Body).Decode(&action); err != nil {
			t.Error(err)
		}
		host := strings.TrimPrefix(id, "conn-")
		var result any = map[string]any{"rows": []any{}}
		if action.Operation == "gsc.sites.list" {
			result = map[string]any{"siteEntry": []any{map[string]string{"siteUrl": "sc-domain:" + host, "permissionLevel": "siteOwner"}}}
		} else if action.Params["siteUrl"] != "sc-domain:"+host {
			t.Errorf("wrong site routing: %s %+v", id, action.Params)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": map[string]any{"action": map[string]any{"providerKey": "google", "operation": action.Operation, "result": result}}})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	registry := workspaceRegistry(t, func(_ context.Context, site string) websites.Evidence {
		host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-" + host, Property: "sc-domain:" + host, AccessLevel: "siteOwner"}, GA4: websites.Access{Status: "requires_consent"}}
	})
	for _, host := range []string{"one.example", "two.example"} {
		if _, _, err := registry.Add(t.Context(), "https://"+host+"/"); err != nil {
			t.Fatal(err)
		}
	}
	cfg := pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path, GoogleConnectionID: "legacy-other-account", GoogleGSCProperty: "sc-domain:legacy.example"}
	service := New(cfg, registry)
	if service.WebsiteRegistry() != registry {
		t.Fatal("wrong registry")
	}
	for _, host := range []string{"one.example", "two.example", "one.example"} {
		got := service.Get(t.Context(), "", "https://"+host+"/")
		if got.Status != pulse.StatusConnected || got.Totals == nil || got.Totals.Clicks != 0 {
			t.Fatalf("site %s: %+v", host, got)
		}
	}
	search := service.GetSearch(t.Context(), "", "https://two.example/")
	if search.Status != pulse.StatusConnected {
		t.Fatalf("search: %+v", search)
	}
	currentEnd := time.Now().UTC().AddDate(0, 0, -3)
	current := pulse.Period{StartDate: currentEnd.AddDate(0, 0, -27).Format("2006-01-02"), EndDate: currentEnd.Format("2006-01-02")}
	previous := pulse.Period{StartDate: currentEnd.AddDate(0, 0, -55).Format("2006-01-02"), EndDate: currentEnd.AddDate(0, 0, -28).Format("2006-01-02")}
	for _, host := range []string{"one.example", "two.example"} {
		comparison := service.GetSearchComparison(t.Context(), "https://"+host+"/", "query", current, previous, 100)
		if comparison.Status != pulse.StatusConnected || comparison.Site != "https://"+host+"/" || comparison.RowLimit != 100 {
			t.Fatalf("comparison for %s: %+v", host, comparison)
		}
	}
	before := calls.Load()
	if got := service.GetSearchComparison(t.Context(), "https://unregistered.example/", "page", current, previous, 100); got.Reason != pulse.ReasonNotConfigured || calls.Load() != before {
		t.Fatal("legacy comparison connection used for unregistered website")
	}
	if got := service.Get(t.Context(), "", "https://unregistered.example/"); got.Reason != pulse.ReasonNotConfigured || calls.Load() != before {
		t.Fatal("legacy connection used for unregistered website")
	}
	if service.cfg != cfg {
		t.Fatal("shared singleton configuration was changed")
	}
}

func TestWorkspacePublicPageSpeedDoesNotNeedPrivateSearchMapping(t *testing.T) {
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var site, strategy string
		if r.Method == http.MethodPost {
			var body map[string]string
			_ = json.NewDecoder(r.Body).Decode(&body)
			site, strategy = body["site"], body["strategy"]
		} else {
			site, strategy = r.URL.Query().Get("site"), r.URL.Query().Get("strategy")
		}
		status := "idle"
		if r.Method == http.MethodPost {
			status = "running"
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": pulse.PageSpeedProjection{Site: site, Strategy: strategy, Source: "google_pagespeed", Status: status}})
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	registry := workspaceRegistry(t, nil)
	if _, _, err := registry.Add(t.Context(), "https://example.com/"); err != nil {
		t.Fatal(err)
	}
	service := New(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}, registry)
	if got := service.GetPageSpeed(t.Context(), "https://example.com/", "mobile"); got.Status != "idle" {
		t.Fatalf("read: %+v", got)
	}
	if got := service.StartPageSpeed(t.Context(), "https://example.com/", "desktop"); got.Status != "running" {
		t.Fatalf("start: %+v", got)
	}
}

func TestOpenRegistryRejectsUnsafeStorageAndConstructsReadOnlyBroker(t *testing.T) {
	if _, err := OpenRegistry("relative.json", "org-one", pulse.Config{}); err == nil {
		t.Fatal("invalid storage accepted")
	}
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	registry, err := OpenRegistry(filepath.Join(dir, "sites.json"), "org-one", pulse.Config{})
	if err != nil {
		t.Fatal(err)
	}
	website, _, err := registry.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if website.Integrations.SearchConsole.Status != "source_unavailable" || website.Integrations.GA4.Status != "source_unavailable" {
		t.Fatal("unconfigured provider pretended connected")
	}
	if _, err := registry.ConnectGoogle(t.Context(), website.ID); err == nil {
		t.Fatal("unconfigured OAuth accepted")
	}
}

func TestOrganizationBindingIsFailClosedButNeverReplacesJWTAuthentication(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bearer")
	if boundToOrganization(path, "org-one") {
		t.Fatal("missing token accepted")
	}
	for _, value := range []string{"", "opaque-token", "header.invalid-base64!@.signature", "header." + base64.RawURLEncoding.EncodeToString([]byte(`{"org_id":"org-two"}`)) + ".signature", "header." + base64.RawURLEncoding.EncodeToString([]byte(`{"org_id":123}`)) + ".signature", strings.Repeat("x", 8193)} {
		if err := os.WriteFile(path, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
		if boundToOrganization(path, "org-one") {
			t.Fatal("invalid organization binding accepted")
		}
	}
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	if !boundToOrganization(path, "org-one") || boundToOrganization(path, "") {
		t.Fatal("binding assertion failed")
	}
	registry := workspaceRegistry(t, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-test", Property: "sc-domain:example.com", AccessLevel: "siteOwner"}, GA4: websites.Access{Status: "requires_consent"}}
	})
	if _, _, err := registry.Add(t.Context(), "https://example.com/"); err != nil {
		t.Fatal(err)
	}
	var calls atomic.Int64
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); w.WriteHeader(http.StatusUnauthorized) }))
	defer upstream.Close()
	service := New(pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path}, registry)
	if got := service.Get(t.Context(), "", "https://example.com/"); got.Reason != pulse.ReasonSourceUnavailable || calls.Load() != 1 {
		t.Fatal("local org assertion treated as JWT authentication")
	}
	if err := os.WriteFile(path, []byte("header."+base64.RawURLEncoding.EncodeToString([]byte(`{"org_id":"org-two"}`))+".signature"), 0600); err != nil {
		t.Fatal(err)
	}
	before := calls.Load()
	if got := service.Get(t.Context(), "", "https://example.com/"); got.Reason != pulse.ReasonNotConfigured || calls.Load() != before {
		t.Fatal("wrong-org bearer reached upstream")
	}
}

func TestOpenRegistryBindsDiscoveryAndInteractiveGoogleThroughBroker(t *testing.T) {
	const trustedReturn = "https://app.example.com/studio/pulse"
	const callback = "https://integrations.example.com/oauth/callback/google"
	query := url.Values{"client_id": {"synthetic.apps.googleusercontent.com"}, "response_type": {"code"}, "redirect_uri": {callback}, "scope": {"openid email profile https://www.googleapis.com/auth/webmasters.readonly https://www.googleapis.com/auth/analytics.readonly"}, "state": {strings.Repeat("s", 43)}, "code_challenge": {strings.Repeat("c", 43)}, "code_challenge_method": {"S256"}}
	authorization := "https://accounts.google.com/o/oauth2/v2/auth?" + query.Encode()
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer "+syntheticBearer {
			t.Error("missing fixed-org bearer")
		}
		var data any
		switch r.URL.Path {
		case "/api/v1/connections":
			data = map[string]any{"connections": []any{map[string]any{"id": "conn-google", "providerKey": "google", "status": "active", "capabilities": []string{"search_console.read"}, "scopes": []string{"https://www.googleapis.com/auth/webmasters.readonly"}}}}
		case "/api/v1/connections/conn-google/actions":
			data = map[string]any{"action": map[string]any{"providerKey": "google", "operation": "gsc.sites.list", "result": map[string]any{"siteEntry": []any{map[string]string{"siteUrl": "sc-domain:example.com", "permissionLevel": "siteOwner"}}}}}
		case "/api/v1/providers/google/connect-session":
			var body struct {
				ReturnURL string `json:"returnUrl"`
			}
			if json.NewDecoder(r.Body).Decode(&body) != nil || body.ReturnURL != trustedReturn {
				t.Error("workspace did not use configured trusted return URL")
			}
			data = map[string]string{"sessionToken": "session-google", "authorizationUrl": authorization, "expiresAt": time.Now().UTC().Add(10 * time.Minute).Format(time.RFC3339)}
		case "/api/v1/connect-sessions/session-google/status":
			data = map[string]string{"id": "session-google", "providerKey": "google", "status": "completed"}
		default:
			t.Errorf("unexpected broker request: %s", r.URL.Path)
			w.WriteHeader(500)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"success": true, "data": data})
	}))
	defer upstream.Close()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "bearer")
	if err := os.WriteFile(path, []byte(syntheticBearer), 0600); err != nil {
		t.Fatal(err)
	}
	registry, err := OpenRegistry(filepath.Join(dir, "sites.json"), "org-one", pulse.Config{IntegrationCoreURL: upstream.URL, IntegrationBearerTokenFile: path, GoogleConsentReturnURL: trustedReturn, GoogleOAuthCallbackURL: callback})
	if err != nil {
		t.Fatal(err)
	}
	website, _, err := registry.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if website.Integrations.SearchConsole.Status != "connected" || website.Ownership.Status != "google_owner" || website.Integrations.GA4.Status != "requires_consent" {
		t.Fatalf("wrong discovery: %+v", website)
	}
	session, err := registry.ConnectGoogle(t.Context(), website.ID)
	if err != nil {
		t.Fatal(err)
	}
	if status, err := registry.GoogleStatus(t.Context(), website.ID, session.SessionID); err != nil || status != "completed" {
		t.Fatalf("Google status: %s %v", status, err)
	}
	if err := os.WriteFile(path, []byte("header.e30.signature"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := registry.GoogleStatus(t.Context(), website.ID, session.SessionID); err == nil {
		t.Fatal("wrong-org status lookup accepted")
	}
	if _, err := registry.ConnectGoogle(t.Context(), website.ID); err == nil {
		t.Fatal("wrong-org consent accepted")
	}
}
