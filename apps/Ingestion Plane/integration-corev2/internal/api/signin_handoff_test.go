package api

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"

	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// A Graph stand-in so the hand-off can resolve the account identity the way
// production does, without leaving the test process.
func graphProfileServer(t *testing.T) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasPrefix(r.URL.Path, "/v1.0/me") {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"id": "graph-oid-1", "displayName": "Ima Fernandes Da Costa", "mail": "ima.dacosta@aquatiq.com",
		})
	}))
}

func signInHandoffApp(t *testing.T, graphURL string) (*fiber.App, *store.MemoryRepository) {
	t.Helper()
	cfg := testConfig()
	cfg.MicrosoftGraphBaseURL = graphURL
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatalf("NewVault error: %v", err)
	}
	repo := store.NewMemoryRepository()
	service := oauth.NewService(cfg, repo, vault, oauth.NewMicrosoftClient(oauth.MicrosoftClientConfig{GraphBaseURL: graphURL}))
	return NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service}), repo
}

func postHandoff(t *testing.T, app *fiber.App, body string, internalKey bool) (*http.Response, map[string]any) {
	t.Helper()
	req := httptest.NewRequest("POST", "/internal/providers/microsoft/sign-in-handoff", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	if internalKey {
		req.Header.Set("X-Internal-API-Key", "dev-key")
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	var decoded map[string]any
	_ = json.NewDecoder(resp.Body).Decode(&decoded)
	return resp, decoded
}

// auth-core pushes the login token after every Microsoft sign-in; the first
// one creates the org's connection with exactly the capabilities the granted
// scopes support, and the response carries the same connection view (with
// sync lanes) every other surface reads.
func TestSignInHandoffCreatesConnectionAndReturnsView(t *testing.T) {
	graph := graphProfileServer(t)
	defer graph.Close()
	app, repo := signInHandoffApp(t, graph.URL)

	resp, body := postHandoff(t, app, `{
		"organizationId":"org-1","userId":"user-1","userEmail":"ima.dacosta@aquatiq.com",
		"providerAccountId":"oidc-sub-1","accessToken":"login-graph-token","expiresAt":"2030-01-01T00:00:00Z",
		"scopes":["email,openid,profile,User.Read,Files.Read.All,Sites.Read.All,Mail.Read"],"tokenRef":"account-row-1"
	}`, true)
	if resp.StatusCode != fiber.StatusCreated {
		t.Fatalf("status = %d, want 201; body %v", resp.StatusCode, body)
	}
	data, _ := body["data"].(map[string]any)
	if data["created"] != true {
		t.Fatalf("created = %v, want true", data["created"])
	}
	connection, _ := data["connection"].(map[string]any)
	if connection["providerKey"] != "microsoft" || connection["status"] != "active" || connection["providerAccountId"] != "graph-oid-1" {
		t.Fatalf("connection = %v, want an active microsoft connection keyed by the Graph id", connection)
	}
	if _, leaked := connection["encryptedAccessToken"]; leaked {
		t.Fatal("token material leaked into the response")
	}
	lanes, _ := connection["syncLanes"].(map[string]any)
	if _, ok := lanes["mail"]; !ok {
		t.Fatalf("syncLanes = %v, want a mail lane for the mail.read grant", lanes)
	}
	capabilities, _ := connection["capabilities"].([]any)
	if len(capabilities) != 3 { // mail.read, profile.read, sharepoint.read
		t.Fatalf("capabilities = %v, want the three the scopes support", capabilities)
	}

	stored, err := repo.GetConnection(context.Background(), connection["id"].(string))
	if err != nil {
		t.Fatalf("GetConnection error: %v", err)
	}
	if stored.ProviderContext["control_plane_token_ref"] != "account-row-1" {
		t.Fatalf("token ref not recorded: %v", stored.ProviderContext)
	}
}

func TestSignInHandoffWidensButNeverNarrowsOnRepeat(t *testing.T) {
	graph := graphProfileServer(t)
	defer graph.Close()
	app, repo := signInHandoffApp(t, graph.URL)

	first, _ := postHandoff(t, app, `{"organizationId":"org-1","userId":"user-1","accessToken":"t1",
		"scopes":["email,openid,profile,User.Read,Mail.Read,Files.Read.All,Sites.Read.All"],"tokenRef":"row"}`, true)
	if first.StatusCode != fiber.StatusCreated {
		t.Fatalf("first status = %d", first.StatusCode)
	}
	// A later login carrying a narrower grant must not remove anything.
	second, body := postHandoff(t, app, `{"organizationId":"org-1","userId":"user-1","accessToken":"t2",
		"scopes":["email,openid,profile,User.Read"],"tokenRef":"row"}`, true)
	if second.StatusCode != fiber.StatusOK {
		t.Fatalf("second status = %d, want 200 (updated, not created); body %v", second.StatusCode, body)
	}
	connections, _ := repo.ListConnections(context.Background(), store.ConnectionFilter{OrganizationID: "org-1"})
	if len(connections) != 1 {
		t.Fatalf("connections = %d, want one", len(connections))
	}
	for _, want := range []string{"mail.read", "sharepoint.read", "profile.read"} {
		found := false
		for _, capability := range connections[0].Capabilities {
			found = found || capability == want
		}
		if !found {
			t.Fatalf("capabilities = %v, lost %q on a narrower re-handoff", connections[0].Capabilities, want)
		}
	}
}

func TestSignInHandoffIsInternalOnlyAndValidated(t *testing.T) {
	graph := graphProfileServer(t)
	defer graph.Close()
	app, _ := signInHandoffApp(t, graph.URL)

	if resp, _ := postHandoff(t, app, `{"organizationId":"org-1","accessToken":"t"}`, false); resp.StatusCode != fiber.StatusUnauthorized {
		t.Fatalf("without internal key status = %d, want 401", resp.StatusCode)
	}
	if resp, body := postHandoff(t, app, `{"organizationId":"org-1"}`, true); resp.StatusCode != fiber.StatusBadRequest || body["error"].(map[string]any)["code"] != "handoff_incomplete" {
		t.Fatalf("missing token status = %d body %v, want 400 handoff_incomplete", resp.StatusCode, body)
	}
	if resp, _ := postHandoff(t, app, `{"organizationId":"org-1","accessToken":"t","expiresAt":"yesterday"}`, true); resp.StatusCode != fiber.StatusBadRequest {
		t.Fatalf("bad expiresAt status = %d, want 400", resp.StatusCode)
	}
	req := httptest.NewRequest("POST", "/internal/providers/shipping/sign-in-handoff", strings.NewReader(`{"organizationId":"org-1","accessToken":"t"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusNotFound {
		t.Fatalf("non-OAuth provider status = %d, want 404", resp.StatusCode)
	}
}
