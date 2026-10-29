package providerleads

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type websiteRepoStub struct{ received int }

func (r *websiteRepoStub) ReceiveWebsiteInquiry(_ context.Context, _ WebsiteInstallation, v WebsiteInquiry, digest []byte) (WebsiteReceipt, error) {
	r.received++
	if len(digest) != 32 {
		panic("digest missing")
	}
	return WebsiteReceipt{ReceiptID: "plead_" + strings.Repeat("a", 24), AcceptedAt: "2026-10-28T12:00:00Z"}, nil
}
func (*websiteRepoStub) WebsiteOutcomes(context.Context, WebsiteInstallation, string, string) (WebsiteOutcomes, error) {
	return WebsiteOutcomes{}, nil
}
func installationFixture(t *testing.T) (*WebsiteService, *websiteRepoStub, string) {
	t.Helper()
	dir := t.TempDir()
	p := filepath.Join(dir, "bindings.json")
	b := WebsiteInstallation{OrganizationID: "org_test", WebsiteID: "web_" + strings.Repeat("a", 32), WebsiteRevision: 1, Site: "https://triodelab.no/", Environment: "development", WriteTokenFile: "write", ReadTokenFile: "read"}
	for name, value := range map[string]string{"write": strings.Repeat("w", 48), "read": strings.Repeat("r", 48)} {
		if e := os.WriteFile(filepath.Join(dir, name), []byte(value), 0600); e != nil {
			t.Fatal(e)
		}
	}
	raw, _ := json.Marshal(map[string]any{"version": 1, "installations": []WebsiteInstallation{b}})
	if e := os.WriteFile(p, raw, 0600); e != nil {
		t.Fatal(e)
	}
	repo := &websiteRepoStub{}
	svc, err := OpenWebsiteService(p, repo)
	if err != nil {
		t.Fatal(err)
	}
	return svc, repo, p
}
func TestWebsiteInstallationDirectionAndConfirmedValidation(t *testing.T) {
	s, r, _ := installationFixture(t)
	b, ok := s.Authorize(strings.Repeat("w", 48), true)
	if !ok {
		t.Fatal("write denied")
	}
	if _, ok = s.Authorize(strings.Repeat("w", 48), false); ok {
		t.Fatal("write credential can read")
	}
	if _, ok = s.Authorize(strings.Repeat("r", 48), true); ok {
		t.Fatal("read credential can write")
	}
	input := WebsiteInquiry{SubmissionID: "95572209-f995-45a3-93b8-b6e8f115b156", Name: "Test", Email: "test@example.com", Message: "A controlled local inquiry.", Source: "contact"}
	if _, err := s.Receive(t.Context(), b, input); err != nil || r.received != 1 {
		t.Fatal(err)
	}
	input.Email = "invalid"
	if _, err := s.Receive(t.Context(), b, input); err == nil || r.received != 1 {
		t.Fatal("invalid data received")
	}
	for _, pair := range [][2]string{{"2026-10-28", "2026-10-01"}, {"2026-02-30", "2026-10-01"}, {"2024-01-01", "2026-10-01"}} {
		if _, err := s.Report(t.Context(), b, pair[0], pair[1]); err == nil {
			t.Fatal("bad period accepted")
		}
	}
}
func TestUnsafeWebsiteSecretsRejected(t *testing.T) {
	_, r, p := installationFixture(t)
	if err := os.Chmod(filepath.Join(filepath.Dir(p), "write"), 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenWebsiteService(p, r); err == nil {
		t.Fatal("public credential accepted")
	}
}
