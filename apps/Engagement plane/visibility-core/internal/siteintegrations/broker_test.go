package siteintegrations

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const gscScope = "https://www.googleapis.com/auth/webmasters.readonly"
const gaScope = "https://www.googleapis.com/auth/analytics.readonly"

func fixtureBroker(t *testing.T, handler http.HandlerFunc) *Broker {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer synthetic-org-bearer" {
			t.Error("missing server bearer")
		}
		w.Header().Set("Content-Type", "application/json")
		handler(w, r)
	}))
	t.Cleanup(server.Close)
	path := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(path, []byte("synthetic-org-bearer"), 0600); err != nil {
		t.Fatal(err)
	}
	return New(Config{IntegrationCoreURL: server.URL, BearerTokenFile: path, PreferredGoogleConnectionID: "google-a"})
}

func googleConnections(scopes, capabilities []string) any {
	return map[string]any{"success": true, "data": map[string]any{"connections": []any{map[string]any{"id": "google-a", "providerKey": "google", "status": "active", "scopes": scopes, "capabilities": capabilities}}}}
}
func writeFixture(w http.ResponseWriter, value any) { _ = json.NewEncoder(w).Encode(value) }
func writeAction(w http.ResponseWriter, operation string, result any) {
	writeFixture(w, map[string]any{"success": true, "data": map[string]any{"action": map[string]any{"providerKey": "google", "operation": operation, "result": result}}})
}

func TestDiscoverMapsOnlyExactVerifiedProviderResources(t *testing.T) {
	broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/connections" {
			writeFixture(w, googleConnections([]string{gscScope, gaScope}, []string{"search_console.read", "analytics.read"}))
			return
		}
		var action struct {
			Operation string         `json:"operation"`
			Params    map[string]any `json:"params"`
		}
		_ = json.NewDecoder(r.Body).Decode(&action)
		switch action.Operation {
		case "gsc.sites.list":
			writeAction(w, action.Operation, map[string]any{"siteEntry": []any{map[string]string{"siteUrl": "sc-domain:example.com", "permissionLevel": "siteOwner"}, map[string]string{"siteUrl": "sc-domain:aquatiq.com", "permissionLevel": "siteFullUser"}}})
		case "ga4.account_summaries.list":
			writeAction(w, action.Operation, map[string]any{"accountSummaries": []any{map[string]any{"propertySummaries": []any{map[string]string{"property": "properties/123"}}}}})
		case "ga4.data_streams.list":
			if action.Params["propertyId"] != "123" {
				t.Error("invalid property selection")
			}
			writeAction(w, action.Operation, map[string]any{"dataStreams": []any{map[string]any{"name": "properties/123/dataStreams/456", "type": "WEB_DATA_STREAM", "webStreamData": map[string]string{"defaultUri": "https://aquatiq.com", "measurementId": "G-ABC123"}}}})
		case "ga4.reporting_host.resolve":
			writeAction(w, action.Operation, map[string]any{"reportingSite": "https://aquatiq.com/"})
		default:
			t.Errorf("unexpected action %s", action.Operation)
			w.WriteHeader(500)
		}
	})
	got := broker.Discover(context.Background(), "https://aquatiq.com/")
	if got.SearchConsole.Status != "connected" || got.SearchConsole.Property != "sc-domain:aquatiq.com" || got.SearchConsole.AccessLevel != "siteFullUser" || got.SearchConsole.ConnectionID != "google-a" {
		t.Fatalf("GSC=%+v", got.SearchConsole)
	}
	if got.GA4.Status != "connected" || got.GA4.PropertyID != "123" || got.GA4.StreamID != "456" || got.GA4.MeasurementID != "G-ABC123" {
		t.Fatalf("GA4=%+v", got.GA4)
	}
	if _, err := time.Parse(time.RFC3339, got.GA4.CheckedAt); err != nil {
		t.Fatal("missing verification time")
	}
}

func TestDiscoverDoesNotTurnReadAccessIntoOwnershipOrConsent(t *testing.T) {
	for _, permission := range []string{"siteOwner", "siteFullUser", "siteRestrictedUser", "siteUnverifiedUser"} {
		t.Run(permission, func(t *testing.T) {
			broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
				if r.URL.Path == "/api/v1/connections" {
					writeFixture(w, googleConnections([]string{gscScope}, []string{"search_console.read"}))
					return
				}
				writeAction(w, "gsc.sites.list", map[string]any{"siteEntry": []any{map[string]string{"siteUrl": "https://aquatiq.com/", "permissionLevel": permission}}})
			})
			got := broker.Discover(t.Context(), "https://aquatiq.com/")
			want := "connected"
			if permission == "siteUnverifiedUser" {
				want = "not_found"
			}
			if got.SearchConsole.Status != want || got.GA4.Status != "requires_consent" {
				t.Fatalf("evidence=%+v", got)
			}
			if want == "connected" && got.SearchConsole.AccessLevel != permission {
				t.Fatal("permission level rewritten")
			}
		})
	}
}

