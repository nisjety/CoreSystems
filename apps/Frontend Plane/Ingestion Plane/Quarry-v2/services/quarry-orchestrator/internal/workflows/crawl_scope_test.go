package workflows

import (
	"fmt"
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// Unit tests of the crawl-scope helpers (WS14.S3). The workflow-level tests
// (TestCrawlJobWF_*, replay) run in the final test phase with the testsuite.

func TestNewCrawlScopeRun_LegacyAndNilScopeAreNotScoped(t *testing.T) {
	if got := newCrawlScopeRun(CrawlJobInput{Seeds: []string{"https://a.test/"}}); got != nil {
		t.Fatalf("a crawl with no scope must run unscoped, got %+v", got)
	}
	legacy := CrawlJobInput{Seeds: []string{"https://a.test/"}, Scope: &activities.CrawlScope{Legacy: true}}
	if got := newCrawlScopeRun(legacy); got != nil {
		t.Fatalf("a legacy scope must run unscoped, got %+v", got)
	}
}

func TestNewCrawlScopeRun_UnknownModeIsObserveAndUnknownRobotsIsRespect(t *testing.T) {
	in := CrawlJobInput{
		Seeds:     []string{"https://a.test/"},
		MaxDepth:  2,
		Scope:     &activities.CrawlScope{CrawlEntireDomain: true},
		ScopeMode: "something-else",
		Robots:    "ignore-everything",
	}
	s := newCrawlScopeRun(in)
	if s == nil {
		t.Fatal("a non-legacy scope must be scoped")
	}
	if s.base.ScopeMode != activities.ScopeModeObserve {
		t.Errorf("scope mode = %q, want observe", s.base.ScopeMode)
	}
	if s.base.Robots != activities.RobotsRespect {
		t.Errorf("robots = %q, want respect", s.base.Robots)
	}
}

func TestCrawlScopeRun_ContextForCarriesSortedSeedHostsAndDepth(t *testing.T) {
	in := CrawlJobInput{
		Seeds:     []string{"https://Zeta.test/a", "https://alpha.test/", ""},
		MaxDepth:  3,
		Scope:     &activities.CrawlScope{CrawlEntireDomain: true},
		ScopeMode: activities.ScopeModeEnforce,
	}
	s := newCrawlScopeRun(in)
	c := s.contextFor(2)
	if c.V != activities.CrawlContractVersion || c.Kind != activities.CrawlKindCrawl {
		t.Fatalf("context header = v%d %q", c.V, c.Kind)
	}
	if c.Depth != 2 || c.MaxDepth != 3 {
		t.Errorf("depth = %d max %d", c.Depth, c.MaxDepth)
	}
	want := []string{"alpha.test", "zeta.test"}
	if fmt.Sprint(c.SeedHosts) != fmt.Sprint(want) {
		t.Errorf("seed hosts = %v, want %v", c.SeedHosts, want)
	}
	if c.SeedURL != "https://Zeta.test/a" {
		t.Errorf("seed url = %q", c.SeedURL)
	}
	if c.ScopeMode != activities.ScopeModeEnforce {
		t.Errorf("scope mode = %q", c.ScopeMode)
	}
}

func TestCrawlScopeRun_AbsorbAddsHostsAndDenialCounts(t *testing.T) {
	s := newCrawlScopeRun(CrawlJobInput{
		Seeds: []string{"https://a.test/"},
		Scope: &activities.CrawlScope{CrawlEntireDomain: true},
	})
	s.absorb(&activities.CrawlReport{
		ScopeApplied: true,
		LinksDenied:  4,
		HostsAdded:   []string{"cdn.a.test"},
		Denials: activities.DenialReport{
			Counts: map[string]uint32{"external_host_disabled": 4},
		},
	})
	if s.linksDenied != 4 {
		t.Errorf("links denied = %d, want 4", s.linksDenied)
	}
	if _, ok := s.seedHosts["cdn.a.test"]; !ok {
		t.Error("a host the edge added must join the seed hosts of later pages")
	}
	if got := s.contextFor(1).SeedHosts; fmt.Sprint(got) != fmt.Sprint([]string{"a.test", "cdn.a.test"}) {
		t.Errorf("seed hosts after absorb = %v", got)
	}
	term := s.terminal(1, 0)
	if term["links_denied"] != uint32(4) || term["pages_blocked"] != uint32(1) || term["pages_blocked_robots"] != uint32(0) {
		t.Errorf("terminal = %v", term)
	}
}

func TestCrawlScopeRun_TerminalCountsRobotsRefusalsApartFromOtherBlocks(t *testing.T) {
	s := newCrawlScopeRun(CrawlJobInput{
		Seeds: []string{"https://a.test/"},
		Scope: &activities.CrawlScope{CrawlEntireDomain: true},
	})
	term := s.terminal(3, 2)
	if term["pages_blocked"] != uint32(3) || term["pages_blocked_robots"] != uint32(2) {
		t.Errorf("terminal = %v, want pages_blocked 3 and pages_blocked_robots 2", term)
	}
}

func TestRobotsRefused_ReadsOnlyTheRobotsDenials(t *testing.T) {
	denial := func(status int, code string) error {
		body := []byte(`{"error":{"code":"SECURITY_BLOCKED","message":"refused","details":{"denial":{"code":"` + code + `","message":"m","remedy":"r"}}}}`)
		return errs.FromHTTPResponse("run_page", status, body)
	}
	if !robotsRefused(denial(403, "robots_disallowed")) {
		t.Error("robots_disallowed is a robots refusal")
	}
	if !robotsRefused(denial(403, "robots_unreachable")) {
		t.Error("robots_unreachable is a robots refusal")
	}
	if robotsRefused(denial(403, "external_host_disabled")) {
		t.Error("a scope refusal is not a robots refusal")
	}
	if robotsRefused(errs.FromHTTPResponse("run_page", 500, []byte(`{}`))) {
		t.Error("a server error is not a robots refusal")
	}
	if robotsRefused(nil) {
		t.Error("no error is not a robots refusal")
	}
	// The workflow sees the activity error after the Temporal round trip.
	if !robotsRefused(denial(403, "robots_disallowed").(*errs.Error).Temporal()) {
		t.Error("the robots denial must survive the Temporal conversion")
	}
}

func TestAppliedBy_RequiresAReportThatSaysScopeApplied(t *testing.T) {
	if appliedBy(nil) {
		t.Error("no report is not proof the edge applied the scope")
	}
	if appliedBy(&activities.CrawlReport{ScopeApplied: false}) {
		t.Error("a report with scope_applied false is not proof")
	}
	if !appliedBy(&activities.CrawlReport{ScopeApplied: true}) {
		t.Error("a report with scope_applied true is proof")
	}
}

func TestDenialAgg_CapsSamplesPerCodeAndSeenURLs(t *testing.T) {
	d := newDenialAgg(activities.ScopeModeObserve)
	var samples []activities.DenialSample
	for i := 0; i < 50; i++ {
		samples = append(samples, activities.DenialSample{
			URL:  fmt.Sprintf("https://a.test/%d", i),
			Code: "external_host_disabled",
		})
	}
	d.absorb(&activities.CrawlReport{Denials: activities.DenialReport{
		Counts:  map[string]uint32{"external_host_disabled": 50},
		Samples: samples,
	}})
	if got := len(d.samples["external_host_disabled"]); got != denialSamplesPerCode {
		t.Errorf("samples kept = %d, want %d", got, denialSamplesPerCode)
	}
	if d.counts["external_host_disabled"] != 50 {
		t.Errorf("counts must keep the full number, got %d", d.counts["external_host_disabled"])
	}
	// The same URL is not sampled twice.
	d.absorb(&activities.CrawlReport{Denials: activities.DenialReport{Samples: samples[:1]}})
	if got := len(d.samples["external_host_disabled"]); got != denialSamplesPerCode {
		t.Errorf("a repeated URL must not add a sample, kept %d", got)
	}
}

func TestDenialAgg_FoldsCodesPastTheBound(t *testing.T) {
	d := newDenialAgg(activities.ScopeModeEnforce)
	counts := map[string]uint32{}
	for i := 0; i < denialMaxCodes+3; i++ {
		counts[fmt.Sprintf("code_%02d", i)] = 1
	}
	d.absorb(&activities.CrawlReport{Denials: activities.DenialReport{Counts: counts}})
	if len(d.counts) != denialMaxCodes+1 {
		t.Errorf("distinct buckets = %d, want %d (the bound plus other)", len(d.counts), denialMaxCodes+1)
	}
	if d.counts[denialOtherCode] != 3 {
		t.Errorf("other bucket = %d, want 3", d.counts[denialOtherCode])
	}
}

func TestScopeFromParams_AbsentIsLegacyAndMalformedIsRefused(t *testing.T) {
	got, err := scopeFromParams(map[string]any{})
	if err != nil || got == nil || !got.Legacy {
		t.Fatalf("absent scope must be legacy, got %+v err %v", got, err)
	}
	scope, err := scopeFromParams(map[string]any{
		"scope": map[string]any{"allow_subdomains": true, "crawl_entire_domain": true},
	})
	if err != nil || !scope.AllowSubdomains || scope.Legacy {
		t.Fatalf("a valid scope must decode, got %+v err %v", scope, err)
	}
	if _, err := scopeFromParams(map[string]any{"scope": "not-an-object"}); err == nil {
		t.Error("a scope that is not an object must be refused, not guessed at")
	}
}
