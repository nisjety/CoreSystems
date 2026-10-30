// Package analyticssync owns bounded, durable normalized private analytics.
// Dashboard reads never collect provider metrics; Integration remains authority.
package analyticssync

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

var (
	ErrInvalid         = errors.New("invalid analytics sync request")
	ErrConflict        = errors.New("analytics website context changed")
	ErrCapacity        = errors.New("analytics sync capacity exhausted")
	ErrCooldown        = errors.New("analytics sync cooldown active")
	idPattern          = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	jobPattern         = regexp.MustCompile(`^sync_[a-f0-9]{32}$`)
	orgPattern         = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	fingerprintPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)
)

const (
	maxEntries   = 64
	maxBytes     = 8 << 20
	metricsTTL   = 6 * time.Hour
	retentionTTL = 24 * time.Hour
	retryDelay   = time.Hour
	jobTimeout   = 90 * time.Second
)

var stepKeys = [...]string{"gsc", "ga4", "clarity"}

type Bundle struct {
	Pulse   pulse.Projection        `json:"pulse"`
	Search  pulse.SearchProjection  `json:"search"`
	GA4     pulse.GA4Projection     `json:"ga4"`
	Clarity pulse.ClarityProjection `json:"clarity"`
}
type Access struct {
	SearchConsole, GA4, Clarity      bool
	ClarityProjectID, ClarityVersion string
}
type Step struct {
	Key           string `json:"key"`
	Status        string `json:"status"`
	Reason        string `json:"reason,omitempty"`
	ObservedAt    string `json:"observedAt,omitempty"`
	LastSuccessAt string `json:"lastSuccessAt,omitempty"`
}
type State struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Source          string `json:"source"`
	Quarter         string `json:"quarter"`
	Enabled         bool   `json:"enabled"`
	Status          string `json:"status"`
	JobID           string `json:"jobId,omitempty"`
	QueuedAt        string `json:"queuedAt,omitempty"`
	StartedAt       string `json:"startedAt,omitempty"`
	CompletedAt     string `json:"completedAt,omitempty"`
	NextSyncAt      string `json:"nextSyncAt,omitempty"`
	ExpiresAt       string `json:"expiresAt,omitempty"`
	Steps           []Step `json:"steps"`
}
type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type Runner interface {
	Collect(context.Context, websites.Record, string) (Bundle, error)
}

// Authority must perform fresh Integration metadata checks, not provider reads.
type Authority interface {
	Check(context.Context, websites.Record) Access
}
type entry struct {
	State
	Fingerprint string  `json:"fingerprint"`
	Year        int     `json:"year"`
	Attempts    int     `json:"attempts"`
	Bundle      *Bundle `json:"bundle,omitempty"`
}
type diskState struct {
	Version        int     `json:"version"`
	OrganizationID string  `json:"organizationId"`
	Entries        []entry `json:"entries"`
}

func stamp(t time.Time) string  { return t.UTC().Format(time.RFC3339Nano) }
func parsed(s string) time.Time { v, _ := time.Parse(time.RFC3339Nano, s); return v }
func newID() string {
	var v [16]byte
	_, _ = rand.Read(v[:])
	return "sync_" + hex.EncodeToString(v[:])
}
func fingerprint(r websites.Record) string {
	v := struct {
		Site         string
		Revision     int64
		GSC, GA4     string
		Integrations websites.Integrations
		Ownership    websites.Ownership
	}{r.Website.Site, r.Website.Revision, r.GSCConnectionID, r.GA4ConnectionID, r.Website.Integrations, r.Website.Ownership}
	raw, _ := json.Marshal(v)
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
func clone[T any](v T) T {
	raw, err := json.Marshal(v)
	if err != nil {
		panic("analytics clone violated validated JSON invariant")
	}
	var result T
	if json.Unmarshal(raw, &result) != nil {
		panic("analytics clone violated typed JSON invariant")
	}
	return result
}
func empty(w websites.Website, q string) State {
	steps := make([]Step, 3)
	for i, key := range stepKeys {
		steps[i] = Step{Key: key, Status: "queued"}
	}
	return State{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "marketing_analytics_sync", Quarter: q, Status: "idle", Steps: steps}
}
func reason(v string) bool {
	switch v {
	case "not_configured", "not_connected", "requires_consent", "needs_selection", "not_found", "source_unavailable", "property_unverified", "pending_verification", "quota_limited", "authorization_required", "no_data", "snapshot_pending", "snapshot_expired", "interrupted", "context_changed", "timeout":
		return true
	}
	return false
}
func collectionReason(v string) bool {
	return reason(v) && v != "snapshot_pending" && v != "snapshot_expired"
}

// MissingBundle contains null metrics and empty observations, never fabricated zeros.
func MissingBundle(site, q string, now time.Time, why string) Bundle {
	if !reason(why) {
		why = "source_unavailable"
	}
	period, _ := pulse.QuarterPeriod(q, now)
	return Bundle{Pulse: pulse.Projection{Site: site, Source: "google_search_console", Status: "unavailable", Reason: why, Period: period, Days: []pulse.Day{}}, Search: pulse.SearchProjection{Site: site, Source: "google_search_console", Status: "unavailable", Reason: why, Period: period, Queries: []pulse.SearchRow{}, Pages: []pulse.SearchRow{}, Opportunities: []pulse.Opportunity{}, RowLimit: pulse.SearchRowLimit}, GA4: pulse.UnavailableGA4(site, period, why), Clarity: pulse.UnavailableClarity(site, why)}
}
func mask(b Bundle, a Access, site, q string, now time.Time, why string) Bundle {
	missing := MissingBundle(site, q, now, why)
	if !a.SearchConsole {
		b.Pulse, b.Search = missing.Pulse, missing.Search
	}
	if !a.GA4 {
		b.GA4 = missing.GA4
	}
	if !a.Clarity || a.ClarityProjectID == "" || a.ClarityVersion == "" || b.Clarity.ProjectID != a.ClarityProjectID || b.Clarity.BindingVersion != a.ClarityVersion {
		b.Clarity = missing.Clarity
	}
	return b
}
