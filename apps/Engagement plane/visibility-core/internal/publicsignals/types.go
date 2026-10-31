// Package publicsignals retains bounded public-source projections for registered
// websites. Integration Core owns all provider requests and deployment secrets.
package publicsignals

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

var (
	ErrInvalid   = errors.New("invalid public signals request")
	ErrConflict  = errors.New("public signals website changed")
	ErrCapacity  = errors.New("public signals capacity exhausted")
	ErrCooldown  = errors.New("public signals cooldown active")
	idPattern    = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	jobPattern   = regexp.MustCompile(`^public_[a-f0-9]{32}$`)
	orgPattern   = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	indexPattern = regexp.MustCompile(`^CC-MAIN-[0-9]{4}-[0-9]{2}$`)
)

const (
	maxEntries   = 32
	maxBytes     = 2 << 20
	successTTL   = 24 * time.Hour
	retentionTTL = 48 * time.Hour
	failureDelay = 6 * time.Hour
)

type Period struct {
	StartDate string `json:"startDate"`
	EndDate   string `json:"endDate"`
}
type Metrics struct {
	LCPMs *float64 `json:"lcpMs"`
	INPMs *float64 `json:"inpMs"`
	CLS   *float64 `json:"cls"`
}
type CrUX struct {
	FormFactor      string   `json:"formFactor"`
	ReportingOrigin string   `json:"reportingOrigin,omitempty"`
	Status          string   `json:"status"`
	Reason          string   `json:"reason,omitempty"`
	Period          *Period  `json:"period,omitempty"`
	Metrics         *Metrics `json:"metrics,omitempty"`
}
type Capture struct {
	URL        string `json:"url"`
	CapturedAt string `json:"capturedAt"`
	HTTPStatus int    `json:"httpStatus"`
}
type CommonCrawl struct {
	ReportingHost string    `json:"reportingHost,omitempty"`
	Status        string    `json:"status"`
	Reason        string    `json:"reason,omitempty"`
	Index         string    `json:"index,omitempty"`
	Captures      []Capture `json:"captures"`
	Limit         int       `json:"limit"`
	Truncated     bool      `json:"truncated"`
}
type Report struct {
	Site        string      `json:"site"`
	Source      string      `json:"source"`
	ObservedAt  string      `json:"observedAt"`
	CrUX        []CrUX      `json:"crux"`
	CommonCrawl CommonCrawl `json:"commonCrawl"`
}
type State struct {
	WebsiteID       string  `json:"websiteId"`
	Site            string  `json:"site"`
	WebsiteRevision int64   `json:"websiteRevision"`
	Source          string  `json:"source"`
	Status          string  `json:"status"`
	JobID           string  `json:"jobId,omitempty"`
	Enabled         bool    `json:"enabled"`
	QueuedAt        string  `json:"queuedAt,omitempty"`
	StartedAt       string  `json:"startedAt,omitempty"`
	CompletedAt     string  `json:"completedAt,omitempty"`
	NextSyncAt      string  `json:"nextSyncAt,omitempty"`
	ExpiresAt       string  `json:"expiresAt,omitempty"`
	Report          *Report `json:"report,omitempty"`
	CachedReport    *Report `json:"cachedReport,omitempty"`
	Reason          string  `json:"reason,omitempty"`
}
type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type Runner interface {
	Collect(context.Context, websites.Record) (Report, error)
}
type diskState struct {
	Version        int     `json:"version"`
	OrganizationID string  `json:"organizationId"`
	Entries        []State `json:"entries"`
}

func stamp(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }
func parse(v string) time.Time { t, _ := time.Parse(time.RFC3339Nano, v); return t }
func clone[T any](v T) T {
	raw, err := json.Marshal(v)
	if err != nil {
		panic("public signals validated JSON invariant")
	}
	var next T
	if json.Unmarshal(raw, &next) != nil {
		panic("public signals typed JSON invariant")
	}
	return next
}
func newID() string {
	var v [16]byte
	_, _ = rand.Read(v[:])
	return "public_" + hex.EncodeToString(v[:])
}
func empty(w websites.Website) State {
	return State{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "marketing_public_signals", Status: "idle"}
}
func providerReason(v string) bool {
	return v == "not_configured" || v == "no_data" || v == "quota_limited" || v == "source_unavailable" || v == "timeout"
}
func jobReason(v string) bool {
	return v == "source_unavailable" || v == "timeout" || v == "interrupted" || v == "context_changed"
}
