package api

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/google/uuid"

	"github.com/triodelab/finspo/internal/sharepoint"
	"github.com/triodelab/finspo/internal/store"
)

type fakeSourceReader struct {
	src store.Source
	err error
}

func (f *fakeSourceReader) Get(_ context.Context, _ uuid.UUID) (store.Source, error) {
	return f.src, f.err
}

func (f *fakeSourceReader) ListByOrganization(_ context.Context, _ string) ([]store.Source, error) {
	return []store.Source{f.src}, f.err
}

type fakeSourceWriter struct {
	received []store.Source
	err      error
}

func (f *fakeSourceWriter) EnsureSource(_ context.Context, src store.Source) (store.Source, error) {
	f.received = append(f.received, src)
	if f.err != nil {
		return store.Source{}, f.err
	}
	src.ID = uuid.New()
	src.Kind = store.NormalizeKind(src.Kind)
	return src, nil
}

type fakeCursorReader struct{}

func (fakeCursorReader) Get(_ context.Context, _ uuid.UUID) (store.Cursor, error) {
	return store.Cursor{}, store.ErrNotFound
}

func newSourcesTestServer(writer *fakeSourceWriter) *fiberApp {
	app := NewServer(ServerConfig{
		APIKey:       "test-key",
		Browser:      &stubBrowser{},
		SourceReader: &fakeSourceReader{},
		SourceWriter: writer,
		CursorReader: fakeCursorReader{},
	})
	return &fiberApp{app: app}
}

// fiberApp wraps app.Test with the auth headers every request needs.
type fiberApp struct {
	app interface {
		Test(*http.Request, ...int) (*http.Response, error)
	}
}

func (f *fiberApp) do(t *testing.T, method, target string, body any) *http.Response {
	t.Helper()
	var reader *bytes.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			t.Fatalf("marshal body: %v", err)
		}
		reader = bytes.NewReader(encoded)
	} else {
		reader = bytes.NewReader(nil)
	}
	req := httptest.NewRequest(method, target, reader)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("x-internal-api-key", "test-key")
	req.Header.Set("X-Org-ID", "org-123")
	resp, err := f.app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	return resp
}

func TestCreateSourceAcceptsFolderScope(t *testing.T) {
	t.Parallel()

	writer := &fakeSourceWriter{}
	server := newSourcesTestServer(writer)

	resp := server.do(t, http.MethodPost, "/api/v1/sources", map[string]any{
		"site_id":     "site-1",
		"drive_id":    "b!drive-1",
		"drive_name":  "Documents",
		"folder_id":   "folder-1",
		"folder_path": "Contracts/2026/",
	})
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusCreated)
	}
	if len(writer.received) != 1 {
		t.Fatalf("writer calls = %d, want 1", len(writer.received))
	}
	got := writer.received[0]
	if got.Kind != store.SourceKindDrive {
		t.Fatalf("kind = %q, want drive", got.Kind)
	}
	if got.FolderID != "folder-1" || got.FolderPath != "/Contracts/2026" {
		t.Fatalf("folder scope = (%q, %q), want (folder-1, /Contracts/2026)", got.FolderID, got.FolderPath)
	}
	if got.OrganizationID != "org-123" {
		t.Fatalf("org = %q, want org-123 (from the auth header, never the body)", got.OrganizationID)
	}
}

func TestCreateSourceRejectsHalfFolderScope(t *testing.T) {
	t.Parallel()

	for name, body := range map[string]map[string]any{
		"folder_id without path": {
			"site_id":   "site-1",
			"drive_id":  "b!drive-1",
			"folder_id": "folder-1",
		},
		"folder_path without id": {
			"site_id":     "site-1",
			"drive_id":    "b!drive-1",
			"folder_path": "/Contracts",
		},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			writer := &fakeSourceWriter{}
			server := newSourcesTestServer(writer)

			resp := server.do(t, http.MethodPost, "/api/v1/sources", body)
			defer resp.Body.Close()

			if resp.StatusCode != http.StatusBadRequest {
				t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
			}
			if len(writer.received) != 0 {
				t.Fatalf("writer must not be called on invalid input")
			}
		})
	}
}

func TestCreateSourceAcceptsSitePagesKind(t *testing.T) {
	t.Parallel()

	writer := &fakeSourceWriter{}
	server := newSourcesTestServer(writer)

	resp := server.do(t, http.MethodPost, "/api/v1/sources", map[string]any{
		"kind":         "site_pages",
		"site_id":      "site-1",
		"site_web_url": "https://sp/sites/intranet",
		"drive_name":   "Intranet · Site pages",
	})
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusCreated)
	}
	got := writer.received[0]
	if got.Kind != store.SourceKindSitePages {
		t.Fatalf("kind = %q, want site_pages", got.Kind)
	}
	if got.DriveID != "" {
		t.Fatalf("drive id = %q, want empty for site pages", got.DriveID)
	}
}

func TestCreateSourceRejectsSitePagesWithDriveScope(t *testing.T) {
	t.Parallel()

	writer := &fakeSourceWriter{}
	server := newSourcesTestServer(writer)

	resp := server.do(t, http.MethodPost, "/api/v1/sources", map[string]any{
		"kind":     "site_pages",
		"site_id":  "site-1",
		"drive_id": "b!drive-1",
	})
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestCreateSourceRejectsUnknownKind(t *testing.T) {
	t.Parallel()

	writer := &fakeSourceWriter{}
	server := newSourcesTestServer(writer)

	resp := server.do(t, http.MethodPost, "/api/v1/sources", map[string]any{
		"kind":     "carrier_pigeon",
		"site_id":  "site-1",
		"drive_id": "b!drive-1",
	})
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", resp.StatusCode, http.StatusBadRequest)
	}
}

func TestListChildrenRouteProxiesToBrowser(t *testing.T) {
	t.Parallel()

	browser := &stubBrowser{folders: []sharepoint.Folder{{
		ID:         "folder-1",
		Name:       "Contracts",
		Path:       "/Contracts",
		ChildCount: 3,
	}}}
	app := NewServer(ServerConfig{
		APIKey:  "test-key",
		Browser: browser,
	})

	req := httptest.NewRequest(http.MethodGet, "/api/v1/sharepoint/drives/b!drive-1/children?item_id=item-9", nil)
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
	if browser.receivedDriveID != "b!drive-1" || browser.receivedItemID != "item-9" {
		t.Fatalf("browser received (%q, %q), want (b!drive-1, item-9)", browser.receivedDriveID, browser.receivedItemID)
	}

	var payload struct {
		Success bool `json:"success"`
		Data    struct {
			Count   int                 `json:"count"`
			Folders []sharepoint.Folder `json:"folders"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&payload); err != nil {
		t.Fatalf("decode body: %v", err)
	}
	if !payload.Success || payload.Data.Count != 1 || payload.Data.Folders[0].Path != "/Contracts" {
		t.Fatalf("payload = %+v, want one /Contracts folder", payload)
	}
}
