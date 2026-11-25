package workflows

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// EP-9 O-23: the org's web domain rules the edge stamps on a crawl's scope reach every page run untouched, so the edge
// can drop each followed link they refuse. A crawl with none marshals exactly as it did before the field existed.

func TestScopeFromParams_CarriesTheOrgsDomainRules(t *testing.T) {
	scope, err := scopeFromParams(map[string]any{
		"scope": map[string]any{
			"crawl_entire_domain": true,
			"domain_rules": map[string]any{
				"mode":  "allowlist",
				"deny":  []any{"bad.example"},
				"allow": []any{"good.example"},
			},
		},
	})
	if err != nil {
		t.Fatalf("a scope with domain rules must decode: %v", err)
	}
	if scope.DomainRules == nil || scope.DomainRules.Mode != "allowlist" {
		t.Fatalf("domain rules lost: %+v", scope.DomainRules)
	}
	if len(scope.DomainRules.Deny) != 1 || scope.DomainRules.Deny[0] != "bad.example" {
		t.Errorf("deny = %v", scope.DomainRules.Deny)
	}
	if len(scope.DomainRules.Allow) != 1 || scope.DomainRules.Allow[0] != "good.example" {
		t.Errorf("allow = %v", scope.DomainRules.Allow)
	}
}

func TestCrawlScopeRun_ContextForCarriesTheDomainRulesToEveryPage(t *testing.T) {
	rules := &activities.DomainRules{Mode: "open", Deny: []string{"bad.example"}, Allow: []string{}}
	in := CrawlJobInput{
		Seeds:     []string{"https://alpha.test/"},
		Scope:     &activities.CrawlScope{CrawlEntireDomain: true, DomainRules: rules},
		ScopeMode: activities.ScopeModeEnforce,
	}
	for _, depth := range []uint32{0, 1, 4} {
		c := newCrawlScopeRun(in).contextFor(depth)
		encoded, err := json.Marshal(c)
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if !strings.Contains(string(encoded), `"domain_rules":{"mode":"open","deny":["bad.example"],"allow":[]}`) {
			t.Errorf("depth %d: the page's crawl context lost the rules: %s", depth, encoded)
		}
	}
}

func TestCrawlScopeRun_ContextForWithoutRulesIsWhatItWas(t *testing.T) {
	in := CrawlJobInput{
		Seeds:     []string{"https://alpha.test/"},
		Scope:     &activities.CrawlScope{CrawlEntireDomain: true},
		ScopeMode: activities.ScopeModeEnforce,
	}
	encoded, err := json.Marshal(newCrawlScopeRun(in).contextFor(0))
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if strings.Contains(string(encoded), "domain_rules") {
		t.Errorf("a crawl with no rules must marshal as before: %s", encoded)
	}
}
