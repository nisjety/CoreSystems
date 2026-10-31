package websites

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRegistryRejectsUnsafeFilesystemAndSchema(t *testing.T) {
	if _, err := Open("relative.json", "org-one"); err == nil {
		t.Fatal("relative storage accepted")
	}
	if _, err := Open(filepath.Join(privateDir(t), "sites.json"), "../org"); err == nil {
		t.Fatal("invalid organization accepted")
	}
	dir := privateDir(t)
	if err := os.Chmod(dir, 0755); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(filepath.Join(dir, "sites.json"), "org-one"); err == nil {
		t.Fatal("public directory accepted")
	}
	for _, tc := range []struct {
		name, raw string
		mode      os.FileMode
	}{
		{"public file", `{"version":1,"organizationId":"org-one","records":[]}`, 0644},
		{"unknown fields", `{"version":1,"organizationId":"org-one","records":[],"bearer":"synthetic"}`, 0600},
		{"trailing object", `{"version":1,"organizationId":"org-one","records":[]} {}`, 0600},
		{"nil records", `{"version":1,"organizationId":"org-one","records":null}`, 0600},
		{"version", `{"version":2,"organizationId":"org-one","records":[]}`, 0600},
		{"oversized", strings.Repeat("x", maxRegistryBytes+1), 0600},
	} {
		t.Run(tc.name, func(t *testing.T) {
			path := filepath.Join(privateDir(t), "sites.json")
			if err := os.WriteFile(path, []byte(tc.raw), tc.mode); err != nil {
				t.Fatal(err)
			}
			if _, err := Open(path, "org-one"); err == nil {
				t.Fatal("unsafe registry accepted")
			}
		})
	}
}

func TestRegistryInvalidBindingsAndCapacityFailClosed(t *testing.T) {
	stamp := "2026-10-21T12:00:00Z"
	for _, mutate := range []func(*Record){
		func(r *Record) { r.Website.ID = "web-invalid" },
		func(r *Record) { r.Website.Revision = 0 },
		func(r *Record) { r.Website.CreatedAt = "yesterday" },
		func(r *Record) { r.Website.Ownership.Status = "admin" },
		func(r *Record) { r.Website.Ownership = Ownership{Status: "google_owner", CheckedAt: stamp} },
		func(r *Record) {
			r.Website.Integrations.SearchConsole = Mapping{Status: "connected", Property: "sc-domain:evil.example", AccessLevel: "siteOwner"}
			r.GSCConnectionID = "conn-test"
		},
		func(r *Record) { r.GSCConnectionID = "conn-unverified" },
		func(r *Record) {
			r.Website.Integrations.GA4 = Mapping{Status: "connected", PropertyID: "not-numeric", StreamID: "1", MeasurementID: "G-ABCDE"}
			r.GA4ConnectionID = "conn-test"
		},
		func(r *Record) { r.GA4ConnectionID = "conn-unverified" },
		func(r *Record) { r.Website.Integrations.GA4.ReportingSite = "https://www.example.com/" },
		func(r *Record) { r.Website.Integrations.GA4.ReportingSite = "https://blog.example.com/" },
		func(r *Record) { r.Website.Integrations.PageSpeed.Status = "connected" },
		func(r *Record) { r.Website.Integrations.Clarity = Mapping{Status: "connected", ProjectID: "abcde"} },
		func(r *Record) { r.Website.Integrations.Clarity.ProjectID = "unverified" },
		func(r *Record) {
			r.DNS = &DNSChallenge{Name: "_studio-verification.evil.example", Value: "studio-verification=" + strings.Repeat("a", 43), ExpiresAt: stamp}
		},
	} {
		record := Record{Website: newWebsite("https://example.com/", stamp)}
		mutate(&record)
		if err := validateRecords([]Record{record}); err == nil {
			t.Fatalf("invalid binding accepted: %+v", record)
		}
	}
	records := []Record{}
	for i := range maxWebsites {
		records = append(records, Record{Website: newWebsite(fmt.Sprintf("https://site%d.example/", i), stamp)})
	}
	s := newTestService(t, nil)
	if err := s.repo.Update(func([]Record) ([]Record, error) { return records, nil }); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.Add(t.Context(), "https://overflow.example/"); !errors.Is(err, ErrInvalid) {
		t.Fatal("capacity exceeded")
	}
	if _, err := s.Change(t.Context(), records[0].Website.ID, records[1].Website.Site, 1); !errors.Is(err, ErrConflict) {
		t.Fatal("duplicate replacement accepted")
	}
}

func TestStaleDiscoveryCannotBindChangedWebsiteAndPublicResponsesHideConnections(t *testing.T) {
	s := newTestService(t, nil)
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	s.discover = func(ctx context.Context, _ string) Evidence {
		if _, err := s.MapClarity(w.ID, "abcde", w.Revision); err != nil {
			t.Fatal(err)
		}
		return Evidence{SearchConsole: Access{Status: "connected", ConnectionID: "private-gsc-connection", Property: "sc-domain:example.com", AccessLevel: "siteOwner"}, GA4: Access{Status: "requires_consent"}}
	}
	if _, err := s.Verify(t.Context(), w.ID); !errors.Is(err, ErrConflict) {
		t.Fatal("stale provider mapping applied")
	}
	s.discover = func(context.Context, string) Evidence {
		return Evidence{SearchConsole: Access{Status: "connected", ConnectionID: "private-gsc-connection", Property: "sc-domain:example.com", AccessLevel: "siteRestrictedUser"}, GA4: Access{Status: "connected", ConnectionID: "private-ga-connection", PropertyID: "123", StreamID: "456", MeasurementID: "G-ABCDE123"}}
	}
	value, err := s.Verify(t.Context(), w.ID)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(value)
	if strings.Contains(string(raw), "private-") || strings.Contains(string(raw), "Connection") || value.Ownership.Status != "unverified" {
		t.Fatalf("private grants escaped: %s", raw)
	}
	raw, _ = json.Marshal(s.List())
	if strings.Contains(string(raw), "private-") {
		t.Fatal("private connection in listing")
	}
	before := s.repo.List()
	before[0].DNS = &DNSChallenge{Name: "untrusted"}
	if s.repo.List()[0].DNS != nil {
		t.Fatal("mutable record escaped")
	}
}
