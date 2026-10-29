package publicsignals

import (
	"context"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type originRunner struct{ report Report }

func (r originRunner) Collect(context.Context, websites.Record) (Report, error) { return r.report, nil }

func TestCrUXDiscoveredOriginIsScopedAndDurable(t *testing.T) {
	s, registry, _, path := fixture(t)
	site := registry.record.Website.Site
	report := testReport(site, s.now())
	lcp, cls := 1892.0, 0.0
	for i := range report.CrUX {
		report.CrUX[i] = CrUX{FormFactor: report.CrUX[i].FormFactor, Status: "connected", ReportingOrigin: "https://www.triodelab.no", Period: &Period{StartDate: "2026-09-25", EndDate: "2026-10-22"}, Metrics: &Metrics{LCPMs: &lcp, CLS: &cls}}
	}
	if !ValidReport(report, site, s.now()) {
		t.Fatal("valid exact-www discovery rejected")
	}
	for _, origin := range []string{"https://other.no", "https://blog.triodelab.no", "https://www.triodelab.no:443", "http://www.triodelab.no", "https://www.triodelab.no/", "https://www.triodelab.no?token=x"} {
		invalid := clone(report)
		invalid.CrUX[0].ReportingOrigin = origin
		if ValidReport(invalid, site, s.now()) {
			t.Fatalf("invalid reporting origin accepted: %s", origin)
		}
	}
	legacy := clone(report)
	legacy.CrUX[0].ReportingOrigin = ""
	if !ValidReport(legacy, site, s.now()) {
		t.Fatal("historical exact-origin snapshot rejected")
	}
	unavailable := testReport(site, s.now())
	unavailable.CrUX[0].ReportingOrigin = "https://www.triodelab.no"
	if ValidReport(unavailable, site, s.now()) {
		t.Fatal("unavailable measurement claimed a successful origin")
	}
	if !cruxOriginAllowed("https://www.triodelab.no/", "https://triodelab.no") {
		t.Fatal("non-www counterpart rejected")
	}
	s.runner = originRunner{report}
	got := complete(t, s, registry)
	if got.Status != "complete" || got.Report == nil {
		t.Fatalf("report not saved: %+v", got)
	}
	repo, err := Open(path, "org_public")
	if err != nil {
		t.Fatal(err)
	}
	restored := New(repo, registry, s.runner, s.now)
	saved, err := restored.Get(got.WebsiteID, site)
	if err != nil || saved.Report == nil || saved.Report.CrUX[0].ReportingOrigin != "https://www.triodelab.no" || saved.Site != site || registry.record.Website.Site != site {
		t.Fatal("discovered origin was lost or selected website changed")
	}
}
