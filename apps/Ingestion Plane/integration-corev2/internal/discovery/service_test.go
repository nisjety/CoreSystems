package discovery

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

func TestDiscoverSlackReturnsSafeMetadata(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/auth.test":
			_, _ = w.Write([]byte(`{"ok":true,"user":"Ima","user_id":"U1","team":"Velion","team_id":"T1"}`))
		case "/conversations.list":
			_, _ = w.Write([]byte(`{"ok":true,"channels":[{"id":"C1","name":"support"},{"id":"C2","name":"sales"}]}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{SlackAPIBaseURL: server.URL}, server.Client())
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:              "conn-1",
		ProviderKey:     "slack",
		ConnectorType:   "slack",
		Scopes:          []string{"team:read", "channels:read"},
		ProviderContext: map[string]string{},
	}, "token")
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	if snapshot.WorkspaceName != "Velion" {
		t.Fatalf("WorkspaceName = %q, want Velion", snapshot.WorkspaceName)
	}
	if snapshot.EntityCounts["public_channels_sampled"] != 2 {
		t.Fatalf("channel count = %d, want 2", snapshot.EntityCounts["public_channels_sampled"])
	}
	if len(snapshot.SampleEntities) != 2 || snapshot.SampleEntities[0].Label != "#support" {
		t.Fatalf("SampleEntities = %#v, want public channel samples", snapshot.SampleEntities)
	}
	if snapshot.Sensitivity != "safe_metadata_only" {
		t.Fatalf("Sensitivity = %q, want safe_metadata_only", snapshot.Sensitivity)
	}
}

func TestAvailabilityFromScopes(t *testing.T) {
	availability := availabilityFromScopes([]string{"Mail.Read", "Files.Read.All", "read_products", "read_orders"})
	for _, key := range []string{"mail", "documents", "products", "orders"} {
		if !availability[key] {
			t.Fatalf("availability[%s] = false, want true in %#v", key, availability)
		}
	}
}

func TestDiscoverMicrosoftDoesNotExposeEmailFallbacks(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v1.0/me":
			_, _ = w.Write([]byte(`{"id":"m1","mail":"ima@example.com","userPrincipalName":"ima@example.com"}`))
		case "/v1.0/me/joinedTeams":
			_, _ = w.Write([]byte(`{"value":[{"displayName":"Sensitive Team"}]}`))
		case "/v1.0/me/drive":
			_, _ = w.Write([]byte(`{"id":"drive-1","quota":{"used":12345}}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{MicrosoftGraphBaseURL: server.URL}, server.Client())
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:            "conn-ms",
		ProviderKey:   "microsoft",
		ConnectorType: "microsoft-graph",
		Scopes:        []string{"Mail.Read", "Files.Read.All", "Team.ReadBasic.All"},
	}, "token")
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	encoded := snapshotJSON(t, snapshot)
	if strings.Contains(encoded, "ima@example.com") || strings.Contains(encoded, "Sensitive Team") {
		t.Fatalf("snapshot leaked sensitive Microsoft metadata: %s", encoded)
	}
	if snapshot.AccountName != "" {
		t.Fatalf("AccountName = %q, want empty when only email fallback exists", snapshot.AccountName)
	}
	if !snapshot.Availability["mail"] || !snapshot.Availability["drive"] || snapshot.EntityCounts["drive_storage_used_bytes"] != 12345 {
		t.Fatalf("snapshot availability/counts = %#v/%#v, want safe high-level signals", snapshot.Availability, snapshot.EntityCounts)
	}
}

