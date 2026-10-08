package api

import (
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/store"
)

// F4: the files and SharePoint read operations are reachable through the
// action route, are gated by the capability the connection already grants for
// files (sharepoint.read for Microsoft, drive.read for Google), never ask for a
// write approval, and an operation the catalog does not know stays refused.

var driveReadOperations = []struct {
	providerKey string
	operation   string
	wantCap     string
}{
	{"microsoft", "drive.files", "sharepoint.read"},
	{"microsoft", "microsoft.drive.files", "sharepoint.read"},
	{"microsoft", "drive.file.content", "sharepoint.read"},
	{"microsoft", "microsoft.drive.file.content", "sharepoint.read"},
	{"microsoft", "sharepoint.sites", "sharepoint.read"},
	{"microsoft", "microsoft.sharepoint.sites", "sharepoint.read"},
	{"microsoft", "sharepoint.drives", "sharepoint.read"},
	{"microsoft", "microsoft.sharepoint.drives", "sharepoint.read"},
	{"google", "drive.files", "drive.read"},
	{"google", "google.drive.files", "drive.read"},
	{"google", "drive.file.content", "drive.read"},
	{"google", "google.drive.file.content", "drive.read"},
}

func TestFilesAndSharePointReadOperationsMapToACapabilityAndNeverNeedApproval(t *testing.T) {
	for _, tt := range driveReadOperations {
		t.Run(tt.providerKey+"/"+tt.operation, func(t *testing.T) {
			gotCap, sensitive := requiredCapabilityForOperation(tt.providerKey, tt.operation)
			if gotCap != tt.wantCap || sensitive {
				t.Fatalf("requiredCapabilityForOperation = %q/%v, want %q/false (the same, non-sensitive capability the file listing already needs)", gotCap, sensitive, tt.wantCap)
			}
			if actionRequiresApproval(tt.providerKey, tt.operation) {
				t.Fatalf("actionRequiresApproval(%q, %q) = true, want false (a read must never demand a write attestation)", tt.providerKey, tt.operation)
			}
		})
	}
}

func TestSharePointOperationsStayUnsupportedWhereTheyAreNotDefined(t *testing.T) {
	for _, tt := range []struct{ providerKey, operation string }{
		{"microsoft", "sharepoint.delete"},
		{"microsoft", "sharepoint.sites.write"},
		{"microsoft", "sharepoint.upload"},
		{"microsoft", "sharepoint"},
		{"microsoft", "sharepoint.lists"},
		{"microsoft", "microsoft.sharepoint.lists"},
		{"microsoft", "drive.file.delete"},
		{"microsoft", "drive.file.upload"},
		{"google", "sharepoint.sites"},
		{"google", "sharepoint.drives"},
		{"google", "google.sharepoint.sites"},
		{"google", "drive.file.upload"},
		{"slack", "sharepoint.sites"},
		{"github", "drive.files"},
		{"", "sharepoint.sites"},
	} {
		t.Run(tt.providerKey+"/"+tt.operation, func(t *testing.T) {
			if gotCap, _ := requiredCapabilityForOperation(tt.providerKey, tt.operation); gotCap != "" {
				t.Fatalf("requiredCapabilityForOperation = %q, want none for an operation that is not defined", gotCap)
			}
			err := requireActionCapability(nil, store.Connection{ProviderKey: tt.providerKey, Capabilities: []string{"sharepoint.read", "drive.read"}}, actionBody{Operation: tt.operation})
			if !errors.Is(err, errActionOperationUnsupported) {
				t.Fatalf("requireActionCapability error = %v, want errActionOperationUnsupported", err)
			}
		})
	}
}

func TestRequireActionCapabilityForTheNewSharePointOperations(t *testing.T) {
	operations := []string{"sharepoint.sites", "microsoft.sharepoint.sites", "sharepoint.drives", "microsoft.sharepoint.drives"}
	for _, operation := range operations {
		t.Run(operation, func(t *testing.T) {
			body := actionBody{Operation: operation}
			if err := requireActionCapability(nil, store.Connection{ProviderKey: "microsoft", Capabilities: []string{"sharepoint.read"}}, body); err != nil {
				t.Fatalf("a connection with sharepoint.read was refused: %v", err)
			}
			for _, capabilities := range [][]string{{"profile.read"}, {"mail.read"}, {"drive.read"}} {
				err := requireActionCapability(nil, store.Connection{ProviderKey: "microsoft", Capabilities: capabilities}, body)
				var authErr auth.Error
				if !errors.As(err, &authErr) || authErr.Status != fiber.StatusForbidden || authErr.Code != "capability_required" {
					t.Fatalf("capabilities %v: error = %v, want 403 capability_required", capabilities, err)
				}
			}
			// Same rule as the file listing itself: a connection from before
			// capabilities were recorded keeps working for a non-sensitive read.
			if err := requireActionCapability(nil, store.Connection{ProviderKey: "microsoft"}, body); err != nil {
				t.Fatalf("a legacy connection without recorded capabilities was refused: %v", err)
			}
		})
	}
}

type graphRequestLog struct {
	mu       sync.Mutex
	requests []string
	bearers  []string
}

