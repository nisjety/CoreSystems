package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/triodelab/finspo/internal/sharepoint"
)

type stubBrowser struct {
	sites  []sharepoint.Site
	drives []sharepoint.Drive
	items  []sharepoint.Item
	err    error

	receivedOrgID  string
	receivedSiteID string
	receivedPath   string
}

func (s *stubBrowser) ListSites(_ context.Context, orgID string) ([]sharepoint.Site, error) {
	s.receivedOrgID = orgID
	if s.err != nil {
		return nil, s.err
	}
	return s.sites, nil
}

func (s *stubBrowser) ListDrives(_ context.Context, orgID, siteID string) ([]sharepoint.Drive, error) {
	s.receivedOrgID = orgID
	s.receivedSiteID = siteID
	if s.err != nil {
		return nil, s.err
	}
	return s.drives, nil
}

func (s *stubBrowser) ListItems(_ context.Context, orgID, siteID, path string) ([]sharepoint.Item, error) {
	s.receivedOrgID = orgID
	s.receivedSiteID = siteID
	s.receivedPath = path
	if s.err != nil {
		return nil, s.err
	}
	return s.items, nil
}

func TestHealthRoute(t *testing.T) {
	t.Parallel()

	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: &stubBrowser{},
	})

	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload struct {
		Status  string `json:"status"`
		Service string `json:"service"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode body: %v", err)
	}

	if payload.Status != "ok" {
		t.Fatalf("status payload = %q, want ok", payload.Status)
	}
	if payload.Service != "finspo-core" {
		t.Fatalf("service payload = %q, want finspo-core", payload.Service)
	}
}

func TestReadyRouteSkipsMissingDependencies(t *testing.T) {
	t.Parallel()

	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: &stubBrowser{},
	})

	req := httptest.NewRequest(http.MethodGet, "/ready", nil)
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload struct {
		Status string            `json:"status"`
		Checks map[string]string `json:"checks"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode body: %v", err)
	}

	if payload.Status != "ok" {
		t.Fatalf("status = %q, want ok", payload.Status)
	}
	if payload.Checks["db"] != "skipped" {
		t.Fatalf("checks.db = %q, want skipped", payload.Checks["db"])
	}
	if payload.Checks["nats"] != "skipped" {
		t.Fatalf("checks.nats = %q, want skipped", payload.Checks["nats"])
	}
}

func TestListSitesRequiresAPIKey(t *testing.T) {
	t.Parallel()

	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: &stubBrowser{},
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/sites", nil)
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusUnauthorized)
	}
}

func TestListSitesReturnsNormalizedPayload(t *testing.T) {
	t.Parallel()

	browser := &stubBrowser{sites: []sharepoint.Site{{
		ID:          "site-1",
		Name:        "finance",
		DisplayName: "Finance",
		WebURL:      "https://example.sharepoint.com/sites/finance",
	}}}
	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: browser,
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/sites", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}

	var payload struct {
		Success bool `json:"success"`
		Data    struct {
			Count int               `json:"count"`
			Sites []sharepoint.Site `json:"sites"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode body: %v", err)
	}

	if !payload.Success {
		t.Fatalf("success = false, want true")
	}
	if browser.receivedOrgID != "org-123" {
		t.Fatalf("orgID = %q, want org-123", browser.receivedOrgID)
	}
	if payload.Data.Count != 1 {
		t.Fatalf("count = %d, want 1", payload.Data.Count)
	}
	if len(payload.Data.Sites) != 1 || payload.Data.Sites[0].ID != "site-1" {
		t.Fatalf("sites payload = %#v, want one site with id site-1", payload.Data.Sites)
	}
}

func TestListItemsDefaultsPathToRoot(t *testing.T) {
	t.Parallel()

	modifiedAt := time.Date(2026, time.April, 9, 10, 0, 0, 0, time.UTC)
	browser := &stubBrowser{items: []sharepoint.Item{{
		Name:         "Budget.xlsx",
		Path:         "/Budget.xlsx",
		Size:         1024,
		IsFolder:     false,
		ModifiedTime: modifiedAt,
		WebURL:       "https://example.sharepoint.com/sites/finance/Budget.xlsx",
	}}}
	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: browser,
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/sites/site-1/items", nil)
	req.Header.Set("X-API-Key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusOK)
	}
	if browser.receivedOrgID != "org-123" {
		t.Fatalf("orgID = %q, want org-123", browser.receivedOrgID)
	}
	if browser.receivedSiteID != "site-1" {
		t.Fatalf("siteID = %q, want site-1", browser.receivedSiteID)
	}
	if browser.receivedPath != "/" {
		t.Fatalf("path = %q, want /", browser.receivedPath)
	}
}

func TestListSitesReturnsServiceUnavailableWhenNotConfigured(t *testing.T) {
	t.Parallel()

	app := NewServer(ServerConfig{
		APIKey: "test-key",
		Browser: &stubBrowser{
			err: sharepoint.ErrNotConfigured,
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/sites", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusServiceUnavailable)
	}
}

func TestListItemsPropagatesUnexpectedErrors(t *testing.T) {
	t.Parallel()

	app := NewServer(ServerConfig{
		APIKey: "test-key",
		Browser: &stubBrowser{
			err: errors.New("boom"),
		},
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/sites/site-1/items?path=/Shared%20Documents", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusBadGateway {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadGateway)
	}
}

func TestListSitesRejectsMissingOrgHeader(t *testing.T) {
	t.Parallel()

	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: &stubBrowser{},
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/sites", nil)
	req.Header.Set("x-internal-api-key", "test-key")
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}