func TestDiscoverGoogleHidesDriveFolderNamesAndEmailFallback(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/oauth2/v3/userinfo":
			_, _ = w.Write([]byte(`{"sub":"g1","email":"ima@example.com"}`))
		case "/drive/v3/about":
			_, _ = w.Write([]byte(`{"storageQuota":{"usage":"9876"}}`))
		case "/drive/v3/files":
			_, _ = w.Write([]byte(`{"files":[{"id":"f1","name":"Private Board Deck","mimeType":"application/vnd.google-apps.folder"}]}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{GoogleAPIBaseURL: server.URL}, server.Client())
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:            "conn-google",
		ProviderKey:   "google",
		ConnectorType: "google-workspace",
		Scopes:        []string{"https://www.googleapis.com/auth/gmail.readonly", "https://www.googleapis.com/auth/drive.metadata.readonly"},
	}, "token")
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	encoded := snapshotJSON(t, snapshot)
	if strings.Contains(encoded, "ima@example.com") || strings.Contains(encoded, "Private Board Deck") {
		t.Fatalf("snapshot leaked sensitive Google metadata: %s", encoded)
	}
	if snapshot.AccountName != "" {
		t.Fatalf("AccountName = %q, want empty when only email fallback exists", snapshot.AccountName)
	}
	if snapshot.EntityCounts["folders_sampled"] != 1 || snapshot.EntityCounts["drive_storage_used_bytes"] != 9876 {
		t.Fatalf("EntityCounts = %#v, want folder count and storage only", snapshot.EntityCounts)
	}
	if len(snapshot.SampleEntities) != 0 {
		t.Fatalf("SampleEntities = %#v, want no Google Drive folder names", snapshot.SampleEntities)
	}
	if len(snapshot.ProviderWarnings) == 0 {
		t.Fatalf("ProviderWarnings = empty, want hidden-folder-name warning")
	}
}

func TestDiscoverGitHubReturnsSafeRepositoryAndOrgMetadata(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/user":
			_, _ = w.Write([]byte(`{"id":123,"login":"ima","name":"Ima"}`))
		case "/user/repos":
			_, _ = w.Write([]byte(`[{"full_name":"triodelab/coresystem"},{"full_name":"triodelab/velion"}]`))
		case "/user/orgs":
			_, _ = w.Write([]byte(`[{"login":"triodelab"},{"login":"openai"}]`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{GitHubAPIBaseURL: server.URL}, server.Client())
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:            "conn-github",
		ProviderKey:   "github",
		ConnectorType: "github",
		Scopes:        []string{"read:user", "user:email", "read:org", "public_repo", "repo"},
	}, "token")
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	if snapshot.AccountName != "Ima" || snapshot.WorkspaceID != "123" {
		t.Fatalf("snapshot identity = %q/%q, want Ima/123", snapshot.AccountName, snapshot.WorkspaceID)
	}
	if snapshot.EntityCounts["repositories_sampled"] != 2 || snapshot.EntityCounts["organizations_sampled"] != 2 {
		t.Fatalf("EntityCounts = %#v, want repo and org counts", snapshot.EntityCounts)
	}
	for _, key := range []string{"profile", "email", "organizations", "repositories", "private_repositories"} {
		if !snapshot.Availability[key] {
			t.Fatalf("Availability[%s] = false in %#v", key, snapshot.Availability)
		}
	}
}

func TestDiscoverLinkedInReturnsProfileAndOrgMetadata(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/v2/userinfo":
			_ = json.NewEncoder(w).Encode(map[string]any{"sub": "member-1", "name": "Ima Da Costa", "email": "ima@example.com"})
		case "/rest/organizationAcls":
			if got := r.URL.Query().Get("q"); got != "roleAssignee" {
				t.Fatalf("q = %q, want roleAssignee", got)
			}
			_ = json.NewEncoder(w).Encode(map[string]any{"elements": []map[string]any{
				{"organization": "urn:li:organization:123", "role": "ADMINISTRATOR", "state": "APPROVED"},
				{"organization": "urn:li:organization:456", "role": "CONTENT_ADMINISTRATOR", "state": "APPROVED"},
			}})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{LinkedInAPIBaseURL: server.URL, LinkedInMarketingVersion: "202606"}, server.Client())
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:            "conn-linkedin",
		ProviderKey:   "linkedin",
		ConnectorType: "linkedin",
		Scopes:        []string{"openid", "profile", "email", "w_member_social", "r_organization_social"},
	}, "token")
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	if snapshot.AccountName != "Ima Da Costa" || snapshot.WorkspaceID != "member-1" {
		t.Fatalf("snapshot identity = %q/%q, want Ima Da Costa/member-1", snapshot.AccountName, snapshot.WorkspaceID)
	}
	if snapshot.EntityCounts["organizations_sampled"] != 2 {
		t.Fatalf("EntityCounts = %#v, want organizations_sampled=2", snapshot.EntityCounts)
	}
	for _, key := range []string{"profile", "email", "publishing", "organizations"} {
		if !snapshot.Availability[key] {
			t.Fatalf("Availability[%s] = false in %#v", key, snapshot.Availability)
		}
	}
}

// Providers without a browsable source tree (social/identity providers, or
// anything without a dedicated walker) resolve to an honest identity snapshot
// instead of an error — a healthy connection must not 502 the discover step.
func TestDiscoverFallsBackToIdentitySnapshot(t *testing.T) {
	service := NewService(config.Config{}, &http.Client{Timeout: time.Second})
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:          "conn_1",
		ProviderKey: "discord",
		DisplayName: "Ima Da Costa",
	}, "token")
	if err != nil {
		t.Fatalf("Discover identity fallback error: %v", err)
	}
	if snapshot.AccountName == "" {
		t.Fatalf("identity snapshot missing account name")
	}
	if snapshot.EntityCounts["accounts"] != 1 {
		t.Fatalf("identity snapshot accounts = %d, want 1", snapshot.EntityCounts["accounts"])
	}
	if len(snapshot.ProviderWarnings) == 0 {
		t.Fatalf("identity snapshot should carry a no-source-tree warning")
	}
}

func TestDiscoverMetaReturnsSafeBusinessInventory(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/me":
			_, _ = w.Write([]byte(`{"id":"user-1","name":"Meta Admin"}`))
		case "/me/accounts":
			_, _ = w.Write([]byte(`{"data":[{"id":"page-1","name":"Velion Page","instagram_business_account":{"id":"ig-1","username":"velion"}}]}`))
		case "/me/adaccounts":
			_, _ = w.Write([]byte(`{"data":[{"id":"act_1","name":"Velion Ads"}]}`))
		case "/me/businesses":
			_, _ = w.Write([]byte(`{"data":[{"id":"biz-1","name":"Velion Business","owned_whatsapp_business_accounts":{"data":[{"id":"waba-1","name":"Velion WhatsApp"}]}}]}`))
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	service := NewService(config.Config{FacebookAPIBaseURL: server.URL}, server.Client())
	snapshot, err := service.Discover(context.Background(), store.Connection{
		ID:            "conn-meta",
		ProviderKey:   "meta",
		ConnectorType: "meta",
		Scopes:        []string{"pages_show_list", "pages_messaging", "instagram_basic", "whatsapp_business_management", "ads_read", "catalog_management", "threads_basic"},
	}, "token")
	if err != nil {
		t.Fatalf("Discover error: %v", err)
	}
	if snapshot.AccountName != "Meta Admin" || snapshot.WorkspaceID != "user-1" {
		t.Fatalf("snapshot identity = %q/%q, want Meta Admin/user-1", snapshot.AccountName, snapshot.WorkspaceID)
	}
	if snapshot.EntityCounts["pages_sampled"] != 1 || snapshot.EntityCounts["ad_accounts_sampled"] != 1 || snapshot.EntityCounts["businesses_sampled"] != 1 || snapshot.EntityCounts["whatsapp_business_accounts_sampled"] != 1 {
		t.Fatalf("EntityCounts = %#v, want Meta inventory counts", snapshot.EntityCounts)
	}
	for _, key := range []string{"pages", "messenger", "instagram", "whatsapp", "ads", "catalogs", "threads"} {
		if !snapshot.Availability[key] {
			t.Fatalf("Availability[%s] = false in %#v", key, snapshot.Availability)
		}
	}
	encoded := snapshotJSON(t, snapshot)
	if strings.Contains(encoded, "access_token") {
		t.Fatalf("snapshot leaked token field: %s", encoded)
	}
}

func snapshotJSON(t *testing.T, snapshot Snapshot) string {
	t.Helper()
	data, err := json.Marshal(snapshot)
	if err != nil {
		t.Fatalf("Marshal snapshot error: %v", err)
	}
	return string(data)
}
