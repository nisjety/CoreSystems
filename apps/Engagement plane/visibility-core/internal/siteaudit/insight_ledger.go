package siteaudit

import (
	"encoding/json"
	"slices"
	"sort"
	"time"
)

func weekStart(t time.Time) time.Time {
	t = t.UTC()
	day := time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC)
	return day.AddDate(0, 0, -(int(t.Weekday())+6)%7)
}
func cloneInsight(v insightRecord) insightRecord {
	v.Conditions = append([]conditionRecord{}, v.Conditions...)
	for i, c := range v.Conditions {
		v.Conditions[i] = cloneCondition(c)
	}
	v.Weeks = append([]weekRecord{}, v.Weeks...)
	for i, w := range v.Weeks {
		w.Receipts = append([]WeeklyReceipt{}, w.Receipts...)
		v.Weeks[i] = w
	}
	v.Processed = append([]string{}, v.Processed...)
	return v
}
func emptyInsight(v Snapshot) insightRecord {
	return insightRecord{WebsiteID: v.WebsiteID, Site: v.Site, WebsiteRevision: v.WebsiteRevision, RuleVersion: v.RuleVersion, TrackingSince: v.StartedAt, Conditions: []conditionRecord{}, Weeks: []weekRecord{}, Processed: []string{}}
}
func recordInsights(state diskState, v Snapshot) (diskState, error) {
	if v.Status != "complete" && v.Status != "failed" {
		return state, nil
	}
	index := -1
	value := emptyInsight(v)
	for i, old := range state.Insights {
		if old.WebsiteID != v.WebsiteID {
			continue
		}
		// An old run finishing after a context change cannot replace the new
		// revision's baseline or contribute observations to its reports.
		if old.WebsiteRevision > v.WebsiteRevision {
			return state, nil
		}
		index = i
		if old.WebsiteRevision == v.WebsiteRevision && old.Site == v.Site && old.RuleVersion == v.RuleVersion {
			value = cloneInsight(old)
		}
		break
	}
	if slices.Contains(value.Processed, v.ID) {
		return state, nil
	}
	older := value.LatestStartedAt != "" && !mustTime(v.StartedAt).After(mustTime(value.LatestStartedAt))
	if older {
		// Idempotency survives eviction of the bounded ID cache. Chronologically
		// older receipts may count once only when their persisted running claim
		// still establishes this exact attempt. Never replay an evicted terminal.
		claimed := false
		for _, run := range state.Runs {
			if run.ID == v.ID && run.Status == "running" {
				claimed = true
				break
			}
		}
		if !claimed {
			return state, nil
		}
	}
	if index < 0 && len(state.Insights) >= maxInsightSites {
		return diskState{}, ErrCapacity
	}
	conditions, changes, gap := compareConditions(value, v)
	for _, c := range conditions {
		if c.DependencyLimited {
			value.Truncated, value.HistoryLimited = true, true
			break
		}
	}
	if older {
		conditions, changes, gap = value.Conditions, ChangeCounts{}, value.LatestCoverageGap
	}
	if len(conditions) > maxConditions {
		conditions = boundedConditions(conditions)
		changes = countConditions(conditions)
		value.Truncated, value.HistoryLimited, gap = true, true, true
	}
	if !older {
		value.ComparisonAvailable = len(value.Conditions) > 0 && !value.Truncated
		value.LatestAuditID, value.LatestStatus, value.LatestStartedAt, value.LatestCompletedAt, value.LatestReason = v.ID, v.Status, v.StartedAt, v.CompletedAt, v.Reason
		value.Conditions, value.LatestCoverageGap = conditions, gap || changes.Unknown > 0
	}
	if mustTime(v.StartedAt).Before(mustTime(value.TrackingSince)) {
		value.TrackingSince = v.StartedAt
	}
	value.Processed = append(value.Processed, v.ID)
	if len(value.Processed) > maxReceiptIDs {
		value.Processed = append([]string{}, value.Processed[len(value.Processed)-maxReceiptIDs:]...)
	}
	value = recordWeek(value, v, changes)
	if index < 0 {
		state.Insights = append(state.Insights, value)
	} else {
		state.Insights[index] = value
	}
	return state, nil
}
func boundedConditions(values []conditionRecord) []conditionRecord {
	// Prefer retaining unresolved issues over healthy baselines. If even those
	// exceed the bound, the explicit truncation flag disables future new-vs-
	// regressed claims for absent conditions; absence never proves resolution.
	out := append([]conditionRecord{}, values...)
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].Unresolved != out[j].Unresolved {
			return out[i].Unresolved
		}
		if out[i].SeenIssue != out[j].SeenIssue {
			return out[i].SeenIssue
		}
		return conditionKey(out[i].PageURL, out[i].RuleID) < conditionKey(out[j].PageURL, out[j].RuleID)
	})
	return append([]conditionRecord{}, out[:maxConditions]...)
}
func recordWeek(value insightRecord, v Snapshot, changes ChangeCounts) insightRecord {
	key := weekStart(mustTime(v.CompletedAt)).Format("2006-01-02")
	index := -1
	w := weekRecord{WeekStart: key, Receipts: []WeeklyReceipt{}}
	for i, old := range value.Weeks {
		if old.WeekStart == key {
			index, w = i, old
			break
		}
	}
	w.Audits.Total++
	if v.Status == "complete" {
		w.Audits.Complete++
	} else {
		w.Audits.Failed++
	}
	r := WeeklyReceipt{AuditID: v.ID, Status: v.Status, StartedAt: v.StartedAt, CompletedAt: v.CompletedAt, FindingCount: len(v.Findings), Reason: v.Reason}
	for _, p := range v.Pages {
		if p.Status == "checked" {
			r.CheckedPages++
		} else {
			r.FailedPages++
		}
		for _, check := range p.Checks {
			if check.Status == "failed" {
				w.FailedChecks++
			}
		}
	}
	w.Pages.Attempted += len(v.Pages)
	w.Pages.Checked, w.Pages.Failed = w.Pages.Checked+r.CheckedPages, w.Pages.Failed+r.FailedPages
	w.Changes = addChanges(w.Changes, changes)
	w.Receipts = append([]WeeklyReceipt{r}, w.Receipts...)
	sort.Slice(w.Receipts, func(i, j int) bool {
		return mustTime(w.Receipts[i].CompletedAt).After(mustTime(w.Receipts[j].CompletedAt))
	})
	if len(w.Receipts) > maxWeeklyReceipts {
		w.Receipts = append([]WeeklyReceipt{}, w.Receipts[:maxWeeklyReceipts]...)
		w.Truncated = true
	}
	if index < 0 {
		value.Weeks = append(value.Weeks, w)
	} else {
		value.Weeks[index] = w
	}
	sort.Slice(value.Weeks, func(i, j int) bool { return value.Weeks[i].WeekStart > value.Weeks[j].WeekStart })
	if len(value.Weeks) > maxInsightWeeks {
		value.Weeks = append([]weekRecord{}, value.Weeks[:maxInsightWeeks]...)
		value.HistoryLimited = true
	}
	return value
}

