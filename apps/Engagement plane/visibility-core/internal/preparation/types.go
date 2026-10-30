// Package preparation owns bounded, durable public-website preparation jobs.
// It stores lifecycle receipts only; source services own measured data.
package preparation

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

var (
	ErrInvalid       = errors.New("invalid website preparation request")
	ErrConflict      = errors.New("website preparation context changed")
	ErrCapacity      = errors.New("website preparation capacity exhausted")
	ErrCooldown      = errors.New("website preparation cooldown active")
	websiteIDPattern = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	jobIDPattern     = regexp.MustCompile(`^prep_[a-f0-9]{32}$`)
	orgPattern       = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
)

const maxJobs = 32
const maxBytes = 1 << 20
const jobTimeout = 3 * time.Minute

var stepKeys = [...]string{"discovery", "pagespeed_mobile", "pagespeed_desktop"}

type Step struct {
	Key        string `json:"key"`
	Status     string `json:"status"`
	Reason     string `json:"reason,omitempty"`
	ObservedAt string `json:"observedAt,omitempty"`
}
type Snapshot struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Source          string `json:"source"`
	Status          string `json:"status"`
	JobID           string `json:"jobId,omitempty"`
	QueuedAt        string `json:"queuedAt,omitempty"`
	StartedAt       string `json:"startedAt,omitempty"`
	CompletedAt     string `json:"completedAt,omitempty"`
	Steps           []Step `json:"steps"`
}
type StepResult struct{ Status, Reason string }
type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type Runner interface {
	Run(context.Context, websites.Website, string) (StepResult, error)
}
type record struct {
	Snapshot
	Attempts int `json:"attempts"`
}
type diskState struct {
	Version        int      `json:"version"`
	OrganizationID string   `json:"organizationId"`
	Jobs           []record `json:"jobs"`
}

func empty(w websites.Website) Snapshot {
	steps := make([]Step, len(stepKeys))
	for i, key := range stepKeys {
		steps[i] = Step{Key: key, Status: "queued"}
	}
	return Snapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "marketing_preparation", Status: "idle", Steps: steps}
}
func clone(v Snapshot) Snapshot { v.Steps = append([]Step{}, v.Steps...); return v }
func cloneState(v diskState) diskState {
	v.Jobs = append([]record{}, v.Jobs...)
	for i := range v.Jobs {
		v.Jobs[i].Snapshot = clone(v.Jobs[i].Snapshot)
	}
	return v
}
func stamp(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }
func parseTime(v string) (time.Time, bool) {
	t, err := time.Parse(time.RFC3339Nano, v)
	return t, err == nil
}
func newID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	return "prep_" + hex.EncodeToString(b[:])
}
func validReason(v string) bool {
	switch v {
	case "not_configured", "source_unavailable", "quota_limited", "capacity_deferred", "timeout", "interrupted", "context_changed":
		return true
	}
	return false
}
func validResult(v StepResult) bool {
	return v.Status == "complete" && v.Reason == "" || (v.Status == "failed" || v.Status == "unavailable") && validReason(v.Reason)
}
func terminal(v string) bool { return v == "complete" || v == "partial" || v == "failed" }
func validState(v diskState) error {
	if v.Version != 1 || !orgPattern.MatchString(v.OrganizationID) || v.Jobs == nil || len(v.Jobs) > maxJobs {
		return ErrInvalid
	}
	seen := map[string]bool{}
	jobs := map[string]bool{}
	for _, r := range v.Jobs {
		canonical, err := pulse.CanonicalSite(r.Site)
		q, qok := parseTime(r.QueuedAt)
		if !websiteIDPattern.MatchString(r.WebsiteID) || seen[r.WebsiteID] || err != nil || canonical != r.Site || r.WebsiteRevision < 1 || r.Source != "marketing_preparation" || !jobIDPattern.MatchString(r.JobID) || jobs[r.JobID] || !qok || r.Attempts < 0 || r.Attempts > 3 || len(r.Steps) != 3 {
			return ErrInvalid
		}
		seen[r.WebsiteID], jobs[r.JobID] = true, true
		if r.Status != "queued" && r.Status != "running" && !terminal(r.Status) {
			return ErrInvalid
		}
		if r.StartedAt != "" {
			start, ok := parseTime(r.StartedAt)
			if !ok || start.Before(q) {
				return ErrInvalid
			}
		}
		if r.Status == "running" && (r.StartedAt == "" || r.Attempts < 1) || r.Status == "queued" && (r.StartedAt != "" || r.CompletedAt != "") {
			return ErrInvalid
		}
		if terminal(r.Status) {
			end, ok := parseTime(r.CompletedAt)
			if !ok || r.StartedAt == "" {
				return ErrInvalid
			}
			start, _ := parseTime(r.StartedAt)
			if end.Before(start) {
				return ErrInvalid
			}
		} else if r.CompletedAt != "" {
			return ErrInvalid
		}
		running, completed, unsuccessful := 0, 0, 0
		for i, s := range r.Steps {
			if s.Key != stepKeys[i] {
				return ErrInvalid
			}
			switch s.Status {
			case "queued":
				if s.Reason != "" || s.ObservedAt != "" {
					return ErrInvalid
				}
			case "running":
				running++
				if s.Reason != "" || s.ObservedAt != "" {
					return ErrInvalid
				}
			case "complete", "failed", "unavailable":
				observed, ok := parseTime(s.ObservedAt)
				if !validResult(StepResult{s.Status, s.Reason}) || !ok || observed.Before(q) {
					return ErrInvalid
				}
				if s.Status == "complete" {
					completed++
				} else {
					unsuccessful++
				}
			default:
				return ErrInvalid
			}
		}
		if running > 1 || running > 0 && r.Status != "running" || terminal(r.Status) && completed+unsuccessful != 3 {
			return ErrInvalid
		}
		if r.Status == "complete" && completed != 3 || r.Status == "failed" && completed != 0 || r.Status == "partial" && (completed == 0 || unsuccessful == 0) {
			return ErrInvalid
		}
	}
	return nil
}
