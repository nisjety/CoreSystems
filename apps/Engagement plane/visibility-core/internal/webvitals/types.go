// Package webvitals owns browser-reported, first-party field measurements.
// These are measurements, not verified visitors or a Chrome UX Report.
package webvitals

import (
	"context"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const (
	maxSamples    = 5000
	maxEntries    = 64 // At most 32 registered websites in two environments.
	maxBytes      = 2 << 20
	RetentionDays = 7
)

var (
	ErrInvalid     = errors.New("invalid web vitals request or receipt")
	ErrForbidden   = errors.New("verified website ownership required")
	ErrCooldown    = errors.New("web vitals refresh already running or cooling down")
	ErrCapacity    = errors.New("web vitals storage is full")
	ErrUnavailable = errors.New("first-party measurement source unavailable")
	idPattern      = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	orgPattern     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	uuidPattern    = regexp.MustCompile(`^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$`)
	pathPattern    = regexp.MustCompile(`^/(?:[A-Za-z0-9_-]+/?)*$`)
)

type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type Collector interface {
	Collect(context.Context, websites.Record, string) (Export, error)
}
type Sample struct {
	SampleID   string  `json:"sampleId"`
	Sequence   int     `json:"sequence"`
	Metric     string  `json:"metric"`
	Value      float64 `json:"value"`
	Viewport   string  `json:"viewport"`
	Path       string  `json:"path"`
	ReceivedAt string  `json:"receivedAt"`
}

// Export is an Integration Core receipt. It never contains credentials, browser
// identifiers, IP addresses, referrers, query strings, or client timestamps.
type Export struct {
	WebsiteID       string   `json:"websiteId"`
	Site            string   `json:"site"`
	WebsiteRevision int64    `json:"websiteRevision"`
	Environment     string   `json:"environment"`
	ReportingOrigin string   `json:"reportingOrigin,omitempty"`
	Status          string   `json:"status"`
	Samples         []Sample `json:"samples"`
	ObservedAt      string   `json:"observedAt"`
}
type Statistic struct {
	Count int      `json:"count"`
	P75   *float64 `json:"p75"`
}
type Metrics struct {
	LCP Statistic `json:"lcp"`
	INP Statistic `json:"inp"`
	CLS Statistic `json:"cls"`
}
type Group struct {
	Viewport string  `json:"viewport"`
	Metrics  Metrics `json:"metrics"`
}
type Window struct {
	StartDate string `json:"startDate"`
	EndDate   string `json:"endDate"`
}
type Report struct {
	WebsiteID       string  `json:"websiteId"`
	Site            string  `json:"site"`
	WebsiteRevision int64   `json:"websiteRevision"`
	Source          string  `json:"source"`
	Environment     string  `json:"environment"`
	Status          string  `json:"status"`
	ReportingOrigin string  `json:"reportingOrigin,omitempty"`
	Window          Window  `json:"window"`
	AsOf            string  `json:"asOf"`
	LastCollectedAt string  `json:"lastCollectedAt,omitempty"`
	RetentionDays   int     `json:"retentionDays"`
	SampleCount     int     `json:"sampleCount"`
	Groups          []Group `json:"groups"`
}
type diskState struct {
	Version        int      `json:"version"`
	OrganizationID string   `json:"organizationId"`
	Entries        []Export `json:"entries"`
}

func stamp(t time.Time) string { return t.UTC().Format(time.RFC3339Nano) }
func windowStart(t time.Time) time.Time {
	u := t.UTC()
	return time.Date(u.Year(), u.Month(), u.Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -6)
}
func environment(s string) bool { return s == "production" || s == "development" }
