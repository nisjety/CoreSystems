// Package collection describes dataset availability without fetching providers
// or treating missing measurements as zero. Owners supply authorized saved state.
package collection

import "time"

type Period struct {
	StartDate string `json:"startDate"`
	EndDate   string `json:"endDate"`
}
type Dataset struct {
	Key               string  `json:"key"`
	Provider          string  `json:"provider"`
	Category          string  `json:"category"`
	Status            string  `json:"status"`
	Reason            string  `json:"reason,omitempty"`
	Coverage          string  `json:"coverage"`
	Partial           bool    `json:"partial"`
	Records           *int    `json:"records"`
	Unit              string  `json:"unit"`
	LastSuccessAt     string  `json:"lastSuccessAt,omitempty"`
	LastAttemptAt     string  `json:"lastAttemptAt,omitempty"`
	NextSyncAt        string  `json:"nextSyncAt,omitempty"`
	ReportingResource string  `json:"reportingResource,omitempty"`
	Period            *Period `json:"period,omitempty"`
	History           string  `json:"history"`
	CollectionMode    string  `json:"collectionMode"`
}
type Summary struct {
	Available   int `json:"available"`
	Empty       int `json:"empty"`
	Pending     int `json:"pending"`
	Stale       int `json:"stale"`
	Attention   int `json:"attention"`
	Unsupported int `json:"unsupported"`
}
type Inventory struct {
	WebsiteID       string    `json:"websiteId"`
	Site            string    `json:"site"`
	WebsiteRevision int64     `json:"websiteRevision"`
	Source          string    `json:"source"`
	AsOf            string    `json:"asOf"`
	Quarter         string    `json:"quarter"`
	Datasets        []Dataset `json:"datasets"`
	Summary         Summary   `json:"summary"`
}

// Finish does not assign counts to absent/expired/unauthorized data. A measured
// empty report is distinct from a collector which has not returned a report.
func Finish(d Dataset, rawStatus, reason, observed string, count *int, freshFor time.Duration, now time.Time) Dataset {
	d.Reason = ""
	d.Status = "pending"
	switch reason {
	case "snapshot_pending":
		d.Status = "pending"
	case "authorization_required", "requires_consent", "not_connected":
		d.Status = "reconnect"
		d.Reason = "authorization_required"
	case "not_configured", "needs_selection", "property_unverified", "pending_verification":
		d.Status = "setup"
		d.Reason = "setup_required"
	case "snapshot_expired", "report_expired":
		d.Status = "stale"
		d.Reason = "expired"
	case "not_found":
		d.Status = "setup"
		d.Reason = "setup_required"
	case "no_data", "insufficient_data":
		d.Status = "empty"
		d.Reason = "no_coverage"
	case "quota_limited":
		d.Status = "failed"
		d.Reason = "quota_limited"
	case "source_unavailable", "timeout", "interrupted", "storage_unavailable":
		d.Status = "failed"
		d.Reason = "source_unavailable"
	default:
		switch rawStatus {
		case "connected", "complete":
			d.Status = "available"
		case "no_data":
			d.Status = "empty"
			d.Reason = "no_coverage"
		case "plan_required":
			d.Status = "setup"
			d.Reason = "plan_required"
		case "access_denied":
			d.Status = "reconnect"
			d.Reason = "authorization_required"
		case "rate_limited":
			d.Status = "failed"
			d.Reason = "quota_limited"
		case "not_configured":
			d.Status = "setup"
			d.Reason = "setup_required"
		case "requires_consent", "not_connected", "authorization_required":
			d.Status = "reconnect"
			d.Reason = "authorization_required"
		case "failed", "unavailable":
			d.Status = "failed"
			d.Reason = "source_unavailable"
		}
	}
	if rawStatus == "connected" || rawStatus == "complete" {
		if t, err := time.Parse(time.RFC3339Nano, observed); err == nil && !t.After(now.Add(5*time.Minute)) {
			d.LastSuccessAt = observed
		}
	}
	if d.Status == "available" || d.Status == "empty" {
		t, err := time.Parse(time.RFC3339Nano, observed)
		if err != nil || t.After(now.Add(5*time.Minute)) {
			d.Status = "failed"
			d.Reason = "invalid_receipt"
		} else {
			d.LastSuccessAt = observed
			d.Records = count
			if freshFor > 0 && !t.Add(freshFor).After(now) {
				d.Status = "stale"
				d.Reason = "expired"
			} else if count != nil && *count == 0 {
				d.Status = "empty"
			}
		}
	}
	return d
}
func Count(items []Dataset) Summary {
	var s Summary
	for _, d := range items {
		switch d.Status {
		case "available":
			s.Available++
		case "empty":
			s.Empty++
		case "pending":
			s.Pending++
		case "stale":
			s.Stale++
		case "unsupported":
			s.Unsupported++
		default:
			s.Attention++
		}
	}
	return s
}
func N(v int) *int { return &v }
