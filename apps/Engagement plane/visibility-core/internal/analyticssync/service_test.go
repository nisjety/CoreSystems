package analyticssync

import (
	"context"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type fakeRegistry struct {
	mu     sync.Mutex
	record websites.Record
}

func (r *fakeRegistry) Resolve(id, site string) (websites.Record, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if (id != "" && id != r.record.Website.ID) || site != r.record.Website.Site {
		return websites.Record{}, websites.ErrNotFound
	}
	return r.record, nil
}
func (*fakeRegistry) OrganizationID() string { return "org_test" }

type fakeAuthority struct {
	mu     sync.Mutex
	access Access
	calls  int
}

func (a *fakeAuthority) Check(context.Context, websites.Record) Access {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.calls++
	return a.access
}

type fakeRunner struct {
	mu    sync.Mutex
	calls int
	now   time.Time
}

func (r *fakeRunner) Collect(_ context.Context, w websites.Record, q string) (Bundle, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.calls++
	b := MissingBundle(w.Website.Site, q, r.now, "not_connected")
	p, _ := pulse.QuarterPeriod(q, r.now)
	b.Pulse = pulse.Projection{Site: w.Website.Site, Source: "google_search_console", Status: "connected", Period: p, Totals: &pulse.Totals{Clicks: 2, Impressions: 3}, Days: []pulse.Day{{Date: p.StartDate, Clicks: 2, Impressions: 3}}, PropertyScope: "domain", UpdatedAt: r.now.Format(time.RFC3339)}
	b.Search = pulse.SearchProjection{Site: w.Website.Site, Source: "google_search_console", Status: "connected", Period: p, Queries: []pulse.SearchRow{}, Pages: []pulse.SearchRow{}, Opportunities: []pulse.Opportunity{}, RowLimit: 100, PropertyScope: "domain", UpdatedAt: r.now.Format(time.RFC3339)}
	return b, nil
}
func fixture(t *testing.T) (*Service, *fakeRegistry, *fakeAuthority, *fakeRunner, string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "analytics.json")
	repo, err := Open(path, "org_test")
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)
	reg := &fakeRegistry{record: websites.Record{Website: websites.Website{ID: "web_00000000000000000000000000000001", Site: "https://triodelab.no/", Revision: 1}, GSCConnectionID: "conn_google"}}
	auth := &fakeAuthority{access: Access{SearchConsole: true}}
	runner := &fakeRunner{now: now}
	svc := New(repo, reg, runner, auth, func() time.Time { return now })
	return svc, reg, auth, runner, path
}
func waitTerminal(t *testing.T, s *Service) State {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		v, e := s.Get("web_00000000000000000000000000000001", "https://triodelab.no/", "Q4")
		if e != nil {
			t.Fatal(e)
		}
		if v.Status == "complete" || v.Status == "partial" || v.Status == "failed" {
			return v
		}
		time.Sleep(5 * time.Millisecond)
	}
	t.Fatal("job did not settle")
	return State{}
}

func TestPureReadsAndExplicitEnrollment(t *testing.T) {
	s, _, _, runner, _ := fixture(t)
	v, e := s.Get("web_00000000000000000000000000000001", "https://triodelab.no/", "Q4")
	if e != nil || v.Status != "idle" || v.Enabled {
		t.Fatalf("idle=%+v %v", v, e)
	}
	b, e := s.Read(context.Background(), v.WebsiteID, v.Site, "Q4")
	if e != nil || b.Pulse.Totals != nil || runner.calls != 0 {
		t.Fatal("read fetched or fabricated metrics")
	}
}
func TestDurableSnapshotFreshAuthorityAndRevocation(t *testing.T) {
	s, r, a, runner, path := fixture(t)
	w := r.record.Website
	v, e := s.Queue(w.ID, w.Site, w.Revision, "Q4")
	if e != nil || v.Status != "queued" || !v.Enabled {
		t.Fatal(v, e)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	v = waitTerminal(t, s)
	cancel()
	<-done
	if v.Status != "partial" || len(v.Steps) != 3 {
		t.Fatal(v)
	}
	b, e := s.Read(context.Background(), w.ID, w.Site, "Q4")
	if e != nil || b.Pulse.Totals == nil || b.Pulse.Totals.Clicks != 2 {
		t.Fatal(b, e)
	}
	a.mu.Lock()
	a.access = Access{}
	a.mu.Unlock()
	b, e = s.Read(context.Background(), w.ID, w.Site, "Q4")
	if e != nil || b.Pulse.Totals != nil {
		t.Fatal("revoked metrics escaped")
	}
	repo, e := Open(path, "org_test")
	if e != nil {
		t.Fatal(e)
	}
	s2 := New(repo, r, runner, a, s.now)
	got, e := s2.Get(w.ID, w.Site, "Q4")
	if e != nil || got.JobID != v.JobID || runner.calls != 1 {
		t.Fatal("restart lost or recollected", e)
	}
}
func TestBindingChangeAndPause(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	w := r.record.Website
	if _, e := s.Queue(w.ID, w.Site, w.Revision, "Q4"); e != nil {
		t.Fatal(e)
	}
	v, e := s.SetEnabled(w.ID, w.Site, w.Revision, "Q4", false)
	if e != nil || v.Enabled {
		t.Fatal(v, e)
	}
	r.mu.Lock()
	r.record.GSCConnectionID = "other"
	r.mu.Unlock()
	v, e = s.Get(w.ID, w.Site, "Q4")
	if e != nil || v.Status != "idle" {
		t.Fatal("changed binding served", v, e)
	}
	if _, e = s.Queue(w.ID, w.Site, 2, "Q4"); e == nil {
		t.Fatal("wrong revision accepted")
	}
}
func TestProtectedRepository(t *testing.T) {
	_, _, _, _, path := fixture(t)
	if _, e := Open(path, "wrong_org"); e == nil {
		t.Fatal("cross org open")
	}
	if e := os.Chmod(path, 0644); e != nil {
		t.Fatal(e)
	}
	if _, e := Open(path, "org_test"); e == nil {
		t.Fatal("public permissions accepted")
	}
}
