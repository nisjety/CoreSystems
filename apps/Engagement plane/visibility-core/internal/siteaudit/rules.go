package siteaudit

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
)

var rules = [...]string{"http_error", "missing_title_signal", "missing_description_signal", "duplicate_title_signal", "canonical_target_review", "broken_checked_link", "h1_structure", "heading_sequence", "noindex_directive", "redirect_chain_review", "structured_data_parse", "hreflang_syntax", "canonical_outside_site", "hreflang_reciprocity"}

func validRule(rule string) bool {
	for _, value := range rules {
		if value == rule {
			return true
		}
	}
	return false
}
func checkStatus(ok bool) string {
	if ok {
		return "passed"
	}
	return "failed"
}

// evaluate never assumes an unvisited URL is healthy/broken or that a partial
// inventory establishes global uniqueness. Titles/descriptions are signals,
// not proof of the presence of particular HTML elements.
func evaluate(input []Page) ([]Page, []Finding) {
	pages := copySnapshot(Snapshot{Pages: input}).Pages
	findings := []Finding{}
	titles := map[string][]string{}
	titleFinals := map[string]map[string]bool{}
	checked := map[string]int{}
	htmlFinals := map[string]bool{}
	for _, p := range pages {
		if p.Status != "checked" {
			continue
		}
		checked[p.URL] = p.HTTPStatus
		checked[p.FinalURL] = p.HTTPStatus
		if p.HTTPStatus >= 200 && p.HTTPStatus < 300 {
			htmlFinals[p.FinalURL] = true
			if title := strings.TrimSpace(p.Title); title != "" {
				if titleFinals[title] == nil {
					titleFinals[title] = map[string]bool{}
				}
				if !titleFinals[title][p.FinalURL] {
					titles[title] = append(titles[title], p.FinalURL)
					titleFinals[title][p.FinalURL] = true
				}
			}
		}
	}
	for i, p := range pages {
		if p.Status != "checked" {
			continue
		}
		p.Checks = []Check{{RuleID: "http_error", Status: checkStatus(p.HTTPStatus < 400)}}
		if p.HTTPStatus >= 400 {
			findings = append(findings, finding(p, "http_error", "error", nil))
		}
		if p.HTTPStatus < 200 || p.HTTPStatus >= 300 {
			pages[i] = p
			continue
		}
		for _, item := range []struct{ rule, value string }{{"missing_title_signal", p.Title}, {"missing_description_signal", p.Description}} {
			status := checkStatus(strings.TrimSpace(item.value) != "")
			p.Checks = append(p.Checks, Check{RuleID: item.rule, Status: status})
			if status == "failed" {
				findings = append(findings, finding(p, item.rule, "warning", nil))
			}
		}
		status := "inconclusive"
		if len(htmlFinals) >= 2 {
			status = "passed"
		}
		if matches := titles[strings.TrimSpace(p.Title)]; strings.TrimSpace(p.Title) != "" && len(matches) > 1 {
			status = "failed"
			findings = append(findings, finding(p, "duplicate_title_signal", "warning", matches))
		}
		p.Checks = append(p.Checks, Check{RuleID: "duplicate_title_signal", Status: status})
		status = "inconclusive"
		if p.Canonical != "" {
			status = "passed"
			if p.Canonical != p.FinalURL {
				status = "review"
				findings = append(findings, finding(p, "canonical_target_review", "review", []string{p.Canonical}))
			}
		}
		p.Checks = append(p.Checks, Check{RuleID: "canonical_target_review", Status: status})
		broken := []string{}
		unknown := false
		for _, link := range p.Links {
			code, ok := checked[link]
			if !ok {
				unknown = true
				continue
			}
			if code == 404 || code == 410 {
				broken = append(broken, link)
			}
		}
		status = "passed"
		if unknown {
			status = "inconclusive"
		}
		if len(broken) > 0 {
			status = "failed"
			findings = append(findings, finding(p, "broken_checked_link", "warning", broken))
		}
		p.Checks = append(p.Checks, Check{RuleID: "broken_checked_link", Status: status})
		findings = append(findings, seoChecks(&p)...)
		if p.SEO != nil {
			status, related := hreflangReciprocity(p, pages)
			p.Checks = append(p.Checks, Check{RuleID: "hreflang_reciprocity", Status: status})
			if status == "review" {
				findings = append(findings, finding(p, "hreflang_reciprocity", "review", related))
			}
		}
		pages[i] = p
	}
	return pages, findings
}
func finding(p Page, rule, severity string, related []string) Finding {
	id := sha256.Sum256([]byte(p.RunID + "\x00" + p.URL + "\x00" + rule))
	return Finding{ID: "finding_" + hex.EncodeToString(id[:16]), RuleID: rule, Severity: severity, PageURL: p.URL, ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, RelatedURLs: append([]string{}, related...)}
}
