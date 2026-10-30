package siteaudit

import "time"

func (r *Repository) insight(id, site string, revision int64) (insightRecord, bool, Snapshot) {
	r.mu.Lock()
	defer r.mu.Unlock()
	var current Snapshot
	for i := len(r.state.Runs) - 1; i >= 0; i-- {
		v := r.state.Runs[i]
		if v.WebsiteID == id && v.Site == site && v.WebsiteRevision == revision {
			current = copySnapshot(v)
			break
		}
	}
	for _, v := range r.state.Insights {
		if v.WebsiteID == id && v.Site == site && v.WebsiteRevision == revision && v.RuleVersion == RuleVersion {
			return cloneInsight(v), true, current
		}
	}
	return insightRecord{}, false, current
}
func insightWeeks(v insightRecord) []string {
	out := []string{}
	for _, w := range v.Weeks {
		out = append(out, w.WeekStart)
	}
	return out
}
func (s *Service) Insights(id, site string) (InsightsSnapshot, error) {
	w, err := s.binding(id, site, 0)
	if err != nil {
		return InsightsSnapshot{}, err
	}
	v, exists, current := s.repo.insight(id, site, w.Revision)
	out := InsightsSnapshot{WebsiteID: id, Site: site, WebsiteRevision: w.Revision, Source: "marketing_audit_history", AsOf: stamp(s.now()), Partial: true, HistoryLimited: !exists, LatestStatus: "idle", Items: []InsightItem{}, Alerts: []InsightAlert{}, AvailableWeeks: []string{}}
	if exists {
		out.TrackingSince, out.HistoryLimited, out.Truncated, out.ComparisonAvailable = v.TrackingSince, v.HistoryLimited, v.Truncated, v.ComparisonAvailable
		out.LatestAuditID, out.LatestStatus = v.LatestAuditID, v.LatestStatus
		out.AvailableWeeks = insightWeeks(v)
	}
	gap, reason, observed := v.LatestCoverageGap, v.LatestReason, v.LatestCompletedAt
	if current.ID != "" && current.Status == "running" {
		out.LatestAuditID, out.LatestStatus = current.ID, current.Status
		gap, reason, observed = true, "", current.StartedAt
		for i, c := range v.Conditions {
			v.Conditions[i] = unknownCondition(c)
		}
	}
	for _, c := range v.Conditions {
		if !c.SeenIssue || c.Status == "" {
			continue
		}
		out.Items = append(out.Items, c.InsightItem)
		if c.Status == "new" || c.Status == "regressed" {
			kind := "new_finding"
			if c.Status == "regressed" {
				kind = "regressed_finding"
			}
			out.Alerts = append(out.Alerts, InsightAlert{Kind: kind, AuditID: out.LatestAuditID, PageURL: c.PageURL, RuleID: c.RuleID, ObservedAt: c.ObservedAt})
		}
	}
	out.Summary = countConditions(v.Conditions)
	if out.LatestAuditID != "" && out.LatestStatus == "failed" {
		out.Alerts = append(out.Alerts, InsightAlert{Kind: "audit_failed", AuditID: out.LatestAuditID, Reason: reason, ObservedAt: observed})
	}
	if out.LatestAuditID != "" && gap {
		out.Alerts = append(out.Alerts, InsightAlert{Kind: "coverage_gap", AuditID: out.LatestAuditID, Reason: reason, ObservedAt: observed})
	}
	if _, err = s.binding(id, site, w.Revision); err != nil {
		return InsightsSnapshot{}, err
	}
	return out, nil
}
func (s *Service) Weekly(id, site, requestedWeek string) (WeeklyReport, error) {
	w, err := s.binding(id, site, 0)
	if err != nil {
		return WeeklyReport{}, err
	}
	now := s.now().UTC()
	currentWeek := weekStart(now)
	if requestedWeek == "" {
		requestedWeek = currentWeek.Format("2006-01-02")
	}
	if !validWeek(requestedWeek) {
		return WeeklyReport{}, ErrInvalid
	}
	start, _ := time.Parse("2006-01-02", requestedWeek)
	if start.After(currentWeek) {
		return WeeklyReport{}, ErrInvalid
	}
	v, exists, _ := s.repo.insight(id, site, w.Revision)
	out := WeeklyReport{WebsiteID: id, Site: site, WebsiteRevision: w.Revision, Source: "marketing_audit_history", AsOf: stamp(now), Partial: true, HistoryLimited: !exists, WeekStart: requestedWeek, EndExclusive: start.AddDate(0, 0, 7).Format("2006-01-02"), Status: "closed", Receipts: []WeeklyReceipt{}, AvailableWeeks: []string{}}
	if start.Equal(currentWeek) {
		out.Status = "open"
	}
	if exists {
		out.TrackingSince, out.HistoryLimited, out.Truncated = v.TrackingSince, v.HistoryLimited, v.Truncated
		out.AvailableWeeks = insightWeeks(v)
		if start.Before(mustTime(v.TrackingSince)) {
			out.HistoryLimited = true
		}
		for _, week := range v.Weeks {
			if week.WeekStart == requestedWeek {
				out.Observed, out.Audits, out.Pages, out.Changes, out.FailedChecks = true, week.Audits, week.Pages, week.Changes, week.FailedChecks
				out.Truncated = out.Truncated || week.Truncated
				out.Receipts = append([]WeeklyReceipt{}, week.Receipts...)
				break
			}
		}
	}
	if _, err = s.binding(id, site, w.Revision); err != nil {
		return WeeklyReport{}, err
	}
	return out, nil
}
