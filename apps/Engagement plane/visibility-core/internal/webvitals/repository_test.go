package webvitals

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestRepositoryRejectsUnsafeStorageAndCorruption(t *testing.T) {
	s, _, _, path := fixture(t)
	if _, err := open(path, "other_org"); err == nil {
		t.Fatal("cross org opened")
	}
	for _, p := range []string{"relative.json", filepath.Join(filepath.Dir(path), "missing", "store.json")} {
		if _, err := open(p, "org_test"); err == nil {
			t.Fatal(p)
		}
	}
	symlink := filepath.Join(filepath.Dir(path), "link.json")
	if err := os.Symlink(path, symlink); err != nil {
		t.Fatal(err)
	}
	if _, err := open(symlink, "org_test"); err == nil {
		t.Fatal("symlink accepted")
	}
	for _, raw := range []string{`{}`, `{"version":1,"organizationId":"org_test","entries":null}`, `{"version":1,"organizationId":"org_test","entries":[],"extra":1}`, `[]`, `{} {}`} {
		if err := os.WriteFile(path, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := open(path, "org_test"); err == nil {
			t.Fatal(raw)
		}
	}
	if err := os.WriteFile(path, []byte(strings.Repeat("x", maxBytes+1)), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := open(path, "org_test"); err == nil {
		t.Fatal("oversize accepted")
	}
	_ = os.Chmod(path, 0644)
	if _, err := open(path, "org_test"); err == nil {
		t.Fatal("world-readable accepted")
	}
	if err := s.store.persist(s.store.state); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
	_ = os.Chmod(path, 0600)
	_ = os.Chmod(filepath.Dir(path), 0755)
	if _, err := open(path, "org_test"); err == nil {
		t.Fatal("public dir accepted")
	}
	if err := s.store.persist(s.store.state); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
}
func TestValidOriginAndSampleBoundaries(t *testing.T) {
	for _, tc := range []struct {
		origin, site, env string
		valid             bool
	}{
		{"https://triodelab.no", "https://triodelab.no/", "production", true}, {"https://www.triodelab.no", "https://triodelab.no/", "production", true},
		{"https://www.triodelab.no/", "https://triodelab.no/", "production", false}, {"https://www.triodelab.no:443", "https://triodelab.no/", "production", false},
		{"https://triodelab.no?x=1", "https://triodelab.no/", "production", false}, {"http://triodelab.no", "https://triodelab.no/", "production", false},
		{"https://triodelab.no?", "https://triodelab.no/", "production", false}, {"https://triodelab.no#", "https://triodelab.no/", "production", false},
		{"http://localhost:3000", "https://triodelab.no/", "development", true}, {"http://127.0.0.1:3000", "https://triodelab.no/", "development", true},
		{"http://localhost:3001", "https://triodelab.no/", "development", true}, {"http://localhost:3001", "https://triodelab.no/", "production", false}, {"http://localhost:3002", "https://triodelab.no/", "development", false}, {"http://user@localhost:3000", "https://triodelab.no/", "development", false},
		{"https://triodelab.no", "https://triodelab.no/", "staging", false},
	} {
		if validOrigin(tc.origin, tc.site, tc.env) != tc.valid {
			t.Fatal(tc)
		}
	}
	for _, bad := range []float64{math.NaN(), math.Inf(1), -1, 120001} {
		v := sample(0, bad, "2026-10-23T00:00:00Z")
		if validSample(v) {
			t.Fatal(v)
		}
	}
	s, r, c, _ := fixture(t)
	w := r.record.Website
	if _, err := s.Get("invalid", w.Site, "production"); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
	if _, err := s.Get(w.ID, "https://evil.no/", "production"); err == nil {
		t.Fatal("cross site")
	}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision+1, "production"); err == nil {
		t.Fatal("stale sync")
	}
	c.err = fmt.Errorf("network unavailable")
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); err == nil {
		t.Fatal("upstream failure ignored")
	}
	s.collector = nil
	s.now = func() time.Time { return time.Date(2026, 10, 23, 13, 0, 0, 0, time.UTC) }
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
}
func TestDirectoryOpenFailureAfterRenameIsCommitted(t *testing.T) {
	// Rename has already replaced the durable file before this final step. An
	// unavailable directory handle must not leave in-memory state on the old copy.
	err := syncCommittedDirectory(filepath.Join(t.TempDir(), "missing"))
	if !errors.Is(err, errCommitted) {
		t.Fatalf("post-rename failure must preserve commit provenance: %v", err)
	}
	if err := syncCommittedDirectory(t.TempDir()); err != nil {
		t.Fatal(err)
	}
}
func TestReceiptRejectsMissingDuplicateUnknownAndNullFields(t *testing.T) {
	_, _, c, _ := fixture(t)
	c.data.Samples = []Sample{sample(0, 0, c.data.ObservedAt)}
	raw, _ := json.Marshal(map[string]any{"success": true, "data": c.data})
	if _, err := DecodeReceipt(raw); err != nil {
		t.Fatal(err, string(raw))
	}
	for _, bad := range []string{
		strings.Replace(string(raw), `"value":0,`, "", 1), strings.Replace(string(raw), `"value":0`, `"value":null`, 1),
		strings.Replace(string(raw), `"value":0`, `"value":0,"value":1`, 1), strings.Replace(string(raw), `"value":0`, `"value":0,"ip":"no"`, 1),
		strings.Replace(string(raw), `"success":true`, `"success":false`, 1), strings.Replace(string(raw), `"samples":[`, `"samples":null,"other":[`, 1),
		string(raw) + " {}", `{"data":{},"success":true}`, `null`,
	} {
		if _, err := DecodeReceipt([]byte(bad)); err == nil {
			t.Fatal(bad)
		}
	}
}
func TestStorageBudgetAndDuplicateRevisionEntries(t *testing.T) {
	s, _, c, _ := fixture(t)
	e := c.data
	e.Samples = make([]Sample, maxSamples+1)
	if err := s.store.save(e, s.now()); !errors.Is(err, ErrCapacity) {
		t.Fatal(err)
	}
	if validState(diskState{Version: 1, OrganizationID: "org_test", Entries: []Export{c.data, c.data}}, "org_test") {
		t.Fatal("duplicate entry")
	}
	if _, err := New(s.store.path, "wrong-org", s.registry, c, nil); err == nil {
		t.Fatal("wrong registry")
	}
	if _, err := New(s.store.path, "org_test", nil, c, nil); err == nil {
		t.Fatal("missing registry")
	}
}
func TestRetentionDeletesPayloadOnReadAndRestart(t *testing.T) {
	s, r, c, path := fixture(t)
	w := r.record.Website
	c.data.Samples = []Sample{sample(0, 100, "2026-10-17T00:00:00Z")}
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); err != nil {
		t.Fatal(err)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 24, 0, 0, 0, 0, time.UTC) }
	if _, err := s.Get(w.ID, w.Site, "production"); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(path)
	if err != nil || strings.Contains(string(raw), c.data.Samples[0].SampleID) {
		t.Fatal(string(raw), err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	s.Run(ctx)
	if _, err = New(path, "org_test", r, c, s.now); err != nil {
		t.Fatal(err)
	}
}
