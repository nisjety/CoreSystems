package http

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/insight-core/internal/insights"
)

func TestRouterRequiresInternalAPIKey(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview", nil)
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", resp.Code, http.StatusUnauthorized)
	}
}

func TestHealthDoesNotRequireInternalAPIKey(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.Code, http.StatusOK)
	}
}

func TestRouterRejectsWrongInternalAPIKey(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview", nil)
	req.Header.Set("x-internal-api-key", "wrong")
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", resp.Code, http.StatusUnauthorized)
	}
}

func TestOverviewRequiresOrganizationScope(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.Code, http.StatusBadRequest)
	}
}

func TestConnectorsReturnMarketingRegistryWithoutCredentialMaterial(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/connectors", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusOK {
		t.Fatalf("status = %d, want %d, body=%s", resp.Code, http.StatusOK, resp.Body.String())
	}
	var decoded struct {
		Data []insights.ConnectorSlot `json:"data"`
	}
	if err := json.Unmarshal(resp.Body.Bytes(), &decoded); err != nil {
		t.Fatalf("Unmarshal connectors: %v", err)
	}
	want := map[string]struct{}{
		"google_search_console": {},
		"google_analytics_4":    {},
		"google_ads":            {},
		"meta_ads":              {},
		"semrush":               {},
		"first_party_seo":       {},
	}
	for _, connector := range decoded.Data {
		if _, required := want[connector.Type]; required {
			delete(want, connector.Type)
		}
	}
	if len(want) != 0 {
		t.Fatalf("marketing registry missing connectors: %#v", want)
	}
	body := strings.ToLower(resp.Body.String())
	for _, forbidden := range []string{"\"access_token\"", "\"refresh_token\"", "\"client_secret\"", "\"api_key\"", "\"password\""} {
		if strings.Contains(body, forbidden) {
			t.Fatalf("connector response exposed credential field %q: %s", forbidden, resp.Body.String())
		}
	}
}

func TestIngestAndOverviewAreOrgScoped(t *testing.T) {
	router := testRouter()
	for _, orgID := range []string{"org-1", "org-2"} {
		body := map[string]any{
			"org_id":  orgID,
			"surface": "campaigns",
			"metric":  "sent",
			"value":   7,
		}
		payload, err := json.Marshal(body)
		if err != nil {
			t.Fatalf("Marshal error: %v", err)
		}
		req := httptest.NewRequest(http.MethodPost, "/internal/insight-events", bytes.NewReader(payload))
		req.Header.Set("x-internal-api-key", "test-key")
		req.Header.Set("x-org-id", orgID)
		resp := httptest.NewRecorder()
		router.ServeHTTP(resp, req)
		if resp.Code != http.StatusAccepted {
			t.Fatalf("ingest status = %d, want %d, body=%s", resp.Code, http.StatusAccepted, resp.Body.String())
		}
	}

	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview?surface=campaigns", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()
	router.ServeHTTP(resp, req)
	if resp.Code != http.StatusOK {
		t.Fatalf("overview status = %d, want %d, body=%s", resp.Code, http.StatusOK, resp.Body.String())
	}

	var decoded struct {
		Data insights.Overview `json:"data"`
	}
	if err := json.Unmarshal(resp.Body.Bytes(), &decoded); err != nil {
		t.Fatalf("Unmarshal overview: %v", err)
	}
	if got := decoded.Data.Surfaces[0].Metrics[0].Value; got != 7 {
		t.Fatalf("overview value = %v, want 7", got)
	}
}

func TestOverviewRejectsInvalidDate(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview?from=yesterday", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.Code, http.StatusBadRequest)
	}
}

func TestOverviewRejectsUnsupportedSurface(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview?surface=billing", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, want %d, body=%s", resp.Code, http.StatusUnprocessableEntity, resp.Body.String())
	}
}

func TestIngestRejectsUnknownJSONFields(t *testing.T) {
	router := testRouter()
	req := httptest.NewRequest(http.MethodPost, "/internal/insight-events", bytes.NewReader([]byte(`{"org_id":"org-1","surface":"social","metric":"posts","value":1,"raw_secret":"nope"}`)))
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("x-org-id", "org-1")
	resp := httptest.NewRecorder()

	router.ServeHTTP(resp, req)

	if resp.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.Code, http.StatusBadRequest)
	}
}

func TestOverviewMeScopeUsesOnlyTheTrustedActorHeader(t *testing.T) {
	router := testRouter()
	for _, actorUserID := range []string{"user-a", "user-b"} {
		payload := []byte(`{"surface":"inbox","metric":"tickets_resolved","value":1}`)
		req := httptest.NewRequest(http.MethodPost, "/internal/insight-events", bytes.NewReader(payload))
		req.Header.Set("x-internal-api-key", "test-key")
		req.Header.Set("x-org-id", "org-1")
		req.Header.Set("x-user-id", actorUserID)
		resp := httptest.NewRecorder()
		router.ServeHTTP(resp, req)
		if resp.Code != http.StatusAccepted {
			t.Fatalf("ingest status = %d, want %d, body=%s", resp.Code, http.StatusAccepted, resp.Body.String())
		}
	}

	req := httptest.NewRequest(http.MethodGet, "/api/v1/insights/overview?surface=inbox&scope=me&user_id=user-b", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("x-org-id", "org-1")
	req.Header.Set("x-user-id", "user-a")
	resp := httptest.NewRecorder()
	router.ServeHTTP(resp, req)
	if resp.Code != http.StatusOK {
		t.Fatalf("overview status = %d, want %d, body=%s", resp.Code, http.StatusOK, resp.Body.String())
	}

	var decoded struct {
		Data insights.Overview `json:"data"`
	}
	if err := json.Unmarshal(resp.Body.Bytes(), &decoded); err != nil {
		t.Fatalf("Unmarshal overview: %v", err)
	}
	if got := decoded.Data.Surfaces[0].Metrics[0].Value; got != 1 {
		t.Fatalf("scoped overview value = %v, want 1", got)
	}
}

func TestNewServerBuildsRouter(t *testing.T) {
	cfg := &config.Config{ServiceName: "insight-core-test"}
	repository := insights.NewMemoryRepository(insights.DefaultConnectorSlots(insights.ConnectorSlotOptions{}))
	service := insights.NewService(repository)
	handler := NewHandler(cfg, service)

	server := NewServer(3163, handler, "test-key")
	if server == nil {
		t.Fatal("NewServer returned nil")
	}
}

func testRouter() http.Handler {
	cfg := &config.Config{ServiceName: "insight-core-test"}
	repository := insights.NewMemoryRepository(insights.DefaultConnectorSlots(insights.ConnectorSlotOptions{}))
	service := insights.NewService(repository)
	handler := NewHandler(cfg, service)
	return newRouter(handler, "test-key")
}