func (l *graphRequestLog) add(r *http.Request) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.requests = append(l.requests, r.Method+" "+r.URL.Path+"?"+r.URL.RawQuery)
	l.bearers = append(l.bearers, r.Header.Get("Authorization"))
}

func (l *graphRequestLog) snapshot() ([]string, []string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	return append([]string(nil), l.requests...), append([]string(nil), l.bearers...)
}

func sharePointActionApp(t *testing.T, capabilities []string, log *graphRequestLog) *fiber.App {
	t.Helper()
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		log.add(r)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"value":[{"id":"marker-row"}]}`))
	}))
	t.Cleanup(providerServer.Close)
	cfg, repo, oauthService := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	encryptedToken, err := vault.Encrypt("access-token", []byte("conn-sharepoint"))
	if err != nil {
		t.Fatalf("Encrypt error: %v", err)
	}
	if _, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-sharepoint", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", UserID: "user-1", Status: "active", Capabilities: capabilities,
		EncryptedAccessToken: encryptedToken, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	return NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Actions: actions.NewService(cfg, providerServer.Client())})
}

func postSharePointAction(t *testing.T, app *fiber.App, payload string) (int, string) {
	t.Helper()
	req := httptest.NewRequest("POST", "/api/v1/connections/conn-sharepoint/actions", strings.NewReader(payload))
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

func TestSharePointReadOperationsReachGraphThroughTheActionRoute(t *testing.T) {
	var log graphRequestLog
	app := sharePointActionApp(t, []string{"sharepoint.read"}, &log)
	tests := []struct {
		payload     string
		wantRequest string
	}{
		{`{"operation":"sharepoint.sites","params":{"query":"contoso"}}`, "GET /v1.0/sites?%24select=id%2CdisplayName%2CwebUrl&search=contoso"},
		{`{"operation":"microsoft.sharepoint.drives","params":{"siteId":"site-1"}}`, "GET /v1.0/sites/site-1/drives?%24select=id%2Cname%2CdriveType%2CwebUrl"},
		{`{"operation":"drive.files","params":{"driveId":"lib-1","folderId":"folder-1","maxResults":5}}`, "GET /v1.0/drives/lib-1/items/folder-1/children?%24select="},
		{`{"operation":"drive.file.content","params":{"driveId":"lib-1","itemId":"item-1"}}`, "GET /v1.0/drives/lib-1/items/item-1/content?"},
	}
	for _, tt := range tests {
		status, body := postSharePointAction(t, app, tt.payload)
		if status != fiber.StatusOK {
			t.Fatalf("%s: status = %d (%s), want 200", tt.payload, status, body)
		}
	}
	requests, bearers := log.snapshot()
	if len(requests) != len(tests) {
		t.Fatalf("provider requests = %v, want %d", requests, len(tests))
	}
	for index, tt := range tests {
		if !strings.HasPrefix(requests[index], tt.wantRequest) {
			t.Fatalf("request %d = %q, want it to start with %q", index, requests[index], tt.wantRequest)
		}
		if bearers[index] != "Bearer access-token" {
			t.Fatalf("request %d Authorization = %q, want the connection's own token", index, bearers[index])
		}
	}
}

func TestSharePointReadOperationsAnswerWithTheGraphRows(t *testing.T) {
	var log graphRequestLog
	app := sharePointActionApp(t, []string{"sharepoint.read"}, &log)
	status, body := postSharePointAction(t, app, `{"operation":"sharepoint.sites"}`)
	if status != fiber.StatusOK || !strings.Contains(body, "marker-row") {
		t.Fatalf("status = %d, body = %s, want the provider's rows in the answer", status, body)
	}
}

func TestSharePointReadOperationsAreRefusedWithoutTheCapabilityBeforeAnyProviderCall(t *testing.T) {
	var log graphRequestLog
	app := sharePointActionApp(t, []string{"profile.read", "mail.read"}, &log)
	for _, operation := range []string{"sharepoint.sites", "sharepoint.drives", "drive.files"} {
		status, body := postSharePointAction(t, app, `{"operation":"`+operation+`","params":{"siteId":"s"}}`)
		if status != fiber.StatusForbidden || !strings.Contains(body, "capability_required") {
			t.Fatalf("%s: status = %d, body = %s, want 403 capability_required", operation, status, body)
		}
	}
	if requests, _ := log.snapshot(); len(requests) != 0 {
		t.Fatalf("a refused request still reached the provider: %v", requests)
	}
}

func TestAnUnknownSharePointOperationFailsClosedBeforeTheProvider(t *testing.T) {
	var log graphRequestLog
	app := sharePointActionApp(t, []string{"sharepoint.read", "sharepoint.write"}, &log)
	for _, operation := range []string{"sharepoint.delete", "sharepoint.upload", "sharepoint.lists", "microsoft.sharepoint.write"} {
		status, body := postSharePointAction(t, app, `{"operation":"`+operation+`"}`)
		if status != fiber.StatusBadRequest || !strings.Contains(body, "operation_not_supported") {
			t.Fatalf("%s: status = %d, body = %s, want 400 operation_not_supported", operation, status, body)
		}
	}
	if requests, _ := log.snapshot(); len(requests) != 0 {
		t.Fatalf("an unknown operation reached the provider: %v", requests)
	}
}
