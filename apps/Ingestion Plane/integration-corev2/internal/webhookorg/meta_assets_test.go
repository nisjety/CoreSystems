package webhookorg

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"slices"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

type staticTokenSource struct{}

func (staticTokenSource) AccessTokenForConnection(context.Context, string) (oauth.AccessTokenResult, error) {
	return oauth.AccessTokenResult{AccessToken: "user-token", ExpiresAt: time.Now().Add(time.Hour)}, nil
}

func TestGraphAssetListerDiscoversAndSubscribesMetaInboxAssets(t *testing.T) {
	var subscribed []string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/me/accounts":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{
				"id": "page-1", "access_token": "page-token",
				"instagram_business_account": map[string]string{"id": "ig-1"},
			}}})
		case r.Method == http.MethodPost && r.URL.Path == "/page-1/subscribed_apps":
			if r.Header.Get("Authorization") != "Bearer page-token" {
				t.Fatalf("Page subscription Authorization = %q", r.Header.Get("Authorization"))
			}
			_ = r.ParseForm()
			if r.Form.Get("subscribed_fields") != "feed,messages" {
				t.Fatalf("Page subscribed_fields = %q, want exact inbox fields", r.Form.Get("subscribed_fields"))
			}
			subscribed = append(subscribed, "page-1")
			_ = json.NewEncoder(w).Encode(map[string]bool{"success": true})
		case r.Method == http.MethodGet && r.URL.Path == "/me/businesses":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{
				"owned_whatsapp_business_accounts": map[string]any{"data": []map[string]string{{"id": "waba-1"}}},
			}}})
		case r.Method == http.MethodPost && r.URL.Path == "/waba-1/subscribed_apps":
			if r.Header.Get("Authorization") != "Bearer user-token" {
				t.Fatalf("WABA subscription Authorization = %q", r.Header.Get("Authorization"))
			}
			subscribed = append(subscribed, "waba-1")
			_ = json.NewEncoder(w).Encode(map[string]bool{"success": true})
		case r.Method == http.MethodGet && r.URL.Path == "/waba-1/phone_numbers":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{{"id": "phone-1"}}})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	ids, err := lister.ListWebhookAccountIDs(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta",
		Capabilities: []string{"social.inbox.read", "social.whatsapp.manage"},
	})
	if err != nil {
		t.Fatalf("ListWebhookAccountIDs error: %v", err)
	}
	for _, want := range []string{"page-1", "ig-1", "waba-1", "phone-1"} {
		if !slices.Contains(ids, want) {
			t.Errorf("ids = %v, missing %s", ids, want)
		}
	}
	if len(subscribed) != 0 {
		t.Fatalf("discovery mutated provider subscriptions: %v", subscribed)
	}
	if err := lister.SubscribeWebhookAccounts(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta",
		Capabilities: []string{"social.inbox.read", "social.whatsapp.manage"},
	}, ids); err != nil {
		t.Fatalf("SubscribeWebhookAccounts error: %v", err)
	}
	if !slices.Equal(subscribed, []string{"page-1", "waba-1"}) {
		t.Fatalf("subscribed assets = %v, want Page and WABA", subscribed)
	}
}

func TestGraphAssetListerLegacyWhatsAppUsesWABAWithoutPageDiscovery(t *testing.T) {
	var paths []string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		paths = append(paths, r.URL.Path)
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/me/businesses":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{
				"owned_whatsapp_business_accounts": map[string]any{"data": []map[string]string{{"id": "waba-1"}}},
			}}})
		case r.Method == http.MethodGet && r.URL.Path == "/waba-1/phone_numbers":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{{"id": "phone-1"}}})
		case r.Method == http.MethodPost && r.URL.Path == "/waba-1/subscribed_apps":
			_ = json.NewEncoder(w).Encode(map[string]bool{"success": true})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	conn := store.Connection{ID: "conn-wa", ProviderKey: "whatsapp", Capabilities: []string{"social.inbox.read"}}
	ids, err := lister.ListWebhookAccountIDs(t.Context(), conn)
	if err != nil || !slices.Equal(ids, []string{"waba-1", "phone-1"}) {
		t.Fatalf("legacy WhatsApp ids=%v err=%v", ids, err)
	}
	if slices.Contains(paths, "/me/accounts") {
		t.Fatalf("legacy WhatsApp unexpectedly queried Pages: %v", paths)
	}
	if err := lister.SubscribeWebhookAccounts(t.Context(), conn, ids); err != nil {
		t.Fatalf("SubscribeWebhookAccounts: %v", err)
	}
}

func TestGraphAssetListerRejectsGraphSuccessFalse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/me/accounts":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{"id": "page-1", "access_token": "page-token"}}})
		case r.Method == http.MethodPost && r.URL.Path == "/page-1/subscribed_apps":
			_ = json.NewEncoder(w).Encode(map[string]bool{"success": false})
		default:
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []any{}})
		}
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	err := lister.SubscribeWebhookAccounts(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta", Capabilities: []string{"social.inbox.read"},
	}, []string{"page-1"})
	if err == nil {
		t.Fatal("SubscribeWebhookAccounts error = nil, want success:false rejection")
	}
}

