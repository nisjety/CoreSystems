package siteaudit

import (
	"sort"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

func validChanges(v ChangeCounts) bool {
	for _, n := range []int{v.New, v.Unchanged, v.Resolved, v.Regressed, v.Unknown} {
		if n < 0 || int64(n) > 1<<53-1 {
			return false
		}
	}
	return true
}
func validWeek(value string) bool {
	t, err := time.Parse("2006-01-02", value)
	return err == nil && len(value) == 10 && t.Weekday() == time.Monday
}
func validCondition(v conditionRecord, site string) bool {
	if !discovery.SafeURL(site, v.PageURL) || !discovery.SafeURL(site, v.FinalURL) || !validRule(v.RuleID) || !auditPattern.MatchString(v.ProofAuditID) || !validStamp(v.ObservedAt) || !runPattern.MatchString(v.RunID) || !fingerprintPattern.MatchString(v.Fingerprint) || v.RelatedURLs == nil || len(v.RelatedURLs) > maxLinks {
		return false
	}
	if v.Severity != "warning" && v.Severity != "error" && v.Severity != "review" {
		return false
	}
	expected := "warning"
	if v.RuleID == "http_error" {
		expected = "error"
	}
	if v.RuleID == "canonical_target_review" {
		expected = "review"
	}
	if v.Severity != expected {
		return false
	}
	for _, u := range v.RelatedURLs {
		if !discovery.SafeURL(site, u) {
			return false
		}
	}
	if len(v.PendingDependencies) > maxLinks {
		return false
	}
	for _, u := range v.PendingDependencies {
		if !discovery.SafeURL(site, u) {
			return false
		}
	}
	switch v.CheckStatus {
	case "passed", "failed", "review", "inconclusive", "unavailable":
	default:
		return false
	}
	if !v.SeenIssue && v.Status != "" {
		return false
	}
	switch v.Status {
	case "":
		return !v.Unresolved
	case "new", "unchanged", "regressed":
		return v.Unresolved && (v.CheckStatus == "failed" || v.CheckStatus == "review")
	case "resolved":
		return v.SeenIssue && !v.Unresolved && v.EverPassed && v.CheckStatus == "passed"
	case "unknown":
		return v.SeenIssue && v.CheckStatus != "passed"
	default:
		return false
	}
}
func validWeekRecord(w weekRecord) bool {
	if !validWeek(w.WeekStart) || !validChanges(w.Changes) || w.Audits.Total < 1 || w.Audits.Total != w.Audits.Complete+w.Audits.Failed || w.Audits.Complete < 0 || w.Audits.Failed < 0 || int64(w.Audits.Total) > 1<<53-1 || w.Pages.Attempted != w.Pages.Checked+w.Pages.Failed || w.Pages.Attempted < 0 || w.Pages.Checked < 0 || w.Pages.Failed < 0 || int64(w.Pages.Attempted) > 1<<53-1 || int64(w.Pages.Checked) > 1<<53-1 || int64(w.Pages.Failed) > 1<<53-1 || w.FailedChecks < 0 || int64(w.FailedChecks) > 1<<53-1 || w.Receipts == nil || len(w.Receipts) > maxWeeklyReceipts || len(w.Receipts) > w.Audits.Total {
		return false
	}
	start, _ := time.Parse("2006-01-02", w.WeekStart)
	ids := map[string]bool{}
	checked, failed := 0, 0
	for _, r := range w.Receipts {
		if ids[r.AuditID] || !auditPattern.MatchString(r.AuditID) || !validStamp(r.StartedAt) || !validStamp(r.CompletedAt) || mustTime(r.CompletedAt).Before(mustTime(r.StartedAt)) || mustTime(r.CompletedAt).Before(start) || !mustTime(r.CompletedAt).Before(start.AddDate(0, 0, 7)) || r.Status != "complete" && r.Status != "failed" || r.CheckedPages < 0 || r.FailedPages < 0 || r.CheckedPages+r.FailedPages > Limit || r.FindingCount < 0 || r.FindingCount > Limit*len(rules) || !validReason(r.Reason) || r.Status == "failed" && r.Reason == "" || r.Status == "complete" && r.Reason != "" {
			return false
		}
		ids[r.AuditID] = true
		checked += r.CheckedPages
		failed += r.FailedPages
	}
	if checked > w.Pages.Checked || failed > w.Pages.Failed {
		return false
	}
	return w.Truncated || len(w.Receipts) == w.Audits.Total && checked == w.Pages.Checked && failed == w.Pages.Failed
}
func validateInsights(version int, records []insightRecord) error {
	if version == 0 && records == nil {
		return nil
	}
	if version != 1 || records == nil || len(records) > maxInsightSites {
		return ErrInvalid
	}
	ids := map[string]bool{}
	for _, v := range records {
		if ids[v.WebsiteID] || !websitePattern.MatchString(v.WebsiteID) || !discovery.SafeURL(v.Site, v.Site) || v.WebsiteRevision < 1 || v.WebsiteRevision > 1<<53-1 || v.RuleVersion != RuleVersion || !validStamp(v.TrackingSince) || !auditPattern.MatchString(v.LatestAuditID) || v.LatestStatus != "complete" && v.LatestStatus != "failed" || !validStamp(v.LatestStartedAt) || !validStamp(v.LatestCompletedAt) || mustTime(v.LatestStartedAt).Before(mustTime(v.TrackingSince)) || mustTime(v.LatestCompletedAt).Before(mustTime(v.LatestStartedAt)) || !validReason(v.LatestReason) || v.LatestStatus == "failed" && v.LatestReason == "" || v.LatestStatus == "complete" && v.LatestReason != "" || v.Conditions == nil || len(v.Conditions) > maxConditions || v.Weeks == nil || len(v.Weeks) > maxInsightWeeks || v.Processed == nil || len(v.Processed) > maxReceiptIDs || len(v.Processed) == 0 {
			return ErrInvalid
		}
		ids[v.WebsiteID] = true
		keys := map[string]bool{}
		for _, c := range v.Conditions {
			key := conditionKey(c.PageURL, c.RuleID)
			if keys[key] || !validCondition(c, v.Site) || mustTime(c.ObservedAt).After(mustTime(v.LatestCompletedAt)) {
				return ErrInvalid
			}
			keys[key] = true
		}
		weeks := map[string]bool{}
		for _, w := range v.Weeks {
			if weeks[w.WeekStart] || !validWeekRecord(w) {
				return ErrInvalid
			}
			weeks[w.WeekStart] = true
		}
		if !sort.SliceIsSorted(v.Weeks, func(i, j int) bool { return v.Weeks[i].WeekStart > v.Weeks[j].WeekStart }) {
			return ErrInvalid
		}
		processed := map[string]bool{}
		for _, id := range v.Processed {
			if processed[id] || !auditPattern.MatchString(id) {
				return ErrInvalid
			}
			processed[id] = true
		}
		if !processed[v.LatestAuditID] {
			return ErrInvalid
		}
	}
	return nil
}
