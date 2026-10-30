package sitesources

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestSavePrunesExpiredSnapshotsBeforeCapacityCheck(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	for i := 0; i < maxEntries; i++ {
		old := clone(c.report)
		old.WebsiteID = fmt.Sprintf("web_%032x", i+10)
		old.ObservedAt = stamp(s.now().Add(-Freshness))
		s.store.state.Entries = append(s.store.state.Entries, old)
	}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal("expired snapshots blocked registered site", err)
	}
	if len(s.store.state.Entries) != 1 {
		t.Fatal(len(s.store.state.Entries))
	}
}
func TestRepositoryRejectsUnsafeFilesAndMalformedState(t *testing.T) {
	s, _, _, path := fixture(t)
	if _, err := open(path, "wrong_org"); err == nil {
		t.Fatal("wrong org")
	}
	for _, p := range []string{"relative.json", filepath.Join(filepath.Dir(path), "missing", "data.json")} {
		if _, err := open(p, "org_sites"); err == nil {
			t.Fatal(p)
		}
	}
	link := filepath.Join(filepath.Dir(path), "link")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := open(link, "org_sites"); err == nil {
		t.Fatal("symlink")
	}
	for _, raw := range []string{`{}`, `{"version":1,"organizationId":"org_sites","entries":null}`, `{"version":1,"organizationId":"org_sites","entries":[],"extra":1}`, `{"version":1,"version":1,"organizationId":"org_sites","entries":[]}`, strings.Repeat("x", maxBytes+1)} {
		if err := os.WriteFile(path, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := open(path, "org_sites"); err == nil {
			t.Fatal(raw)
		}
	}
	_ = os.Chmod(path, 0644)
	if _, err := open(path, "org_sites"); err == nil {
		t.Fatal("public file")
	}
	if err := s.store.persist(s.store.state); err == nil {
		t.Fatal("public persist")
	}
	_ = os.Chmod(path, 0600)
	_ = os.Chmod(filepath.Dir(path), 0755)
	if _, err := open(path, "org_sites"); err == nil {
		t.Fatal("public dir")
	}
	if err := s.store.persist(s.store.state); err == nil {
		t.Fatal("public dir persist")
	}
}
