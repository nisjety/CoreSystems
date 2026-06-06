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

func TestDiscoverRejectsUnknownProvider(t *testing.T) {
	service := NewService(config.Config{}, &http.Client{Timeout: time.Second})
	_, err := service.Discover(context.Background(), store.Connection{ProviderKey: "unknown"}, "token")
	if err == nil {
		t.Fatalf("expected unknown provider error")
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
