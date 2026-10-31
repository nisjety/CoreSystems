package siteintegrations

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestDiscoveryKeepsOwnerProofForExactHost(t *testing.T) {
	broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/connections" {
			writeFixture(w, googleConnections([]string{gscScope}, []string{"search_console.read"}))
			return
		}
		writeAction(w, "gsc.sites.list", map[string]any{"siteEntry": []any{map[string]string{"siteUrl": "https://aquatiq.com/", "permissionLevel": "siteRestrictedUser"}, map[string]string{"siteUrl": "sc-domain:aquatiq.com", "permissionLevel": "siteOwner"}}})
	})
	got := broker.Discover(t.Context(), "https://aquatiq.com/")
	if got.SearchConsole.AccessLevel != "siteOwner" || got.SearchConsole.Property != "sc-domain:aquatiq.com" {
		t.Fatalf("lostverifiedowner=%+v", got.SearchConsole)
	}
}

func TestGoogleStatusUsesOnlyKnownBoundSessionStates(t *testing.T) {
	for _, tc := range []struct{ upstream, expires, want string }{
		{"pending", time.Now().Add(time.Minute).UTC().Format(time.RFC3339), "pending"},
		{"pending", time.Now().Add(-time.Minute).UTC().Format(time.RFC3339), "expired"},
		{"completed", "", "connected"}, {"failed", "", "error"}, {"unexpected", "", ""}, {"pending", "invalid", ""},
	} {
		t.Run(tc.upstream+tc.want, func(t *testing.T) {
			broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
				writeFixture(w, map[string]any{"success": true, "data": map[string]string{"id": "session-123", "providerKey": "google", "status": tc.upstream, "expiresAt": tc.expires}})
			})
			got, err := broker.GoogleStatus(t.Context(), "session-123")
			if got != tc.want || (err != nil) != (tc.want == "") {
				t.Fatalf("status=%s err=%v", got, err)
			}
		})
	}
}

func TestGoogleAuthorizationRejectsScopeOrRedirectEscalation(t *testing.T) {
	if !validGoogleAuthorizationURL(validGoogleAuthorization()) {
		t.Fatal("validflowrejected")
	}
	for _, tc := range []struct{ key, value string }{
		{"redirect_uri", "https://evil.example/callback"}, {"code_challenge_method", "plain"}, {"code_challenge", "short"}, {"state", "short"}, {"client_id", "foreign"}, {"scope", gaScope + " https://www.googleapis.com/auth/analytics.edit"}, {"scope", gscScope},
	} {
		parsed, _ := url.Parse(validGoogleAuthorization())
		query := parsed.Query()
		query.Set(tc.key, tc.value)
		parsed.RawQuery = query.Encode()
		if validGoogleAuthorizationURL(parsed.String()) {
			t.Errorf("acceptedunsafe %s", tc.key)
		}
	}
	for _, raw := range []string{"https://accounts.google.com.evil.example/o/oauth2/v2/auth", validGoogleAuthorization() + "&state=duplicate", strings.Repeat("a", 8193)} {
		if validGoogleAuthorizationURL(raw) {
			t.Error("acceptedunsafedestination")
		}
	}
}

func TestBrokerRejectsUntrustedOrMissingBearerBeforeRequest(t *testing.T) {
	for _, cfg := range []Config{{}, {IntegrationCoreURL: "https://user:pass@provider.example", BearerTokenFile: "missing"}, {IntegrationCoreURL: "http://unapproved.example", BearerTokenFile: "missing"}} {
		if err := New(cfg).request(t.Context(), "GET", "/anything", nil, &struct{}{}); err == nil {
			t.Fatal("acceptedunsafe brokerconfiguration")
		}
	}
	for _, value := range []string{"", "has space", "has\nline", strings.Repeat("x", 8193)} {
		path := filepath.Join(t.TempDir(), "bearer")
		if err := os.WriteFile(path, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
		if err := New(Config{IntegrationCoreURL: "http://127.0.0.1:1", BearerTokenFile: path}).request(context.Background(), "GET", "/anything", nil, &struct{}{}); err == nil {
			t.Fatal("acceptedinvalid bearer")
		}
	}
}

func TestDiscoveryDoesNotConfuseAnotherHostOrMalformedResource(t *testing.T) {
	for _, tc := range []struct{ uri, name, next, want string }{
		{"https://blog.aquatiq.com/", "properties/123/dataStreams/456", "", "not_found"},
		{"https://aquatiq.com/", "properties/999/dataStreams/456", "", "source_unavailable"},
		{"https://aquatiq.com/", "properties/123/dataStreams/456", "continue", "needs_selection"},
	} {
		t.Run(tc.want, func(t *testing.T) {
			broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
				if r.URL.Path == "/api/v1/connections" {
					writeFixture(w, googleConnections([]string{gaScope}, []string{"analytics.read"}))
					return
				}
				var action struct {
					Operation string `json:"operation"`
				}
				_ = json.NewDecoder(r.Body).Decode(&action)
				if action.Operation == "ga4.account_summaries.list" {
					writeAction(w, action.Operation, map[string]any{"accountSummaries": []any{map[string]any{"propertySummaries": []any{map[string]string{"property": "properties/123"}}}}})
					return
				}
				result := map[string]any{"dataStreams": []any{map[string]any{"name": tc.name, "type": "WEB_DATA_STREAM", "webStreamData": map[string]string{"defaultUri": tc.uri, "measurementId": "G-ABC123"}}}}
				if tc.next != "" {
					result["nextPageToken"] = tc.next
				}
				writeAction(w, action.Operation, result)
			})
			got := broker.Discover(t.Context(), "https://aquatiq.com/")
			// Repeating an upstream page token is malformed, not safe truncation.
			want := tc.want
			if tc.next != "" {
				want = "source_unavailable"
			}
			if got.GA4.Status != want {
				t.Fatalf("GA4=%+v want=%s", got.GA4, want)
			}
		})
	}
}

func TestIdentifierValidation(t *testing.T) {
	for _, value := range []string{"", "../x", strings.Repeat("x", 129), "a b"} {
		if validID(value) {
			t.Errorf("accepted %q", value)
		}
	}
	for _, value := range []string{"", "123/4", strings.Repeat("1", 31)} {
		if numericID(value) {
			t.Errorf("acceptednumeric %q", value)
		}
	}
	for _, value := range []string{"", "G-abc", "G-", "G-ABC!", strings.Repeat("G", 33)} {
		if measurementID(value) {
			t.Errorf("acceptedmeasurement %q", value)
		}
	}
}
