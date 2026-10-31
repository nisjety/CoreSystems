package algoliamapping

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type fakeRegistry struct{ record websites.Record }

func (r *fakeRegistry) Resolve(id, site string) (websites.Record, error) {
	if id != r.record.Website.ID || site != r.record.Website.Site {
		return websites.Record{}, websites.ErrNotFound
	}
	return r.record, nil
}
func (*fakeRegistry) OrganizationID() string { return "org_maps" }

type fakeClient struct {
	access  Access
	allowed bool
	err     error
	calls   int
	report  sitesources.Report
	after   func()
}

func (c *fakeClient) Access(context.Context, string) (Access, error) {
	c.calls++
	return c.access, c.err
}
func (c *fakeClient) Allowed(context.Context, string, string, string) (bool, error) {
	if c.after != nil {
		c.after()
	}
	return c.allowed, c.err
}
func (c *fakeClient) Report(context.Context, websites.Record, Mapping) (sitesources.Report, error) {
	return c.report, c.err
}
func fixture(t *testing.T) (*Service, *fakeRegistry, *fakeClient, string) {
	t.Helper()
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	path := filepath.Join(dir, "algolia-mappings.json")
	r := &fakeRegistry{websites.Record{Website: websites.Website{ID: "web_00000000000000000000000000000001", Site: "https://triodelab.no/", Revision: 7, Ownership: websites.Ownership{Status: "google_owner"}}}}
	c := &fakeClient{access: Access{ConnectionID: "conn_1234", Status: "active", Generation: "grant-1"}, allowed: true}
	s, err := New(path, "org_maps", r, c)
	if err != nil {
		t.Fatal(err)
	}
	return s, r, c, path
}
func TestMappingValidatedDurableAndTombstoneBlocksFallback(t *testing.T) {
	s, r, c, path := fixture(t)
	w := r.record.Website
	initial, err := s.Get(t.Context(), w.ID, w.Site)
	if err != nil || initial.Status != "not_mapped" || c.calls != 0 {
		t.Fatal(initial, err)
	}
	m, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab")
	if err != nil || m.Status != "mapped" {
		t.Fatal(m, err)
	}
	a, err := s.Authorize(t.Context(), r.record)
	if err != nil || !a.Managed || !a.Active || a.Generation != "grant-1" {
		t.Fatal(a, err)
	}
	restored, err := New(path, "org_maps", r, c)
	if err != nil {
		t.Fatal(err)
	}
	got, err := restored.Get(t.Context(), w.ID, w.Site)
	if err != nil || got.ConnectionID != m.ConnectionID {
		t.Fatal(got, err)
	}
	deleted, err := s.Delete(w.ID, w.Site, w.Revision)
	if err != nil || deleted.Status != "not_mapped" || deleted.ConnectionID != "" {
		t.Fatal(deleted, err)
	}
	a, err = s.Authorize(t.Context(), r.record)
	if err != nil || !a.Managed || a.Active {
		t.Fatal("tombstone permits fallback", a, err)
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0600 {
		t.Fatal(info.Mode())
	}
}
func TestMappingFailsClosedOnWrongIdentityResourceRevocationAndRace(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	c.allowed = false
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); !errors.Is(err, ErrUnavailable) {
		t.Fatal(err)
	}
	c.allowed = true
	c.after = func() { r.record.Website.Revision++ }
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); !errors.Is(err, websites.ErrConflict) {
		t.Fatal(err)
	}
	c.after = nil
	w = r.record.Website
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); err != nil {
		t.Fatal(err)
	}
	c.access.Status = "reconnect_required"
	got, err := s.Get(t.Context(), w.ID, w.Site)
	if err != nil || got.Status != "reconnect_required" {
		t.Fatal(got, err)
	}
	c.err = errors.New("private provider topology")
	got, err = s.Get(t.Context(), w.ID, w.Site)
	if err != nil || got.Status != "access_check_required" {
		t.Fatal(got, err)
	}
	r.record.Website.Ownership.Status = "unverified"
	if got, err := s.Get(t.Context(), w.ID, w.Site); err != nil || got.Status != "access_check_required" {
		t.Fatal(err)
	}
}
func TestSnapshotBindingAndMappingReplacementInvalidateSavedReceipt(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); err != nil {
		t.Fatal(err)
	}
	a, err := s.Authorize(t.Context(), r.record)
	if err != nil {
		t.Fatal(err)
	}
	report := sitesources.Report{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Provider: "algolia", ObservedAt: time.Now().UTC().Format(time.RFC3339Nano)}
	if err = s.BindSnapshot(r.record, a, report); err != nil {
		t.Fatal(err)
	}
	got, err := s.Authorize(t.Context(), r.record)
	if err != nil || got.SnapshotObservedAt != report.ObservedAt {
		t.Fatal(got, err)
	}
	c.access.Generation = "grant-2"
	got, err = s.Authorize(t.Context(), r.record)
	if err != nil || got.SnapshotObservedAt != "" {
		t.Fatal(got, err)
	}
	if _, err = s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); err != nil {
		t.Fatal(err)
	}
	if err = s.BindSnapshot(r.record, a, report); !errors.Is(err, websites.ErrConflict) {
		t.Fatal(err)
	}
}
func TestDisconnectWinsAgainstAlreadyInFlightResourceValidation(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	c.after = func() {
		if _, err := s.Delete(w.ID, w.Site, w.Revision); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "triodelab"); !errors.Is(err, websites.ErrConflict) {
		t.Fatal("pending mapping resurrected tombstone", err)
	}
	a, err := s.Authorize(t.Context(), r.record)
	if err != nil || !a.Managed || a.Active {
		t.Fatal(a, err)
	}
}