// Journaling and report eviction share one protected-file commit. Compact facts
// are never reconstructed from a later subset, and a post-rename ambiguity uses
// the same in-memory committed state as the existing audit repository.
func fitInsightBudget(state diskState, keep string) (diskState, error) {
	for pass := 0; pass < 12; pass++ {
		raw, err := json.Marshal(state)
		if err != nil {
			return diskState{}, err
		}
		if len(raw) <= maxFileBytes {
			return state, nil
		}
		switch pass {
		case 0:
			// One batch removes old terminal full reports, preserving current and
			// in-flight reports. Their compact facts are already journaled.
			retained := []Snapshot{}
			for _, v := range state.Runs {
				if v.ID == keep || v.Status == "running" {
					retained = append(retained, v)
				}
			}
			state.Runs = retained
		case 1:
			for i, v := range state.Insights {
				for j, w := range v.Weeks {
					if len(w.Receipts) > 0 {
						w.Receipts = []WeeklyReceipt{}
						w.Truncated = true
						v.Weeks[j] = w
						v.Truncated = true
						v.HistoryLimited = true
					}
				}
				state.Insights[i] = v
			}
		case 2:
			for i, v := range state.Insights {
				if len(v.Weeks) > 1 {
					v.Weeks = append([]weekRecord{}, v.Weeks[:1]...)
					v.Truncated = true
					v.HistoryLimited = true
					state.Insights[i] = v
				}
			}
		default:
			for i, v := range state.Insights {
				if len(v.Conditions) > 0 {
					// Keep unresolved evidence first; absent pruned identities become
					// unknown rather than falsely new/resolved on subsequent runs.
					v.Conditions = boundedConditionsFor(v.Conditions, len(v.Conditions)/2)
					v.Truncated, v.HistoryLimited, v.ComparisonAvailable, v.LatestCoverageGap = true, true, false, true
					state.Insights[i] = v
				}
			}
		}
	}
	return diskState{}, ErrCapacity
}
func boundedConditionsFor(values []conditionRecord, limit int) []conditionRecord {
	out := append([]conditionRecord{}, values...)
	sort.SliceStable(out, func(i, j int) bool { return out[i].Unresolved && !out[j].Unresolved })
	return append([]conditionRecord{}, out[:limit]...)
}