func TestDiscoverGA4AmbiguityDoesNotStoreArbitraryMapping(t *testing.T) {
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
		streams := []any{}
		for _, id := range []string{"1", "2"} {
			streams = append(streams, map[string]any{"name": "properties/123/dataStreams/" + id, "type": "WEB_DATA_STREAM", "webStreamData": map[string]string{"defaultUri": "https://aquatiq.com/", "measurementId": "G-ABC123"}})
		}
		writeAction(w, action.Operation, map[string]any{"dataStreams": streams})
	})
	got := broker.Discover(t.Context(), "https://aquatiq.com/")
	if got.GA4.Status != "needs_selection" || got.GA4.PropertyID != "" || got.GA4.ConnectionID != "" {
		t.Fatalf("GA4=%+v", got.GA4)
	}
}

func TestDiscoverRejectsInvalidSitesAndUnavailableSources(t *testing.T) {
	calls := 0
	broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) { calls++; w.WriteHeader(500) })
	if got := broker.Discover(t.Context(), "https://127.0.0.1/"); got.SearchConsole.Status != "source_unavailable" || calls != 0 {
		t.Fatalf("invalidsite=%+v calls=%d", got, calls)
	}
	if got := broker.Discover(t.Context(), "https://aquatiq.com/"); got.GA4.Status != "source_unavailable" {
		t.Fatalf("upstreamfailure=%+v", got)
	}
	got := New(Config{}).Discover(t.Context(), "https://aquatiq.com/")
	if got.SearchConsole.Status != "needs_connection" || got.GA4.Status != "requires_consent" {
		t.Fatalf("unconfigured=%+v", got)
	}
}

func validGoogleAuthorization() string {
	query := url.Values{"client_id": {"synthetic.apps.googleusercontent.com"}, "response_type": {"code"}, "redirect_uri": {"http://localhost:3026/oauth/callback/google"}, "scope": {"openid email profile " + gscScope + " " + gaScope}, "state": {strings.Repeat("s", 43)}, "code_challenge": {strings.Repeat("c", 43)}, "code_challenge_method": {"S256"}, "access_type": {"offline"}}
	return "https://accounts.google.com/o/oauth2/v2/auth?" + query.Encode()
}

func TestGoogleConsentAndStatusStayInsideApprovedBrokerFlow(t *testing.T) {
	broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/providers/google/connect-session" {
			var body struct {
				Bundles   []string `json:"bundles"`
				ReturnURL string   `json:"returnUrl"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			if strings.Join(body.Bundles, ",") != "search_console,analytics_readonly" || body.ReturnURL != "http://localhost:5173/studio/pulse" {
				t.Errorf("unsafeconsent=%+v", body)
			}
			writeFixture(w, map[string]any{"success": true, "data": map[string]any{"sessionToken": "session-123", "authorizationUrl": validGoogleAuthorization(), "expiresAt": time.Now().Add(10 * time.Minute).UTC().Format(time.RFC3339)}})
			return
		}
		if r.URL.Path != "/api/v1/connect-sessions/session-123/status" {
			t.Errorf("unexpected statuspath %s", r.URL.Path)
		}
		writeFixture(w, map[string]any{"success": true, "data": map[string]any{"id": "session-123", "providerKey": "google", "status": "completed"}})
	})
	session, err := broker.BeginGoogle(t.Context(), "http://localhost:5173/studio/pulse")
	if err != nil || session.SessionID != "session-123" {
		t.Fatalf("session=%+v err=%v", session, err)
	}
	if status, err := broker.GoogleStatus(t.Context(), session.SessionID); err != nil || status != "connected" {
		t.Fatalf("status=%s err=%v", status, err)
	}
	for _, bad := range []string{"https://evil.example/", "http://localhost:5174/studio/pulse?x=1", "http://127.0.0.1:5174/studio/pulse"} {
		if _, err := broker.BeginGoogle(t.Context(), bad); err == nil {
			t.Fatal("accepted arbitraryreturnURL")
		}
	}
	if _, err := broker.GoogleStatus(t.Context(), "../bad"); err == nil {
		t.Fatal("accepted unsafe sessionID")
	}
}

func TestGoogleConsentRejectsUnexpectedAuthorizationDestination(t *testing.T) {
	broker := fixtureBroker(t, func(w http.ResponseWriter, r *http.Request) {
		writeFixture(w, map[string]any{"success": true, "data": map[string]any{"sessionToken": "session-123", "authorizationUrl": "https://evil.example/?secret=hidden", "expiresAt": time.Now().Add(time.Minute).UTC().Format(time.RFC3339)}})
	})
	if session, err := broker.BeginGoogle(t.Context(), "http://localhost:5173/studio/pulse"); err == nil || session.AuthorizationURL != "" {
		t.Fatal(fmt.Sprintf("untrusted authorization accepted: %v", err))
	}
}
