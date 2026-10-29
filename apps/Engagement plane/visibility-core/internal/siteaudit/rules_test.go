package siteaudit

import (
	"strings"
	"testing"
)

func testPage(raw string, status int, title string) Page {
	return Page{URL: raw, FinalURL: raw, Status: "checked", HTTPStatus: status, Title: title, Description: "description", Canonical: raw, ObservedAt: "2026-10-22T10:00:00Z", RunID: "run_01K7K6MYJ0B6E321J1K5S8M2J7", Fingerprint: "blake3:" + strings.Repeat("a", 64), Links: []string{}, Checks: []Check{}}
}
func TestRulesOnlyClaimObservedEvidence(t *testing.T) {
	a := testPage("https://triodelab.no/", 200, "")
	a.Description = ""
	a.Links = []string{"https://triodelab.no/missing", "https://triodelab.no/unmeasured"}
	a.Canonical = "https://triodelab.no/canonical"
	b := testPage("https://triodelab.no/missing", 404, "")
	c := testPage("https://triodelab.no/a", 200, "Same")
	d := testPage("https://triodelab.no/b", 200, "Same")
	e := Page{URL: "https://triodelab.no/denied", Status: "failed", Reason: "policy_blocked", Links: []string{}, Checks: []Check{}}
	pages, findings := evaluate([]Page{a, b, c, d, e})
	counts := map[string]int{}
	for _, f := range findings {
		counts[f.RuleID]++
		if !findingPattern.MatchString(f.ID) || f.RunID == "" || f.Fingerprint == "" {
			t.Fatal("missing provenance")
		}
	}
	for rule, want := range map[string]int{"missing_title_signal": 1, "missing_description_signal": 1, "http_error": 1, "duplicate_title_signal": 2, "canonical_target_review": 1, "broken_checked_link": 1} {
		if counts[rule] != want {
			t.Errorf("%s=%d want%d", rule, counts[rule], want)
		}
	}
	if len(pages[4].Checks) != 0 {
		t.Fatal("failed capture invented checks")
	}
	for _, f := range findings {
		for _, u := range f.RelatedURLs {
			if strings.Contains(u, "unmeasured") {
				t.Fatal("unmeasured link claimed broken")
			}
		}
	}
}
func TestRulesDoNotInferMissingHTMLTagsOrGlobalUniqueness(t *testing.T) {
	a := testPage("https://triodelab.no/", 200, "OG title signal")
	pages, findings := evaluate([]Page{a})
	if len(findings) != 0 {
		t.Fatalf("invented findings %+v", findings)
	}
	for _, check := range pages[0].Checks {
		if check.RuleID == "duplicate_title_signal" && check.Status != "inconclusive" {
			t.Fatal("single page cannot establish uniqueness")
		}
	}
	b := testPage("https://triodelab.no/binary", 200, "")
	b.Status = "failed"
	b.Reason = "unsupported_content"
	_, findings = evaluate([]Page{b})
	if len(findings) != 0 {
		t.Fatal("binary described as missing metadata")
	}
}

func TestDuplicateTitlesCountDistinctEffectivePagesNotRedirectAliases(t *testing.T) {
	a := testPage("https://triodelab.no/", 200, "Shared")
	b := testPage("https://www.triodelab.no/", 200, "Shared")
	a.FinalURL = b.URL
	a.Canonical = b.URL
	pages, findings := evaluate([]Page{a, b})
	for _, f := range findings {
		if f.RuleID == "duplicate_title_signal" {
			t.Fatal("one effective page counted as duplicate", f)
		}
	}
	for _, p := range pages {
		for _, c := range p.Checks {
			if c.RuleID == "duplicate_title_signal" && c.Status != "inconclusive" {
				t.Fatal("one effective page establishes uniqueness")
			}
		}
	}
	c := testPage("https://triodelab.no/different", 200, "Shared")
	_, findings = evaluate([]Page{a, b, c})
	duplicates := 0
	for _, f := range findings {
		if f.RuleID == "duplicate_title_signal" {
			duplicates++
			if len(f.RelatedURLs) != 2 {
				t.Fatal("alias added as third effective duplicate", f)
			}
		}
	}
	if duplicates != 3 {
		t.Fatal("distinct effective pages were missed", duplicates)
	}
}
