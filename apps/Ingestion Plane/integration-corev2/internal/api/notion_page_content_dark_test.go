package api

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

// The Notion page-content read (page.content) is dark while
// INTEGRATION_NOTION_PAGE_CONTENT_READ is off: the action route refuses it as an
// unsupported operation before the provider token and the Notion API are reached,
// and the served operation catalog leaves its row out. With the flag on, the read
// reaches Notion with the connection's own token.

const notionPageContentTestPageID = "0123456789abcdef0123456789abcdef"

// notionPageContentApp serves the action route and the catalog for one Notion
// connection that grants content.read. The returned func lists the requests the
// stub Notion API received, as method, request URI and Authorization header.
func notionPageContentApp(t *testing.T, notionPageContentRead bool) (*fiber.App, func() []string) {
	t.Helper()
	var mu sync.Mutex
	var requests []string
	notionServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		requests = append(requests, r.Method+" "+r.URL.RequestURI()+" "+r.Header.Get("Authorization"))
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"object":"list","results":[{"object":"block","id":"block-1"}],"next_cursor":null}`))
	}))
	t.Cleanup(notionServer.Close)

	cfg, repo, oauthService := testOAuthStack(t)
	cfg.NotionAPIBaseURL = notionServer.URL
	cfg.NotionPageContentReadEnabled = notionPageContentRead
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("notion-token", []byte("conn-notion-content"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-notion-content", ProviderKey: "notion", ConnectorType: "notion",
		OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: []string{"content.read"},
		EncryptedAccessToken: encryptedToken, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Actions: actions.NewService(cfg, notionServer.Client())})
	return app, func() []string {
		mu.Lock()
		defer mu.Unlock()
		return append([]string(nil), requests...)
	}
}

func postNotionPageContent(t *testing.T, app *fiber.App, payload string) (int, string) {
	t.Helper()
	req := httptest.NewRequest("POST", "/api/v1/connections/conn-notion-content/actions", strings.NewReader(payload))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	req.Header.Set("X-User-ID", "user-1")
	response, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	defer response.Body.Close()
	body, _ := io.ReadAll(response.Body)
	return response.StatusCode, string(body)
}

// notionCatalogOperations returns the operation names the served catalog lists for notion.
func notionCatalogOperations(t *testing.T, app *fiber.App) []string {
	t.Helper()
	req := httptest.NewRequest("GET", "/api/v1/actions/catalog", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	response, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	defer response.Body.Close()
	if response.StatusCode != fiber.StatusOK {
		t.Fatalf("catalog status = %d, want 200", response.StatusCode)
	}
	var doc struct {
		Providers []struct {
			Provider   string `json:"provider"`
			Operations []struct {
				Operation string `json:"operation"`
			} `json:"operations"`
		} `json:"providers"`
	}
	if err := json.NewDecoder(response.Body).Decode(&doc); err != nil {
		t.Fatalf("decode catalog: %v", err)
	}
	var operations []string
	for _, provider := range doc.Providers {
		if provider.Provider != "notion" {
			continue
		}
		for _, operation := range provider.Operations {
			operations = append(operations, operation.Operation)
		}
	}
	return operations
}

// While the flag is off, page.content is refused as an unsupported operation (400
// operation_not_supported) before the Notion API is reached, under either spelling.
func TestNotionPageContentIsRefusedBeforeTheProviderWhileDark(t *testing.T) {
	app, providerRequests := notionPageContentApp(t, false)
	for _, operation := range []string{"page.content", "notion.page.content"} {
		status, body := postNotionPageContent(t, app, `{"operation":"`+operation+`","params":{"pageId":"`+notionPageContentTestPageID+`"}}`)
		if status != fiber.StatusBadRequest || !strings.Contains(body, "operation_not_supported") {
			t.Fatalf("%s: status = %d (%s), want 400 operation_not_supported", operation, status, body)
		}
	}
	if requests := providerRequests(); len(requests) != 0 {
		t.Fatalf("a dark page.content reached the Notion API: %v", requests)
	}
}

// While the flag is off, the served catalog leaves the page.content row out and
// keeps the other notion rows.
func TestNotionPageContentRowIsNotServedWhileDark(t *testing.T) {
	app, _ := notionPageContentApp(t, false)
	operations := notionCatalogOperations(t, app)
	if slices.Contains(operations, "page.content") {
		t.Fatalf("the catalog serves page.content while its switch is off: %v", operations)
	}
	if !slices.Contains(operations, "databases") || !slices.Contains(operations, "pages") {
		t.Fatalf("the catalog lost the ungated notion rows: %v", operations)
	}
}

// With the flag on, page.content reads one page of the block children with the
// connection's own token, and the catalog serves the row.
func TestNotionPageContentReadsTheBlockChildrenWhileLive(t *testing.T) {
	app, providerRequests := notionPageContentApp(t, true)
	status, body := postNotionPageContent(t, app, `{"operation":"page.content","params":{"pageId":"`+notionPageContentTestPageID+`","pageSize":10}}`)
	if status != fiber.StatusOK {
		t.Fatalf("status = %d (%s), want 200", status, body)
	}
	want := "GET /v1/blocks/" + notionPageContentTestPageID + "/children?page_size=10 Bearer notion-token"
	if requests := providerRequests(); len(requests) != 1 || requests[0] != want {
		t.Fatalf("Notion API requests = %v, want [%q]", requests, want)
	}
	if !slices.Contains(notionCatalogOperations(t, app), "page.content") {
		t.Fatal("the catalog does not serve page.content while its switch is on")
	}
}
