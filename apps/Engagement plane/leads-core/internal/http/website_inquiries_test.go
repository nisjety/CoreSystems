package http

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/providerleads"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type websiteHTTPRepo struct{ calls int }

func (r *websiteHTTPRepo) ReceiveWebsiteInquiry(context.Context, providerleads.WebsiteInstallation, providerleads.WebsiteInquiry, []byte) (providerleads.WebsiteReceipt, error) {
	r.calls++
	return providerleads.WebsiteReceipt{ReceiptID: "plead_" + strings.Repeat("a", 24), AcceptedAt: "2026-10-28T12:00:00Z"}, nil
}
func (*websiteHTTPRepo) WebsiteOutcomes(context.Context, providerleads.WebsiteInstallation, string, string) (providerleads.WebsiteOutcomes, error) {
	return providerleads.WebsiteOutcomes{}, nil
}
func TestWebsiteIntakeHasIsolatedCredentialsStrictBodyAndNoBrowserScope(t *testing.T) {
	dir := t.TempDir()
	write, read := strings.Repeat("w", 48), strings.Repeat("r", 48)
	_ = os.WriteFile(filepath.Join(dir, "write"), []byte(write), 0600)
	_ = os.WriteFile(filepath.Join(dir, "read"), []byte(read), 0600)
	raw, _ := json.Marshal(map[string]any{"version": 1, "installations": []providerleads.WebsiteInstallation{{OrganizationID: "org_test", WebsiteID: "web_" + strings.Repeat("a", 32), WebsiteRevision: 1, Site: "https://triodelab.no/", Environment: "development", WriteTokenFile: "write", ReadTokenFile: "read"}}})
	p := filepath.Join(dir, "bindings.json")
	_ = os.WriteFile(p, raw, 0600)
	repo := &websiteHTTPRepo{}
	service, err := providerleads.OpenWebsiteService(p, repo)
	if err != nil {
		t.Fatal(err)
	}
	h := NewHandler(&config.Config{}, nil)
	h.SetWebsiteInquiries(service)
	router := newRouter(h, "core-secret")
	body := `{"submissionId":"95572209-f995-45a3-93b8-b6e8f115b156","name":"Dev test","email":"test@example.com","message":"An isolated local test.","source":"contact"}`
	for _, test := range []struct {
		token, origin, body string
		want                int
	}{{write, "", body, 200}, {read, "", body, 401}, {"core-secret", "", body, 401}, {write, "http://localhost:3000", body, 401}, {write, "", strings.Replace(body, `"name"`, `"organizationId":"other","name"`, 1), 400}, {write, "", body + strings.Repeat(" ", 17000), 400}} {
		req := httptest.NewRequest("POST", "/api/v1/website-inquiries", bytes.NewBufferString(test.body))
		req.Header.Set("Authorization", "Bearer "+test.token)
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("Origin", test.origin)
		w := httptest.NewRecorder()
		router.ServeHTTP(w, req)
		if w.Code != test.want {
			t.Fatalf("wanted %d got %d", test.want, w.Code)
		}
		if strings.Contains(w.Body.String(), "test@example.com") || strings.Contains(w.Body.String(), write) {
			t.Fatal("inquiry response leaked fields")
		}
	}
	if repo.calls != 1 {
		t.Fatal("invalid request reached storage")
	}
	req := httptest.NewRequest(http.MethodPost, "/api/v1/website-inquiries", strings.NewReader(body))
	req.Header.Set("Authorization", write)
	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)
	if w.Code != 401 {
		t.Fatal("missing Bearer accepted")
	}
}
