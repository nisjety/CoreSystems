package outcomes

import (
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type registryStub struct{ revision int64 }

func (r *registryStub) Resolve(id, site string) (websites.Record, error) {
	return websites.Record{Website: websites.Website{ID: id, Site: site, Revision: r.revision}}, nil
}

type transport func(*http.Request) (*http.Response, error)

func (f transport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func TestOutcomeProjectionScopesReadAndDropsPersonData(t *testing.T) {
	now := time.Date(2026, 10, 28, 12, 0, 0, 0, time.UTC)
	reg := &registryStub{1}
	file := filepath.Join(t.TempDir(), "read-token")
	_ = os.WriteFile(file, []byte(strings.Repeat("r", 48)), 0600)
	s, err := New(reg, "http://leads-core:3164", file)
	if err != nil {
		t.Fatal(err)
	}
	s.now = func() time.Time { return now }
	id, site := "web_"+strings.Repeat("a", 32), "https://triodelab.no/"
	calls := 0
	body := map[string]any{"websiteId": id, "websiteRevision": 1, "site": site, "environment": "development", "source": "leads_core", "status": "connected", "startDate": "2026-10-01", "endDate": "2026-10-28", "timeZone": "Europe/Oslo", "inquiries": 1, "leads": 1, "bookings": nil, "lastReceivedAt": nil, "asOf": now.Format(time.RFC3339), "email": "must-not-escape@example.com", "token": "private"}
	s.client.Transport = transport(func(r *http.Request) (*http.Response, error) {
		calls++
		if r.Method != "GET" || r.URL.Path != "/api/v1/website-outcomes" || r.URL.Query().Get("websiteId") != id || r.URL.Query().Get("environment") != "development" || r.Header.Get("Authorization") != "Bearer "+strings.Repeat("r", 48) {
			t.Fatal("invalid scoped request")
		}
		raw, _ := json.Marshal(map[string]any{"data": body})
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(string(raw)))}, nil
	})
	got, err := s.Get(context.Background(), id, site, "Q4", "development")
	if err != nil || got.Metrics == nil || got.Metrics.Inquiries != 1 || calls != 1 {
		t.Fatal(got, err)
	}
	raw, _ := json.Marshal(got)
	if strings.Contains(string(raw), "email") || strings.Contains(string(raw), "private") {
		t.Fatal("person fields escaped")
	}
	body["websiteRevision"] = 2
	got, err = s.Get(t.Context(), id, site, "Q4", "development")
	if err != nil || got.Metrics != nil || got.Status != "unavailable" {
		t.Fatal("foreign revision accepted")
	}
	body["websiteRevision"] = 1
	body["asOf"] = now.Add(-time.Hour).Format(time.RFC3339)
	got, _ = s.Get(t.Context(), id, site, "Q4", "development")
	if got.Metrics != nil {
		t.Fatal("stale report accepted")
	}
	if _, err = s.Get(t.Context(), id, site, "Q4", "other"); err == nil {
		t.Fatal("invalid environment accepted")
	}
}

func TestOutcomeBusinessDatesIncludeTodayAndYearEnd(t *testing.T) {
	for _, tc := range []struct{ now, q, start, end string }{{"2026-12-31T23:30:00Z", "FY", "2027-01-01", "2027-01-01"}, {"2026-12-31T12:00:00Z", "Q4", "2026-10-01", "2026-12-31"}, {"2026-10-28T12:00:00Z", "Q4", "2026-10-01", "2026-10-28"}, {"2026-10-28T12:00:00Z", "Q1", "2026-01-01", "2026-03-31"}} {
		now, _ := time.Parse(time.RFC3339, tc.now)
		p, err := outcomePeriod(tc.q, now)
		if err != nil || p.StartDate != tc.start || p.EndDate != tc.end {
			t.Fatal(p, err)
		}
	}
}
