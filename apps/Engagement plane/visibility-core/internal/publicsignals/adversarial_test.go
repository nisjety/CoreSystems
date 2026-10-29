package publicsignals

import (
	"context"
	"errors"
	"fmt"
	"math"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func measuredReport(site string, now time.Time) Report {
	r := testReport(site, now)
	lcp := float64(2000)
	cls := float64(.1)
	for i := range r.CrUX {
		r.CrUX[i] = CrUX{FormFactor: r.CrUX[i].FormFactor, Status: "connected", Period: &Period{StartDate: "2026-09-25", EndDate: "2026-10-22"}, Metrics: &Metrics{LCPMs: &lcp, CLS: &cls}}
	}
	r.CommonCrawl = CommonCrawl{Status: "connected", Index: "CC-MAIN-2026-40", Captures: []Capture{{URL: site + "innsikt", CapturedAt: "2026-10-01T10:00:00Z", HTTPStatus: 200}}, Limit: 20}
	return r
}
func TestStrictReportValidation(t *testing.T) {
	s, r, _, _ := fixture(t)
	report := measuredReport(r.record.Website.Site, s.now())
	if !ValidReport(report, r.record.Website.Site, s.now()) {
		t.Fatal("validreportrejected")
	}
	cases := map[string]func(*Report){"nan": func(r *Report) { n := math.NaN(); r.CrUX[0].Metrics.CLS = &n }, "negative": func(r *Report) { n := -1.0; r.CrUX[0].Metrics.INPMs = &n }, "allnull": func(r *Report) { r.CrUX[0].Metrics = &Metrics{} }, "nodatafields": func(r *Report) { r.CrUX[0].Status = "unavailable"; r.CrUX[0].Reason = "no_data" }, "period": func(r *Report) { r.CrUX[0].Period.EndDate = "2026-10-20" }, "badcalendar": func(r *Report) { r.CrUX[0].Period.StartDate = "2026-09-31" }, "future": func(r *Report) { r.ObservedAt = stamp(s.now().Add(time.Hour)) }, "oldreceipt": func(r *Report) { r.ObservedAt = stamp(s.now().Add(-25 * time.Hour)) }, "wrongsite": func(r *Report) { r.Site = "https://other.no/" }, "wrongsource": func(r *Report) { r.Source = "raw" }, "factor": func(r *Report) { r.CrUX[0].FormFactor = "TABLET" }, "emptycaptures": func(r *Report) { r.CommonCrawl.Captures = nil }, "badreason": func(r *Report) { r.CommonCrawl.Reason = "secret" }, "futureindex": func(r *Report) { r.CommonCrawl.Index = "CC-MAIN-2099-40" }, "badweek": func(r *Report) { r.CommonCrawl.Index = "CC-MAIN-2026-00" }, "capturefuture": func(r *Report) { r.CommonCrawl.Captures[0].CapturedAt = stamp(s.now().Add(time.Hour)) }, "captureold": func(r *Report) { r.CommonCrawl.Captures[0].CapturedAt = "2007-01-01T00:00:00Z" }, "badstatus": func(r *Report) { r.CommonCrawl.Captures[0].HTTPStatus = 999 }, "query": func(r *Report) { r.CommonCrawl.Captures[0].URL += "?token=x" }, "forcedquery": func(r *Report) { r.CommonCrawl.Captures[0].URL += "?" }, "foreignhost": func(r *Report) { r.CommonCrawl.Captures[0].URL = "https://www.triodelab.no/" }, "authpath": func(r *Report) { r.CommonCrawl.Captures[0].URL = r.Site + "%256cogin" }, "controlpath": func(r *Report) { r.CommonCrawl.Captures[0].URL = r.Site + "x%2500" }, "backslash": func(r *Report) { r.CommonCrawl.Captures[0].URL = r.Site + "x%255cy" }, "overencode": func(r *Report) { r.CommonCrawl.Captures[0].URL = r.Site + "%2525252525256cogin" }, "duplicate": func(r *Report) { r.CommonCrawl.Captures = append(r.CommonCrawl.Captures, r.CommonCrawl.Captures[0]) }, "limit": func(r *Report) { r.CommonCrawl.Limit = 10 }}
	for name, change := range cases {
		t.Run(name, func(t *testing.T) {
			v := clone(report)
			change(&v)
			if ValidReport(v, r.record.Website.Site, s.now()) {
				t.Fatal("badreportaccepted")
			}
		})
	}
	report.CommonCrawl.Captures[0].URL = "http://triodelab.no/innsikt"
	if !ValidReport(report, r.record.Website.Site, s.now()) {
		t.Fatal("historichttprejected")
	}
}

type blocked struct {
	entered, release chan struct{}
	runner           *runnerFake
}

func (b blocked) Collect(ctx context.Context, r websites.Record) (Report, error) {
	close(b.entered)
	select {
	case <-ctx.Done():
		return Report{}, ctx.Err()
	case <-b.release:
		return b.runner.Collect(ctx, r)
	}
}
func TestPauseAndCommitRetriesNeverRepeatProvider(t *testing.T) {
	s, r, runner, _ := fixture(t)
	w := r.record.Website
	b := blocked{make(chan struct{}), make(chan struct{}), runner}
	s.runner = b
	_, _ = s.Queue(w.ID, w.Site, 1)
	e, _ := s.claim()
	done := make(chan struct{})
	go func() { s.execute(context.Background(), e); close(done) }()
	<-b.entered
	_, _ = s.SetEnabled(w.ID, w.Site, 1, false)
	original := s.repo.persistFn
	saves := 0
	s.repo.persistFn = func(v diskState) error {
		saves++
		if saves == 1 {
			return errors.New("disk")
		}
		return original(v)
	}
	close(b.release)
	<-done
	v, _ := s.Get(w.ID, w.Site)
	if v.Status != "complete" || v.Enabled || v.NextSyncAt != "" || runner.calls != 1 || saves != 2 {
		t.Fatal(v, runner.calls, saves)
	}
}
func TestExpiredPatchAndPeriodicFailureAndRecovery(t *testing.T) {
	s, r, runner, path := fixture(t)
	v := complete(t, s, r)
	s.now = func() time.Time { return runner.now.Add(25 * time.Hour) }
	paused, err := s.SetEnabled(v.WebsiteID, v.Site, 1, false)
	if err != nil || paused.Report != nil || paused.ExpiresAt != v.ExpiresAt {
		t.Fatal("PATCHleakedexpired", paused, err)
	}
	_, _ = s.SetEnabled(v.WebsiteID, v.Site, 1, true)
	s.now = func() time.Time { return runner.now.Add(50 * time.Hour) }
	e, ok := s.claim()
	if !ok || e.JobID == v.JobID {
		t.Fatal("periodicnotclaimed")
	}
	runner.err = errors.New("provider")
	s.execute(context.Background(), e)
	got, _ := s.Get(v.WebsiteID, v.Site)
	if got.Status != "failed" || got.Report != nil || got.ExpiresAt != "" || got.Reason != "source_unavailable" {
		t.Fatal(got)
	}
	if _, err = Open(path, "org_public"); err != nil {
		t.Fatal(err)
	}
	s.now = func() time.Time { return runner.now.Add(57 * time.Hour) }
	_, _ = s.claim()
	if err = s.recover(); err != nil {
		t.Fatal(err)
	}
	got, _ = s.Get(v.WebsiteID, v.Site)
	if got.Reason != "interrupted" {
		t.Fatal(got)
	}
}
func TestProtectedRepositoryRollbackAndGuards(t *testing.T) {
	s, r, _, path := fixture(t)
	w := r.record.Website
	for _, input := range []struct {
		id, site string
		rev      int64
	}{{"bad", w.Site, 1}, {w.ID, "https://localhost/", 1}, {w.ID, w.Site, 0}, {w.ID, w.Site, 99}} {
		if _, err := s.Queue(input.id, input.site, input.rev); err == nil {
			t.Fatal("badinputaccepted")
		}
	}
	s.repo.persistFn = func(diskState) error { return errors.New("disk") }
	if _, err := s.Queue(w.ID, w.Site, 1); err == nil || len(s.repo.read().Entries) != 0 {
		t.Fatal("failedwriteswallowed")
	}
	if _, err := Open(path, "other_org"); err == nil {
		t.Fatal("crossorg")
	}
	_ = os.Chmod(path, 0644)
	if _, err := Open(path, "org_public"); err == nil {
		t.Fatal("unsafepermission")
	}
	if _, err := Open("relative", "org_public"); err == nil {
		t.Fatal("relative")
	}
	if _, err := s.SetEnabled(w.ID, w.Site, 0, true); err == nil {
		t.Fatal("badrevision")
	}
	_ = New(nil, nil, nil, nil)
}
func TestBoundedBudgetAndStateValidation(t *testing.T) {
	s, r, _, _ := fixture(t)
	v := complete(t, s, r)
	state := s.repo.read()
	bad := clone(state)
	bad.Entries[0].Status = "unknown"
	if validState(bad) {
		t.Fatal("badstate")
	}
	bad = clone(state)
	bad.Entries[0].QueuedAt = ""
	if validState(bad) {
		t.Fatal("badstamp")
	}
	bad = clone(state)
	bad.Entries[0].Report.CrUX[0].Reason = "rawsecret"
	if validState(bad) {
		t.Fatal("badreport")
	}
	prior := diskState{}
	for i := range 3 {
		e := clone(v)
		e.JobID = fmt.Sprintf("job%d", i)
		e.Report.CommonCrawl.Captures = []Capture{{URL: strings.Repeat("x", 1<<20)}}
		prior.Entries = append(prior.Entries, e)
	}
	next := clone(prior)
	next.Entries[2].Status = "running"
	next.Entries[2].Report = nil
	trimmed, err := trimBudget(next, prior)
	if err != nil || len(trimmed.Entries) >= 3 {
		t.Fatal(err)
	}
	for i := range next.Entries {
		next.Entries[i].Status = "running"
	}
	if _, err = trimBudget(next, diskState{}); !errors.Is(err, ErrCapacity) {
		t.Fatal(err)
	}
}
func TestWorkerLifecycleJoinedAndRestartNoImplicitSites(t *testing.T) {
	s, _, runner, _ := fixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { s.Run(ctx); close(done) }()
	time.Sleep(time.Millisecond)
	s.Run(ctx)
	cancel()
	<-done
	if runner.calls != 0 {
		t.Fatal("legacyautoenrolled")
	}
}

type slowRegistry struct {
	base     *registryFake
	calls    int
	onSecond func()
}

func (r *slowRegistry) OrganizationID() string { return r.base.OrganizationID() }
func (r *slowRegistry) Resolve(id, site string) (websites.Record, error) {
	r.calls++
	if r.calls == 2 {
		r.onSecond()
	}
	return r.base.Resolve(id, site)
}
func TestGetRechecksExpiryAndJobAfterSlowBinding(t *testing.T) {
	s, r, runner, _ := fixture(t)
	v := complete(t, s, r)
	clock := runner.now
	s.now = func() time.Time { return clock }
	s.registry = &slowRegistry{base: r, onSecond: func() { clock = clock.Add(25 * time.Hour) }}
	got, err := s.Get(v.WebsiteID, v.Site)
	if err != nil || got.Report != nil {
		t.Fatal("lateTTLcheckmissing", got, err)
	}
	clock = runner.now
	s.registry = &slowRegistry{base: r, onSecond: func() {
		if err := s.repo.update(func(v diskState) (diskState, error) { v.Entries[0].JobID = newID(); return v, nil }); err != nil {
			t.Fatal(err)
		}
	}}
	if _, err = s.Get(v.WebsiteID, v.Site); !errors.Is(err, ErrConflict) {
		t.Fatal("oldjobreturnedafterrefresh", err)
	}
}

type advancingRunner struct {
	now   func() time.Time
	calls int
}

func (r *advancingRunner) Collect(_ context.Context, w websites.Record) (Report, error) {
	r.calls++
	return testReport(w.Website.Site, r.now()), nil
}
func TestAdvancingClockCommitsOneTerminalReceipt(t *testing.T) {
	s, r, _, _ := fixture(t)
	base := s.now()
	ticks := time.Duration(0)
	s.now = func() time.Time { ticks++; return base.Add(ticks * time.Millisecond) }
	runner := &advancingRunner{now: s.now}
	s.runner = runner
	w := r.record.Website
	_, err := s.Queue(w.ID, w.Site, 1)
	if err != nil {
		t.Fatal(err)
	}
	e, ok := s.claim()
	if !ok {
		t.Fatal("notclaimed")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 150*time.Millisecond)
	defer cancel()
	s.execute(ctx, e)
	got, err := s.Get(w.ID, w.Site)
	if err != nil || got.Status != "complete" || got.Report == nil || runner.calls != 1 {
		t.Fatal("advancingclockpreventedterminalcommit", got, err, runner.calls)
	}
	if parse(got.ExpiresAt) != parse(got.CompletedAt).Add(successTTL) || parse(got.NextSyncAt) != parse(got.CompletedAt).Add(successTTL) {
		t.Fatal("terminaltimesdidnotshareoneclockreading", got)
	}
}

func TestPermanentPayloadErrorsBecomeFailureWithoutRetryingProvider(t *testing.T) {
	for _, permanent := range []error{ErrInvalid, ErrCapacity} {
		t.Run(permanent.Error(), func(t *testing.T) {
			s, r, runner, _ := fixture(t)
			w := r.record.Website
			_, _ = s.Queue(w.ID, w.Site, 1)
			e, _ := s.claim()
			original := s.repo.persistFn
			saves := 0
			s.repo.persistFn = func(v diskState) error {
				saves++
				if v.Entries[0].Report != nil {
					return permanent
				}
				return original(v)
			}
			s.execute(context.Background(), e)
			got, err := s.Get(w.ID, w.Site)
			if err != nil || got.Status != "failed" || got.Reason != "source_unavailable" || got.Report != nil || runner.calls != 1 || saves != 2 {
				t.Fatal("permanenterrorlooped", got, err, runner.calls, saves)
			}
			calls := 0
			if s.retry(context.Background(), func() error { calls++; return permanent }) || calls != 1 {
				t.Fatal("permanentfailurewasretried", calls)
			}
		})
	}
}
