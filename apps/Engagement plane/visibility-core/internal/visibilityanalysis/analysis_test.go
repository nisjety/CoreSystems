package visibilityanalysis

import (
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"testing"
	"time"
)

const site = "https://triodelab.no/"

func TestCombinedRowsWeightedHostAliasAndUnknownMetrics(t *testing.T) {
	w := websites.Website{ID: "web_11111111111111111111111111111111", Site: site, Revision: 1}
	search := pulse.SearchProjection{Status: "connected", PagesAtLimit: true, Pages: []pulse.SearchRow{{Key: site, Clicks: 1, Impressions: 10, Position: 5}, {Key: "https://www.triodelab.no/", Clicks: 2, Impressions: 20, Position: 8}}}
	inventory := discovery.Snapshot{Status: "complete", Pages: []discovery.Page{{URL: site + "about"}}}
	v := Build(w, search, siteaudit.Snapshot{Status: "idle"}, inventory, time.Now())
	if len(v.Pages) != 2 || v.Pages[0].Clicks == nil || *v.Pages[0].Clicks != 3 || *v.Pages[0].Impressions != 30 || *v.Pages[0].Position != 7 {
		t.Fatal(v.Pages)
	}
	if v.Pages[1].Clicks != nil || v.Pages[1].HTTPStatus != nil || v.Pages[1].Outbound != nil || v.Pages[1].AuditStatus != "not_checked" {
		t.Fatal("missing data became zero", v.Pages[1])
	}
	if !v.Coverage.Partial || !v.Coverage.SearchTruncated {
		t.Fatal("lost coverage limits")
	}
	for _, raw := range []string{"https://other.no/", "https://api.triodelab.no/", "http://triodelab.no/", site + "?token=secret"} {
		if Key(site, raw) != "" {
			t.Fatal("unsafe join", raw)
		}
	}
}
func TestObservedLinkGraphBrokenUnknownDepthAndGroups(t *testing.T) {
	w := websites.Website{Site: site, Revision: 1}
	audit := siteaudit.Snapshot{Status: "complete", Pages: []siteaudit.Page{
		{URL: site, FinalURL: "https://www.triodelab.no/", Status: "checked", HTTPStatus: 200, Links: []string{site + "gone", site + "gone", site + "unvisited"}},
		{URL: site + "gone", FinalURL: site + "gone", Status: "checked", HTTPStatus: 404, Checks: []siteaudit.Check{{RuleID: "http_error", Status: "failed"}}},
		{URL: site + "failed", Status: "failed"},
	}}
	search := pulse.SearchProjection{Status: "connected", Queries: []pulse.SearchRow{{Key: "near", Impressions: 20, Position: 15}, {Key: "low", Impressions: 100, Position: 5, CTR: .01}, {Key: "insufficient", Impressions: 19, Position: 15}}}
	v := Build(w, search, audit, discovery.Snapshot{}, time.Now())
	if len(v.Edges) != 2 || v.Coverage.Checked != 2 || v.Coverage.Failed != 1 {
		t.Fatal(v)
	}
	broken, unknown := 0, 0
	for _, e := range v.Edges {
		if e.Status == "broken" {
			broken++
		}
		if e.Status == "not_checked" {
			unknown++
		}
	}
	if broken != 1 || unknown != 1 {
		t.Fatal(v.Edges)
	}
	for _, p := range v.Pages {
		if p.URL == site+"gone" && (p.Inbound != 1 || p.Depth == nil || *p.Depth != 1) {
			t.Fatal(p)
		}
	}
	if len(v.Groups[0].Items) != 1 || len(v.Groups[1].Items) != 1 || len(v.Groups[2].Items) != 1 {
		t.Fatal(v.Groups)
	}
}
func TestUnavailableSearchNeverExposesRowsAndCountOverflowStaysUnknown(t *testing.T) {
	w := websites.Website{Site: site, Revision: 1}
	search := pulse.SearchProjection{Status: "unavailable", Pages: []pulse.SearchRow{{Key: site, Clicks: 8, Impressions: 9}}, Queries: []pulse.SearchRow{{Key: "private", Impressions: 100, Position: 15}}}
	v := Build(w, search, siteaudit.Snapshot{}, discovery.Snapshot{}, time.Now())
	if len(v.Pages) != 0 || len(v.Groups[0].Items) != 0 {
		t.Fatal("unavailable private source exposed")
	}
	search.Status = "connected"
	search.Pages = []pulse.SearchRow{{Key: site, Clicks: 1, Impressions: 9007199254740991, Position: 10}, {Key: "https://www.triodelab.no/", Clicks: 1, Impressions: 1, Position: 10}}
	v = Build(w, search, siteaudit.Snapshot{}, discovery.Snapshot{}, time.Now())
	if v.Pages[0].Impressions != nil || !v.Coverage.SearchTruncated {
		t.Fatal("unsafe integer aggregate", v.Pages)
	}
}

func TestHostAliasesUseLatestAuditObservationAndUniqueCoverage(t *testing.T) {
	w := websites.Website{Site: site, Revision: 1}
	audit := siteaudit.Snapshot{Status: "complete", CompletedAt: "2026-10-28T12:00:00Z", Pages: []siteaudit.Page{
		{URL: site, FinalURL: site, Status: "checked", HTTPStatus: 200, Title: "Newest", ObservedAt: "2026-10-28T11:00:00Z"},
		{URL: "https://www.triodelab.no/", FinalURL: "https://www.triodelab.no/", Status: "checked", HTTPStatus: 200, Title: "Older", ObservedAt: "2026-10-28T10:00:00Z"},
	}}
	v := Build(w, pulse.SearchProjection{}, audit, discovery.Snapshot{}, time.Now())
	if len(v.Pages) != 1 || v.Coverage.Checked != 1 || v.Pages[0].Title != "Newest" {
		t.Fatal(v)
	}
	audit.Pages = append(audit.Pages, siteaudit.Page{URL: "https://www.triodelab.no/", Status: "failed"})
	v = Build(w, pulse.SearchProjection{}, audit, discovery.Snapshot{}, time.Now())
	if v.Coverage.Checked != 0 || v.Coverage.Failed != 1 || v.Pages[0].HTTPStatus != nil || v.Pages[0].AuditStatus != "failed" {
		t.Fatal("failed latest observation reused old success", v)
	}
}
