package siteintegrations

import (
	"encoding/json"
	"net/http"
	"net/url"
	"sync/atomic"
	"testing"
	"time"
)

func TestGoogleConsentUsesExactDeploymentDestinationsAndRejectsCallerOverrides(t *testing.T) {
	const trustedReturn = "https://app.example.com/studio/pulse"
	const callback = "https://integrations.example.com/oauth/callback/google"
	var calls atomic.Int64
	fixture := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		var body struct {
			ReturnURL string   `json:"returnUrl"`
			Bundles   []string `json:"bundles"`
		}
		if r.Method != http.MethodPost || r.URL.Path != "/api/v1/providers/google/connect-session" || json.NewDecoder(r.Body).Decode(&body) != nil || body.ReturnURL != trustedReturn {
			t.Error("consent did not use exact deployment return URL")
		}
		if len(body.Bundles) != 2 || body.Bundles[0] != "search_console" || body.Bundles[1] != "analytics_readonly" {
			t.Error("consent bundles changed")
		}
		parsed, _ := url.Parse(validGoogleAuthorization())
		query := parsed.Query()
		query.Set("redirect_uri", callback)
		parsed.RawQuery = query.Encode()
		writeFixture(w, map[string]any{"success": true, "data": map[string]string{"sessionToken": "session-123", "authorizationUrl": parsed.String(), "expiresAt": time.Now().Add(time.Minute).UTC().Format(time.RFC3339)}})
	})
	cfg := fixture.cfg
	cfg.GoogleConsentReturnURL = trustedReturn
	cfg.GoogleOAuthCallbackURL = callback
	broker := New(cfg)
	for _, raw := range []string{"", "http://localhost:5173/studio/pulse", trustedReturn + "?next=evil", trustedReturn + "/", "https://foreign.example.com/studio/pulse"} {
		if _, err := broker.BeginGoogle(t.Context(), raw); err == nil {
			t.Fatal("caller return override accepted")
		}
	}
	if calls.Load() != 0 {
		t.Fatal("invalid caller return reached upstream")
	}
	if session, err := broker.BeginGoogle(t.Context(), trustedReturn); err != nil || session.SessionID != "session-123" || calls.Load() != 1 {
		t.Fatalf("valid configured consent rejected: %v", err)
	}
}

func TestGoogleConsentRejectsMalformedDeploymentConfigWithoutUpstreamRequest(t *testing.T) {
	var calls atomic.Int64
	fixture := fixtureBroker(t, func(http.ResponseWriter, *http.Request) { calls.Add(1) })
	for _, callback := range []bool{false, true} {
		cfg := fixture.cfg
		cfg.GoogleConsentReturnURL = "https://app.example.com/studio/pulse"
		if callback {
			cfg.GoogleOAuthCallbackURL = "http://foreign.example.com/oauth/callback/google"
		} else {
			cfg.GoogleConsentReturnURL += "?next=evil"
		}
		if _, err := New(cfg).BeginGoogle(t.Context(), cfg.GoogleConsentReturnURL); err == nil {
			t.Fatal("unsafe deployment destination accepted")
		}
	}
	if calls.Load() != 0 {
		t.Fatal("unsafe deployment consent reached upstream")
	}
}

func TestGoogleConsentRejectsCallbackDifferentFromExactConfiguredValue(t *testing.T) {
	fixture := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
		writeFixture(w, map[string]any{"success": true, "data": map[string]string{"sessionToken": "session-123", "authorizationUrl": validGoogleAuthorization(), "expiresAt": time.Now().Add(time.Minute).UTC().Format(time.RFC3339)}})
	})
	cfg := fixture.cfg
	cfg.GoogleOAuthCallbackURL = "https://integrations.example.com/oauth/callback/google"
	if session, err := New(cfg).BeginGoogle(t.Context(), "http://localhost:5173/studio/pulse"); err == nil || session.AuthorizationURL != "" {
		t.Fatal("upstream response with foreign callback accepted")
	}
}