func TestGraphAssetListerUnifiedMetaRejectsWABASuccessFalse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.Method == http.MethodGet && r.URL.Path == "/me/businesses":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{
				"owned_whatsapp_business_accounts": map[string]any{"data": []map[string]string{{"id": "waba-1"}}},
			}}})
		case r.Method == http.MethodGet && r.URL.Path == "/waba-1/phone_numbers":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]string{{"id": "phone-1"}}})
		case r.Method == http.MethodPost && r.URL.Path == "/waba-1/subscribed_apps":
			_ = json.NewEncoder(w).Encode(map[string]bool{"success": false})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	conn := store.Connection{ID: "conn-meta", ProviderKey: "meta", Capabilities: []string{"social.whatsapp.manage"}}
	ids, err := lister.ListWebhookAccountIDs(t.Context(), conn)
	if err != nil {
		t.Fatalf("ListWebhookAccountIDs: %v", err)
	}
	if err := lister.SubscribeWebhookAccounts(t.Context(), conn, ids); err == nil {
		t.Fatal("SubscribeWebhookAccounts error = nil, want unified WABA success:false rejection")
	}
}

func TestGraphAssetListerUnifiedMetaWithoutWABAKeepsPageInboxAvailable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch r.URL.Path {
		case "/me/accounts":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []map[string]any{{"id": "page-1", "access_token": "page-token"}}})
		case "/me/businesses":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []any{}})
		case "/page-1/subscribed_apps":
			_ = json.NewEncoder(w).Encode(map[string]bool{"success": true})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	conn := store.Connection{ID: "conn-meta", ProviderKey: "meta", Capabilities: []string{"social.inbox.read", "social.whatsapp.manage"}}
	ids, err := lister.ListWebhookAccountIDs(t.Context(), conn)
	if err != nil || !slices.Equal(ids, []string{"page-1"}) {
		t.Fatalf("unified Meta page-only ids=%v err=%v", ids, err)
	}
	if err := lister.SubscribeWebhookAccounts(t.Context(), conn, ids); err != nil {
		t.Fatalf("page provisioning should not require a WABA: %v", err)
	}
}

func TestGraphAssetListerDoesNotSubscribePublishingOnlyConnection(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests++
		http.Error(w, "unexpected Graph call", http.StatusInternalServerError)
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	ids, err := lister.ListWebhookAccountIDs(t.Context(), store.Connection{
		ID: "conn-publish", ProviderKey: "meta", Capabilities: []string{"social.post.write"},
	})
	if err != nil || len(ids) != 0 || requests != 0 {
		t.Fatalf("publishing-only result ids=%v err=%v requests=%d", ids, err, requests)
	}
}

func TestGraphAssetListerRequiresWhatsAppDiscoveryWhenCapabilityGranted(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch r.URL.Path {
		case "/me/accounts":
			_ = json.NewEncoder(w).Encode(map[string]any{"data": []any{}})
		case "/me/businesses":
			http.Error(w, `{"error":{"message":"permission missing"}}`, http.StatusForbidden)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	_, err := lister.ListWebhookAccountIDs(t.Context(), store.Connection{
		ID: "conn-wa", ProviderKey: "whatsapp", Capabilities: []string{"social.inbox.read"},
	})
	if err == nil {
		t.Fatal("ListWebhookAccountIDs error = nil, want required WhatsApp discovery failure")
	}
}

func TestGraphAssetListerRejectsOffOriginPagination(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"data": []any{}, "paging": map[string]string{"next": "https://attacker.invalid/steal"},
		})
	}))
	defer server.Close()
	lister := &GraphAssetLister{BaseURL: server.URL, Tokens: staticTokenSource{}, HTTP: server.Client()}
	_, err := lister.ListWebhookAccountIDs(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta", Capabilities: []string{"social.inbox.read"},
	})
	if err == nil {
		t.Fatal("ListWebhookAccountIDs error = nil, want off-origin pagination rejection")
	}
}

func TestGraphAssetListerRejectsOffOriginRedirect(t *testing.T) {
	targetCalls := 0
	target := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { targetCalls++ }))
	defer target.Close()
	source := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusFound)
	}))
	defer source.Close()
	lister := &GraphAssetLister{BaseURL: source.URL, Tokens: staticTokenSource{}, HTTP: source.Client()}
	_, err := lister.ListWebhookAccountIDs(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta", Capabilities: []string{"social.inbox.read"},
	})
	if err == nil {
		t.Fatal("ListWebhookAccountIDs error = nil, want off-origin redirect rejection")
	}
	if targetCalls != 0 {
		t.Fatalf("off-origin redirect target calls = %d, want 0", targetCalls)
	}
}
