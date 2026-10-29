package publicsignals

import "testing"

func TestCommonCrawlReportingHostScopesEveryCapture(t *testing.T) {
	s, r, _, _ := fixture(t)
	site := r.record.Website.Site
	report := measuredReport(site, s.now())
	report.CommonCrawl.ReportingHost = "www.triodelab.no"
	report.CommonCrawl.Captures[0].URL = "https://www.triodelab.no/innsikt"
	if !ValidReport(report, site, s.now()) {
		t.Fatal("valid exact alias archive rejected")
	}
	for _, host := range []string{"foreign.no", "blog.triodelab.no", "www.triodelab.no:443", "www.triodelab.no/"} {
		bad := clone(report)
		bad.CommonCrawl.ReportingHost = host
		if ValidReport(bad, site, s.now()) {
			t.Fatal("unsafe archive origin", host)
		}
	}
	report.CommonCrawl.Captures[0].URL = "https://triodelab.no/innsikt"
	if ValidReport(report, site, s.now()) {
		t.Fatal("capture differs from reporting host")
	}
}
