package algoliamapping

import (
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestStrictAccessAndResourceReceipts(t *testing.T) {
	access := `{"success":true,"data":{"connectionId":"conn_1234","status":"active","generation":"grant-1"}}`
	if _, err := DecodeAccess([]byte(access), "conn_1234"); err != nil {
		t.Fatal(err)
	}
	allowed := `{"success":true,"data":{"connectionId":"conn_1234","applicationId":"APP123","indexName":"triodelab","status":"allowed"}}`
	if yes, err := DecodeAllowed([]byte(allowed), "conn_1234", "APP123", "triodelab"); err != nil || !yes {
		t.Fatal(yes, err)
	}
	if yes, err := DecodeAllowed([]byte(strings.Replace(allowed, `"allowed"`, `"unavailable"`, 1)), "conn_1234", "APP123", "triodelab"); err != nil || yes {
		t.Fatal(yes, err)
	}
	for _, raw := range []string{`null`, `[]`, access + `{}`, strings.Repeat("x", (64<<10)+1), strings.Replace(access, `"success":true`, `"success":false`, 1), strings.Replace(access, `"status":"active"`, `"status":"deleted"`, 1), strings.Replace(access, `"generation":"grant-1"`, `"generation":null`, 1), strings.Replace(access, `"generation":"grant-1"`, `"generation":"a b"`, 1), strings.Replace(access, `"generation":"grant-1"`, `"generation":"grant-1","token":"bad"`, 1), strings.Replace(access, `"status":"active"`, `"status":"active","status":"active"`, 1)} {
		if _, err := DecodeAccess([]byte(raw), "conn_1234"); err == nil {
			t.Fatal(raw)
		}
	}
	if _, err := DecodeAccess([]byte(access), "conn_other"); err == nil {
		t.Fatal("wrong connection")
	}
	for _, raw := range []string{`{}`, strings.Replace(allowed, `"indexName":"triodelab"`, `"indexName":"other"`, 1), strings.Replace(allowed, `"status":"allowed"`, `"status":"maybe"`, 1)} {
		if _, err := DecodeAllowed([]byte(raw), "conn_1234", "APP123", "triodelab"); err == nil {
			t.Fatal(raw)
		}
	}
}
func TestRepositoryRejectsUnsafeStateAndLocalFiles(t *testing.T) {
	s, _, _, path := fixture(t)
	if _, err := open(path, "wrong_org"); err == nil {
		t.Fatal("wrong org")
	}
	if _, err := open("relative", "org_maps"); err == nil {
		t.Fatal("relative path")
	}
	link := filepath.Join(filepath.Dir(path), "link")
	_ = os.Symlink(path, link)
	if _, err := open(link, "org_maps"); err == nil {
		t.Fatal("symlink")
	}
	for _, raw := range []string{`{}`, `{"version":1,"organizationId":"org_maps","entries":null}`, `{"version":1,"organizationId":"org_maps","entries":[],"token":"bad"}`, `{"version":1,"version":1,"organizationId":"org_maps","entries":[]}`, strings.Repeat("x", maxBytes+1)} {
		_ = os.WriteFile(path, []byte(raw), 0600)
		if _, err := open(path, "org_maps"); err == nil {
			t.Fatal(raw)
		}
	}
	_ = os.Chmod(path, 0644)
	if _, err := open(path, "org_maps"); err == nil {
		t.Fatal("public file")
	}
	if err := s.store.persist(s.store.state); err == nil {
		t.Fatal("public persist")
	}
	_ = os.Chmod(path, 0600)
	_ = os.Chmod(filepath.Dir(path), 0755)
	if _, err := open(path, "org_maps"); err == nil {
		t.Fatal("public dir")
	}
	if err := s.store.persist(s.store.state); err == nil {
		t.Fatal("public dir persist")
	}
}
func TestOAuthCollectionAndBindingRequireCurrentSelection(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); err != nil {
		t.Fatal(err)
	}
	a, err := s.Authorize(t.Context(), r.record)
	if err != nil {
		t.Fatal(err)
	}
	c.report = sitesources.Report{Provider: "algolia"}
	if got, err := s.CollectOAuth(t.Context(), r.record, a); err != nil || got.Provider != "algolia" {
		t.Fatal(got, err)
	}
	a.Active = false
	if _, err = s.CollectOAuth(t.Context(), r.record, a); !errors.Is(err, ErrUnavailable) {
		t.Fatal(err)
	}
	if err = s.BindSnapshot(r.record, a, c.report); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
	if _, err = s.Delete(w.ID, w.Site, w.Revision+1); err == nil {
		t.Fatal("stale delete")
	}
	if _, err = s.Put(t.Context(), w.ID, w.Site, w.Revision, "bad", "APP123", "triodelab"); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
	if _, err = s.Get(t.Context(), "invalid", w.Site); !errors.Is(err, ErrInvalid) {
		t.Fatal(err)
	}
	if _, err = New(s.store.path, "wrong_org", r, c); err == nil {
		t.Fatal("wrong scoped registry")
	}
	if _, err = New(s.store.path, "org_maps", nil, c); err == nil {
		t.Fatal("nil registry")
	}
	state := s.store.state
	state.Entries = append(state.Entries, state.Entries[0])
	if validState(state, "org_maps") {
		t.Fatal("duplicate mapping")
	}
	bad := s.store.state.Entries[0]
	bad.SnapshotObservedAt = "invalid"
	bad.SnapshotGeneration = "g"
	bad.SnapshotFingerprint = fingerprint(bad)
	bad.SnapshotDigest = strings.Repeat("0", 64)
	if validState(diskState{Version: 1, OrganizationID: "org_maps", Entries: []entry{bad}}, "org_maps") {
		t.Fatal("bad timestamp")
	}
}
