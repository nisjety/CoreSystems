// Package workqueue owns durable, evidence-backed marketing work for one organization.
package workqueue

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"math"
	"regexp"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

var (
	ErrInvalid           = errors.New("invalid marketing work request")
	ErrNotFound          = errors.New("marketing finding or task not found")
	ErrConflict          = errors.New("marketing work context changed")
	ErrSourceUnavailable = errors.New("marketing evidence unavailable")
	ErrCapacity          = errors.New("marketing work capacity reached")
	websitePattern       = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	taskPattern          = regexp.MustCompile(`^task_[a-f0-9]{32}$`)
	eventPattern         = regexp.MustCompile(`^evt_[a-f0-9]{32}$`)
	orgPattern           = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
)

const maxTasks = 500
const maxActivity = 1000
const maxFileBytes = 2 << 20

type Evidence struct {
	Review            *ReviewEvidence `json:"-"`
	Kind              string          `json:"kind"`
	Key               string          `json:"key"`
	OpportunityType   string          `json:"opportunityType"`
	Period            pulse.Period    `json:"period"`
	Clicks            int64           `json:"clicks"`
	Impressions       int64           `json:"impressions"`
	CTR               float64         `json:"ctr"`
	Position          float64         `json:"position"`
	ObservedAt        string          `json:"observedAt"`
	Strategy          string          `json:"-"`
	CheckStatus       string          `json:"-"`
	CheckScore        *float64        `json:"-"`
	LighthouseVersion string          `json:"-"`
	Audit             *AuditEvidence  `json:"-"`
}
type Task struct {
	Planning     *Planning     `json:"planning,omitempty"`
	ID           string        `json:"id"`
	WebsiteID    string        `json:"websiteId"`
	Site         string        `json:"site"`
	Revision     int64         `json:"revision"`
	Status       string        `json:"status"`
	CreatedAt    string        `json:"createdAt"`
	UpdatedAt    string        `json:"updatedAt"`
	Evidence     Evidence      `json:"evidence"`
	Verification *Verification `json:"verification,omitempty"`
}
type Activity struct {
	Planning     *Planning     `json:"planning,omitempty"`
	ID           string        `json:"id"`
	TaskID       string        `json:"taskId"`
	Kind         string        `json:"kind"`
	Status       string        `json:"status"`
	OccurredAt   string        `json:"occurredAt"`
	Verification *Verification `json:"verification,omitempty"`
}
type Summary struct {
	Open              int      `json:"open"`
	InProgress        int      `json:"inProgress"`
	Done              int      `json:"done"`
	Dismissed         int      `json:"dismissed"`
	Total             int      `json:"total"`
	CompletionPercent *float64 `json:"completionPercent"`
}
type Snapshot struct {
	WebsiteID string     `json:"websiteId"`
	Site      string     `json:"site"`
	Tasks     []Task     `json:"tasks"`
	Activity  []Activity `json:"activity"`
	Summary   Summary    `json:"summary"`
}

func newID(prefix string) string {
	var id [16]byte
	_, _ = rand.Read(id[:])
	return prefix + hex.EncodeToString(id[:])
}
func validStatus(value string) bool {
	return value == "todo" || value == "in_progress" || value == "done" || value == "dismissed"
}
func validStamp(value string) bool { _, err := time.Parse(time.RFC3339Nano, value); return err == nil }
func validKey(value string) bool {
	return len(value) > 0 && len(value) <= 512 && strings.TrimSpace(value) != "" && utf8.ValidString(value) && !strings.ContainsFunc(value, unicode.IsControl)
}
func validEvidence(e Evidence) bool {
	if e.Kind == "search_group_review" {
		return validReviewEvidence(e)
	}
	if e.Review != nil {
		return false
	}
	if e.Kind == "site_audit_finding" {
		return validAuditEvidence(e)
	}
	if e.Audit != nil {
		return false
	}
	if e.Kind == "pagespeed_check" {
		return validTechnicalEvidence(e)
	}
	start, a := time.Parse("2006-01-02", e.Period.StartDate)
	end, b := time.Parse("2006-01-02", e.Period.EndDate)
	if e.Kind != "gsc_opportunity" || !validKey(e.Key) || !validStamp(e.ObservedAt) || a != nil || b != nil || end.Before(start) || end.Sub(start) > 366*24*time.Hour {
		return false
	}
	if e.Clicks < 0 || e.Impressions < 100 || e.Clicks > e.Impressions || e.Impressions > 1<<53-1 || math.IsNaN(e.CTR) || math.IsInf(e.CTR, 0) || math.IsNaN(e.Position) || math.IsInf(e.Position, 0) {
		return false
	}
	if e.Position < 1 || e.Position > 20 || e.CTR < 0 || e.CTR >= 0.05 || math.Abs(e.CTR-float64(e.Clicks)/float64(e.Impressions)) > 0.000001 {
		return false
	}
	expected := "content_expansion"
	switch {
	case e.Position <= 3:
		if e.CTR >= 0.03 {
			return false
		}
		expected = "snippet_review"
	case e.Position <= 10 && e.CTR < 0.03:
		expected = "title_meta"
	case e.Position <= 10:
		expected = "title_test"
	}
	return e.OpportunityType == expected
}
func summarize(tasks []Task) Summary {
	result := Summary{Total: len(tasks)}
	for _, task := range tasks {
		switch task.Status {
		case "todo":
			result.Open++
		case "in_progress":
			result.InProgress++
			result.Open++
		case "done":
			result.Done++
		case "dismissed":
			result.Dismissed++
		}
	}
	if active := result.Total - result.Dismissed; active > 0 {
		percent := math.Round(float64(result.Done)/float64(active)*1000) / 10
		result.CompletionPercent = &percent
	}
	return result
}
