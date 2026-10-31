package publicsignals

import (
	"context"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func TestProviderTimeoutPreservesReportAndLegacyReasons(t *testing.T) {
	s, registry, _, _ := fixture(t)
	site := registry.record.Website.Site
	for _, reason := range []string{"not_configured", "no_data", "quota_limited", "source_unavailable", "timeout"} {
		t.Run(reason, func(t *testing.T) {
			report := testReport(site, s.now())
			report.CrUX[0].Reason = reason
			report.CommonCrawl.Reason = reason
			report.CommonCrawl.Index = "CC-MAIN-2026-43"
			if !ValidReport(report, site, s.now()) {
				t.Fatal("bounded provider reason was rejected")
			}
		})
	}
	for _, reason := range []string{"provider says token=secret", "504", "context_changed", ""} {
		report := testReport(site, s.now())
		report.CommonCrawl.Reason = reason
		if ValidReport(report, site, s.now()) {
			t.Fatal("untrusted provider reason accepted")
		}
	}
}

type timeoutReportRunner struct{ report Report }

func (r timeoutReportRunner) Collect(context.Context, websites.Record) (Report, error) {
	return r.report, nil
}

func TestProviderTimeoutReceiptSurvivesRestartWithoutRecollection(t *testing.T) {
	s, registry, original, path := fixture(t)
	report := testReport(registry.record.Website.Site, s.now())
	report.CommonCrawl.Reason = "timeout"
	report.CommonCrawl.Index = "CC-MAIN-2026-43"
	s.runner = timeoutReportRunner{report}
	before := complete(t, s, registry)
	if before.Status != "complete" || before.Report == nil || before.Report.CommonCrawl.Reason != "timeout" {
		t.Fatal("provider timeout discarded the collection receipt")
	}
	repo, err := Open(path, registry.OrganizationID())
	if err != nil {
		t.Fatal(err)
	}
	restored := New(repo, registry, original, s.now)
	after, err := restored.Get(before.WebsiteID, before.Site)
	if err != nil || after.JobID != before.JobID || after.Report == nil || after.Report.CommonCrawl.Reason != "timeout" || original.calls != 0 {
		t.Fatal("saved timeout did not survive a pure read after restart")
	}
}
