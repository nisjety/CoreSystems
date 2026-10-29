package api

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/auth"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

var tenantMutations = []struct {
	name, method, path, body string
	want                     int
}{
	{"capabilities", http.MethodPatch, "/api/v1/connections/conn-guard/capabilities", `{"capabilities":["profile.read","mail.read"]}`, http.StatusOK},
	{"disconnect", http.MethodDelete, "/api/v1/connections/conn-guard", "", http.StatusNoContent},
	{"consents", http.MethodPost, "/api/v1/connections/conn-guard/consents", `{"source":"mail","purpose":"inbox","granted":true}`, http.StatusOK},
	{"scim-create", http.MethodPost, "/api/v1/scim/tokens", `{"organizationId":"org-1","name":"fixture"}`, http.StatusCreated},
	{"scim-revoke", http.MethodDelete, "/api/v1/scim/tokens/scim-guard?organizationId=org-1", "", http.StatusOK},
}

func mutationFixture(t *testing.T, principal auth.Principal) (*store.MemoryRepository, func(string, string, string) *http.Response) {
	t.Helper()
	cfg, repo, oauthService := testOAuthStack(t)
	_, err := repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-guard", ProviderKey: "microsoft", ConnectorType: "microsoft-graph",
		OrganizationID: "org-1", WorkspaceID: "org-1", UserID: "user-1", Status: "active",
		Capabilities: []string{"profile.read"}, AccessTokenExpiresAt: time.Now().Add(time.Hour),
	})
	if err != nil {
		t.Fatal(err)
	}
	_, err = repo.CreateSCIMToken(t.Context(), store.SCIMToken{
		ID: "scim-guard", OrganizationID: "org-1", Name: "existing", CreatedBy: "user-1",
		CreatedAt: time.Now().UTC(), UpdatedAt: time.Now().UTC(),
	}, scimTokenHash("synthetic-scim-bearer"))
	if err != nil {
		t.Fatal(err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: oauthService, Auth: fakeVerifier{principal: principal}})
	return repo, func(method, path, body string) *http.Response {
		t.Helper()
		request := httptest.NewRequest(method, path, strings.NewReader(body))
		request.Header.Set("Authorization", "Bearer synthetic-service-token")
		request.Header.Set("Content-Type", "application/json")
		response, err := app.Test(request)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { response.Body.Close() })
		return response
	}
}

func TestReadOnlyServiceCannotMutateTenantIntegrations(t *testing.T) {
	for _, operation := range tenantMutations {
		t.Run(operation.name, func(t *testing.T) {
			repo, request := mutationFixture(t, auth.Principal{
				UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"},
			})
			response := request(operation.method, operation.path, operation.body)
			if response.StatusCode != http.StatusForbidden {
				t.Fatalf("status = %d, want 403", response.StatusCode)
			}
			var envelope struct {
				Error struct {
					Code string `json:"code"`
				} `json:"error"`
			}
			if err := json.NewDecoder(response.Body).Decode(&envelope); err != nil || envelope.Error.Code != "integration_write_scope_required" {
				t.Fatalf("expected explicit scope denial, got %q (%v)", envelope.Error.Code, err)
			}
			connection, err := repo.GetConnection(t.Context(), "conn-guard")
			if err != nil || connection.Status != "active" || !slices.Equal(connection.Capabilities, []string{"profile.read"}) {
				t.Fatal("denied request changed connection state")
			}
			consents, err := repo.ListConnectionConsents(t.Context(), "conn-guard")
			if err != nil || len(consents) != 0 {
				t.Fatal("denied request changed consents")
			}
			tokens, err := repo.ListSCIMTokens(t.Context(), "org-1")
			if err != nil || len(tokens) != 1 || tokens[0].RevokedAt != nil {
				t.Fatal("denied request changed SCIM token state")
			}
		})
	}
}

func TestTenantMutationGuardPreservesWritersAndInteractiveUsers(t *testing.T) {
	for _, principal := range []auth.Principal{
		{UserID: "service:writer", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"}},
		{UserID: "user-1", OrganizationID: "org-1", PrincipalType: "user"},
	} {
		for _, operation := range tenantMutations {
			t.Run(principal.PrincipalType+"/"+operation.name, func(t *testing.T) {
				_, request := mutationFixture(t, principal)
				if response := request(operation.method, operation.path, operation.body); response.StatusCode != operation.want {
					t.Fatalf("status = %d, want %d", response.StatusCode, operation.want)
				}
			})
		}
	}
}

func TestTenantMutationGuardDoesNotBypassOrgAccess(t *testing.T) {
	for _, principalType := range []string{"service", "user"} {
		for _, operation := range tenantMutations {
			if operation.name == "scim-create" {
				continue // SCIM creation derives the target from the principal's org.
			}
			t.Run(principalType+"/"+operation.name, func(t *testing.T) {
				_, request := mutationFixture(t, auth.Principal{
					UserID: "other-actor", OrganizationID: "org-other", PrincipalType: principalType, Scopes: []string{"integration:write"},
				})
				want := http.StatusForbidden
				if operation.name == "scim-revoke" {
					want = http.StatusNotFound // Token lookup is scoped to org-other.
				}
				if response := request(operation.method, operation.path, operation.body); response.StatusCode != want {
					t.Fatalf("status = %d, want %d", response.StatusCode, want)
				}
			})
		}
	}
}

func TestReadOnlyServiceStillReadsIntegrationMetadata(t *testing.T) {
	_, request := mutationFixture(t, auth.Principal{
		UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"},
	})
	for _, path := range []string{"/api/v1/connections", "/api/v1/connections/conn-guard/capabilities", "/api/v1/connections/conn-guard/consents", "/api/v1/scim/tokens"} {
		if response := request(http.MethodGet, path, ""); response.StatusCode != http.StatusOK {
			t.Fatalf("GET %s status = %d, want 200", path, response.StatusCode)
		}
	}
}

func TestReadOnlyServiceStillBeginsAuthorizedGoogleConsent(t *testing.T) {
	cfg := testConfig()
	cfg.GoogleClientID = "synthetic-google-client"
	cfg.GoogleClientSecret = "synthetic-google-client-secret"
	cfg.GoogleAuthorizationURL = "https://accounts.google.com/o/oauth2/v2/auth"
	cfg.GoogleTokenURL = "https://oauth2.googleapis.com/token"
	cfg.GoogleAPIBaseURL = "https://www.googleapis.com"
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatal(err)
	}
	repo := store.NewMemoryRepository()
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: oauth.NewService(cfg, repo, vault, nil),
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "service:marketing-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:read"},
		}}, Org: fakeOrgClient{plan: "expert"},
	})
	request := httptest.NewRequest(http.MethodPost, "/api/v1/providers/google/connect-session", strings.NewReader(`{"bundles":["search_console"]}`))
	request.Header.Set("Authorization", "Bearer synthetic-service-token")
	request.Header.Set("Content-Type", "application/json")
	response, err := app.Test(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		t.Fatalf("Google onboarding status = %d, want 200", response.StatusCode)
	}
	var envelope struct {
		Data struct {
			Capabilities []string `json:"capabilities"`
			Scopes       []string `json:"scopes"`
		} `json:"data"`
	}
	if err := json.NewDecoder(response.Body).Decode(&envelope); err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(envelope.Data.Capabilities, "search_console.read") || !slices.Contains(envelope.Data.Scopes, "https://www.googleapis.com/auth/webmasters.readonly") {
		t.Fatal("minimal read-only Search Console consent was not preserved")
	}
}
