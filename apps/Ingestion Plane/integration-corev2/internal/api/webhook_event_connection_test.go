package api

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/webhookorg"
)

// FU-18: a stored webhook event carries the provider connection ingest resolved it to, and the internal event read
// (the one capability-core's trigger dry run uses) returns it. An event ingest could not bind to a connection carries
// none, and a connection id the caller put in the payload or a header is never taken as the binding.

func postSignedMetaWebhook(t *testing.T, app *fiber.App, secret, body string, headers map[string]string) []string {
	t.Helper()
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = mac.Write([]byte(body))
	req := httptest.NewRequest("POST", "/api/v1/webhooks/meta", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Hub-Signature-256", "sha256="+hex.EncodeToString(mac.Sum(nil)))
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("post meta webhook: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("post meta webhook status = %d, want 200", resp.StatusCode)
	}
	var posted struct {
		Data struct {
			WebhookEventIDs []string `json:"webhookEventIds"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&posted); err != nil {
		t.Fatalf("decode post response: %v", err)
	}
	if len(posted.Data.WebhookEventIDs) == 0 {
		t.Fatal("post response carries no webhookEventIds")
	}
	return posted.Data.WebhookEventIDs
}

// readStoredWebhookEvent reads the event through the internal route and returns the raw JSON object, so an absent
// connectionId is distinguishable from an empty one.
func readStoredWebhookEvent(t *testing.T, app *fiber.App, id, organizationID string) map[string]any {
	t.Helper()
	req := httptest.NewRequest("GET", "/internal/webhooks/events/"+id+"?organizationId="+organizationID, nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("get webhook event: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("get webhook event %s status = %d, want 200", id, resp.StatusCode)
	}
	var fetched struct {
		Data struct {
			WebhookEvent map[string]any `json:"webhookEvent"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&fetched); err != nil {
		t.Fatalf("decode get response: %v", err)
	}
	return fetched.Data.WebhookEvent
}

func TestAResolvedWebhookEventCarriesItsConnectionOnTheEventRead(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookSecret = "meta-webhook-secret"
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-meta", ProviderKey: "meta", OrganizationID: "org-1", Status: "active",
		ProviderContext: map[string]string{"webhook_account_ids": "page-1"},
	}); err != nil {
		t.Fatalf("UpsertConnection: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, WebhookOrg: &webhookorg.Resolver{Store: repo}})

	// The payload and a header both claim another connection; neither may become the binding.
	ids := postSignedMetaWebhook(t, app, cfg.MetaWebhookSecret,
		`{"object":"page","connectionId":"conn-forged","entry":[{"id":"page-1","connectionId":"conn-forged"}]}`,
		map[string]string{"X-Connection-ID": "conn-forged"})
	if len(ids) != 1 {
		t.Fatalf("event ids = %v, want one", ids)
	}
	event := readStoredWebhookEvent(t, app, ids[0], "org-1")
	if event["connectionId"] != "conn-meta" {
		t.Fatalf("event connectionId = %#v, want the resolved connection conn-meta", event["connectionId"])
	}
	stored, err := repo.GetWebhookEvent(t.Context(), "org-1", ids[0])
	if err != nil || stored.ConnectionID != "conn-meta" {
		t.Fatalf("stored event connection = %q err=%v, want conn-meta", stored.ConnectionID, err)
	}
}

func TestEachPartitionOfAMultiTenantBatchIsBoundToItsOwnConnection(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.MetaWebhookSecret = "meta-webhook-secret"
	for _, connection := range []store.Connection{
		{ID: "conn-a", ProviderKey: "meta", OrganizationID: "org-a", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-a"}},
		{ID: "conn-b", ProviderKey: "meta", OrganizationID: "org-b", Status: "active", ProviderContext: map[string]string{"webhook_account_ids": "page-b"}},
	} {
		if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
			t.Fatalf("UpsertConnection: %v", err)
		}
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service, WebhookOrg: &webhookorg.Resolver{Store: repo}})
	ids := postSignedMetaWebhook(t, app, cfg.MetaWebhookSecret,
		`{"object":"page","entry":[{"id":"page-a","messaging":[]},{"id":"page-b","messaging":[]}]}`, nil)
	if len(ids) != 2 {
		t.Fatalf("event ids = %v, want one per tenant", ids)
	}
	want := map[string]string{"org-a": "conn-a", "org-b": "conn-b"}
	seen := map[string]bool{}
	for _, id := range ids {
		var event map[string]any
		var org string
		for candidate := range want {
			if _, err := repo.GetWebhookEvent(t.Context(), candidate, id); err == nil {
				org = candidate
				event = readStoredWebhookEvent(t, app, id, candidate)
				break
			}
		}
		if org == "" {
			t.Fatalf("event %s is stored for neither tenant", id)
		}
		if event["connectionId"] != want[org] {
			t.Errorf("event %s of %s carries connectionId %#v, want %s", id, org, event["connectionId"], want[org])
		}
		seen[org] = true
	}
	if len(seen) != 2 {
		t.Fatalf("tenants seen = %v, want both", seen)
	}
}

func TestAnEventIngestCannotBindCarriesNoConnection(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.AllowUnverifiedWebhooks = true
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("POST", "/api/v1/webhooks/github",
		strings.NewReader(`{"organizationId":"org-1","deliveryId":"delivery-fu18","connectionId":"conn-forged"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Connection-ID", "conn-forged")
	resp, err := app.Test(req)
	if err != nil || resp.StatusCode != http.StatusOK {
		t.Fatalf("post github webhook status=%v err=%v", resp.StatusCode, err)
	}
	var posted struct {
		Data struct {
			WebhookEventID string `json:"webhookEventId"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&posted); err != nil {
		t.Fatalf("decode post response: %v", err)
	}
	event := readStoredWebhookEvent(t, app, posted.Data.WebhookEventID, "org-1")
	if value, present := event["connectionId"]; present {
		t.Fatalf("unbound event carries connectionId %#v; it must carry none so the dry run refuses it", value)
	}
	stored, err := repo.GetWebhookEvent(t.Context(), "org-1", posted.Data.WebhookEventID)
	if err != nil || stored.ConnectionID != "" {
		t.Fatalf("stored connection = %q err=%v, want empty", stored.ConnectionID, err)
	}
}
