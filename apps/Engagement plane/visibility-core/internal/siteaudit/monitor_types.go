package siteaudit

import "time"

const maxSchedules = 64
const maxMonitorHistory = 10

// MonitorReceipt survives bounded full-report eviction; it contains no captured
// page content and never implies that a failed run checked every page.
type MonitorReceipt struct {
	AuditID      string `json:"auditId"`
	ScheduledFor string `json:"scheduledFor"`
	StartedAt    string `json:"startedAt"`
	CompletedAt  string `json:"completedAt"`
	Status       string `json:"status"`
	Attempt      int    `json:"attempt"`
	CheckedPages int    `json:"checkedPages"`
	FindingCount int    `json:"findingCount"`
	Reason       string `json:"reason,omitempty"`
}

type MonitorSnapshot struct {
	WebsiteID       string           `json:"websiteId"`
	Site            string           `json:"site"`
	WebsiteRevision int64            `json:"websiteRevision"`
	Source          string           `json:"source"`
	Revision        int64            `json:"revision"`
	Configured      bool             `json:"configured"`
	Enabled         bool             `json:"enabled"`
	Cadence         string           `json:"cadence"`
	Status          string           `json:"status"`
	NextRunAt       string           `json:"nextRunAt"`
	UpdatedAt       string           `json:"updatedAt"`
	LastAuditID     string           `json:"lastAuditId"`
	LastOutcome     string           `json:"lastOutcome"`
	FailureCount    int              `json:"failureCount"`
	Reason          string           `json:"reason,omitempty"`
	History         []MonitorReceipt `json:"history"`
}

// A scheduled occurrence and its audit receipt are committed in audits.json in
// one transaction. PendingFor remains stable across capacity deferrals/retries.
type monitorRecord struct {
	MonitorSnapshot
	PendingFor     string `json:"pendingFor,omitempty"`
	PendingAttempt int    `json:"pendingAttempt,omitempty"`
}

func interval(cadence string) time.Duration {
	if cadence == "daily" {
		return 24 * time.Hour
	}
	if cadence == "weekly" {
		return 7 * 24 * time.Hour
	}
	return 0
}
func monitorReason(value string) bool { return value == "capacity_deferred" || validReason(value) }
func cloneMonitor(v monitorRecord) monitorRecord {
	v.History = append([]MonitorReceipt{}, v.History...)
	return v
}
func validateMonitors(records []monitorRecord) error {
	if len(records) > maxSchedules {
		return ErrInvalid
	}
	seen := map[string]bool{}
	for _, v := range records {
		if seen[v.WebsiteID] || !websitePattern.MatchString(v.WebsiteID) || !validMonitorSite(v.Site) || v.WebsiteRevision < 1 || v.Source != "marketing_scheduler" || v.Revision < 1 || !v.Configured || interval(v.Cadence) == 0 || !validStamp(v.UpdatedAt) || !monitorReason(v.Reason) || v.History == nil || len(v.History) > maxMonitorHistory || v.FailureCount < 0 || v.FailureCount > 3 {
			return ErrInvalid
		}
		seen[v.WebsiteID] = true
		if v.PendingFor != "" && (!validStamp(v.PendingFor) || v.PendingAttempt < 1 || v.PendingAttempt > 3) || v.PendingFor == "" && v.PendingAttempt != 0 || v.NextRunAt != "" && !validStamp(v.NextRunAt) {
			return ErrInvalid
		}
		switch v.Status {
		case "active":
			if !v.Enabled || v.NextRunAt == "" {
				return ErrInvalid
			}
		case "paused", "context_changed", "blocked":
			if v.Enabled || v.NextRunAt != "" {
				return ErrInvalid
			}
		default:
			return ErrInvalid
		}
		if v.LastOutcome == "none" {
			if v.LastAuditID != "" || len(v.History) != 0 || v.FailureCount != 0 {
				return ErrInvalid
			}
		} else if len(v.History) == 0 || v.LastAuditID != v.History[0].AuditID || v.LastOutcome != v.History[0].Status {
			return ErrInvalid
		}
		ids := map[string]bool{}
		for i, h := range v.History {
			if ids[h.AuditID] || !auditPattern.MatchString(h.AuditID) || !validStamp(h.ScheduledFor) || !validStamp(h.StartedAt) || mustTime(h.StartedAt).Before(mustTime(h.ScheduledFor)) || h.Attempt < 1 || h.Attempt > 3 || h.CheckedPages < 0 || h.CheckedPages > Limit || h.FindingCount < 0 || h.FindingCount > Limit*len(rules) || !validReason(h.Reason) {
				return ErrInvalid
			}
			ids[h.AuditID] = true
			if h.Status == "running" {
				if i != 0 || h.CompletedAt != "" || h.Reason != "" || h.CheckedPages != 0 || h.FindingCount != 0 {
					return ErrInvalid
				}
			} else if h.Status != "complete" && h.Status != "failed" || !validStamp(h.CompletedAt) || mustTime(h.CompletedAt).Before(mustTime(h.StartedAt)) || h.Status == "complete" && h.Reason != "" || h.Status == "failed" && h.Reason == "" {
				return ErrInvalid
			}
		}
	}
	return nil
}
