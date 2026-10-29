package siteaudit

import (
	"reflect"
	"slices"
	"sort"
	"strings"
)

func cloneCondition(v conditionRecord) conditionRecord {
	v.RelatedURLs = append([]string{}, v.RelatedURLs...)
	v.PendingDependencies = append([]string{}, v.PendingDependencies...)
	return v
}
func conditionKey(page, rule string) string { return page + "\x00" + rule }
func unknownCondition(v conditionRecord) conditionRecord {
	v = cloneCondition(v)
	v.CheckStatus = "unavailable"
	if v.SeenIssue {
		v.Status = "unknown"
	} else {
		v.Status = ""
	}
	return v
}
func pageConflict(p Page, all []Page) bool {
	for _, other := range all {
		if other.Status == "checked" && other.FinalURL == p.FinalURL && (other.HTTPStatus != p.HTTPStatus || other.Title != p.Title || other.Description != p.Description || other.Canonical != p.Canonical || !slices.Equal(other.Links, p.Links) || !sameContentSEO(other.SEO, p.SEO)) {
			return true
		}
	}
	return false
}

func sameContentSEO(a, b *SEO) bool {
	x, y := copySEO(a), copySEO(b)
	if x != nil {
		x.Redirects = nil
		x.RobotsStatus = 0
	}
	if y != nil {
		y.Redirects = nil
		y.RobotsStatus = 0
	}
	return reflect.DeepEqual(x, y)
}

// A subset's passed rule is not sufficient to resolve a cross-page finding.
// Require every previous dependency to be observed successfully under the same
// final URL. Link disappearance alone is unsafe because capture links are capped.
func dependencyProof(old conditionRecord, p Page, all []Page) bool {
	if old.DependencyLimited || p.HTTPStatus < 200 || p.HTTPStatus >= 300 || old.FinalURL != p.FinalURL || pageConflict(p, all) {
		return false
	}
	if old.RuleID != "duplicate_title_signal" && old.RuleID != "broken_checked_link" {
		return true
	}
	if old.RuleID == "duplicate_title_signal" && strings.TrimSpace(p.Title) == "" {
		return false
	}
	for _, target := range old.PendingDependencies {
		found := false
		for _, dependency := range all {
			if dependency.Status == "checked" && dependency.FinalURL == target && dependency.HTTPStatus >= 200 && dependency.HTTPStatus < 300 && !pageConflict(dependency, all) {
				if old.RuleID == "duplicate_title_signal" && (strings.TrimSpace(dependency.Title) == "" || dependency.FinalURL != p.FinalURL && dependency.Title == p.Title) {
					continue
				}
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	return true
}
func proofCondition(v Snapshot, p Page, c Check) conditionRecord {
	value := conditionRecord{InsightItem: InsightItem{PageURL: p.URL, RuleID: c.RuleID, CheckStatus: c.Status, Severity: "warning", ProofAuditID: v.ID, ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, RelatedURLs: []string{}}, FinalURL: p.FinalURL}
	if c.RuleID == "http_error" {
		value.Severity = "error"
	}
	if c.RuleID == "canonical_target_review" {
		value.Severity = "review"
	}
	for _, f := range v.Findings {
		if f.PageURL == p.URL && f.RuleID == c.RuleID {
			value.RelatedURLs = append([]string{}, f.RelatedURLs...)
			break
		}
	}
	return value
}
func compareConditions(old insightRecord, v Snapshot) ([]conditionRecord, ChangeCounts, bool) {
	byKey := map[string]conditionRecord{}
	for _, previous := range old.Conditions {
		byKey[conditionKey(previous.PageURL, previous.RuleID)] = unknownCondition(previous)
	}
	gap := v.Status != "complete"
	if v.Status == "complete" {
		for _, p := range v.Pages {
			if p.Status != "checked" {
				gap = true
				continue
			}
			for _, check := range p.Checks {
				key := conditionKey(p.URL, check.RuleID)
				previous, exists := byKey[key]
				if pageConflict(p, v.Pages) || exists && (!mustTime(p.ObservedAt).After(mustTime(previous.ObservedAt)) || previous.FinalURL != p.FinalURL) {
					gap = true
					continue
				}
				next := proofCondition(v, p, check)
				next.SeenIssue, next.Unresolved, next.EverPassed = previous.SeenIssue, previous.Unresolved, previous.EverPassed
				next.DependencyLimited = previous.DependencyLimited || old.Truncated && !exists
				switch check.Status {
				case "failed", "review":
					next.PendingDependencies = append([]string{}, next.RelatedURLs...)
					if previous.Unresolved {
						var limited bool
						next.PendingDependencies, limited = mergeDependencies(previous, next)
						next.DependencyLimited = next.DependencyLimited || limited
					}
					next.Status = "new"
					if previous.Unresolved {
						next.Status = "unchanged"
					} else if previous.SeenIssue && previous.EverPassed {
						next.Status = "regressed"
					} else if old.Truncated && !exists {
						next.Status = "unknown"
						gap = true
					}
					if next.DependencyLimited {
						next.Status = "unknown"
						gap = true
					}
					next.SeenIssue, next.Unresolved = true, true
				case "passed":
					if previous.Unresolved && !dependencyProof(previous, p, v.Pages) {
						gap = true
						continue
					}
					if previous.Unresolved {
						next.Status = "resolved"
					}
					next.Unresolved, next.EverPassed = false, true
				case "inconclusive":
					gap = true
					if previous.SeenIssue {
						next.Status = "unknown"
					}
					// Preserve dependencies and the most recent determinate proof.
					if exists {
						continue
					}
				}
				byKey[key] = next
			}
		}
	}
	conditions := make([]conditionRecord, 0, len(byKey))
	for _, value := range byKey {
		conditions = append(conditions, value)
	}
	sort.Slice(conditions, func(i, j int) bool {
		return conditionKey(conditions[i].PageURL, conditions[i].RuleID) < conditionKey(conditions[j].PageURL, conditions[j].RuleID)
	})
	return conditions, countConditions(conditions), gap
}
func mergeDependencies(previous, next conditionRecord) ([]string, bool) {
	if next.RuleID != "duplicate_title_signal" && next.RuleID != "broken_checked_link" {
		return next.RelatedURLs, false
	}
	seen := map[string]bool{}
	for _, u := range append(append([]string{}, previous.PendingDependencies...), next.RelatedURLs...) {
		seen[u] = true
	}
	urls := make([]string, 0, len(seen))
	for u := range seen {
		urls = append(urls, u)
	}
	sort.Strings(urls)
	limited := previous.DependencyLimited || len(urls) > maxLinks
	if len(urls) > maxLinks {
		urls = append([]string{}, urls[:maxLinks]...)
	}
	return urls, limited
}
func countConditions(conditions []conditionRecord) ChangeCounts {
	out := ChangeCounts{}
	for _, c := range conditions {
		if !c.SeenIssue {
			continue
		}
		switch c.Status {
		case "new":
			out.New++
		case "unchanged":
			out.Unchanged++
		case "resolved":
			out.Resolved++
		case "regressed":
			out.Regressed++
		case "unknown":
			out.Unknown++
		}
	}
	return out
}
func addChanges(a, b ChangeCounts) ChangeCounts {
	return ChangeCounts{New: a.New + b.New, Unchanged: a.Unchanged + b.Unchanged, Resolved: a.Resolved + b.Resolved, Regressed: a.Regressed + b.Regressed, Unknown: a.Unknown + b.Unknown}
}
