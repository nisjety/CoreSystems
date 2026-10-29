// Package sitesources owns validated, read-only provider aggregate snapshots.
// Provider credentials and direct provider calls belong to Integration Core.
package sitesources

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const (
	maxEntries      = 64
	maxBytes        = 8 << 20
	MaxReceiptBytes = 128 << 10
	Freshness       = 6 * time.Hour
)

var (
	ErrInvalid     = errors.New("invalid site source request")
	ErrForbidden   = errors.New("verified website ownership required")
	ErrUnavailable = errors.New("site source unavailable")
	ErrCooldown    = errors.New("site source refresh busy")
	ErrCapacity    = errors.New("site source snapshot capacity exceeded")
	idPattern      = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	orgPattern     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
	indexPattern   = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,128}$`)
)

type Registry interface {
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type Collector interface {
	Collect(context.Context, websites.Record, string) (Report, error)
}

// Authorization is private coherence metadata, never part of the public Report.
type Authorization struct {
	Managed, Active                                             bool
	Fingerprint, Generation, SnapshotObservedAt, SnapshotDigest string
}
type OAuthAuthority interface {
	Authorize(context.Context, websites.Record) (Authorization, error)
	CollectOAuth(context.Context, websites.Record, Authorization) (Report, error)
	BindSnapshot(websites.Record, Authorization, Report) error
}
type Window struct {
	StartDate string `json:"startDate"`
	EndDate   string `json:"endDate"`
}
type Report struct {
	WebsiteID       string     `json:"websiteId"`
	Site            string     `json:"site"`
	WebsiteRevision int64      `json:"websiteRevision"`
	Provider        string     `json:"provider"`
	ObservedAt      string     `json:"observedAt"`
	Window          Window     `json:"window"`
	Status          string     `json:"status"`
	Cookiebot       *Cookiebot `json:"cookiebot,omitempty"`
	Algolia         *Algolia   `json:"algolia,omitempty"`
}
type Cookiebot struct {
	ReportingDomain string    `json:"reportingDomain"`
	Consents        Consents  `json:"consents"`
	Inventory       Inventory `json:"inventory"`
}
type Consents struct {
	Status       string         `json:"status"`
	ReportedDays int            `json:"reportedDays"`
	Totals       *ConsentTotals `json:"totals,omitempty"`
}
type ConsentTotals struct {
	OptIn       int64 `json:"optIn"`
	OptOut      int64 `json:"optOut"`
	Preferences int64 `json:"preferences"`
	Statistics  int64 `json:"statistics"`
	Marketing   int64 `json:"marketing"`
}
type Inventory struct {
	Status     string      `json:"status"`
	ScannedAt  string      `json:"scannedAt,omitempty"`
	Total      *int64      `json:"total,omitempty"`
	Categories *Categories `json:"categories,omitempty"`
	ThirdParty *int64      `json:"thirdParty,omitempty"`
	Secure     *int64      `json:"secure,omitempty"`
	HTTPOnly   *int64      `json:"httpOnly,omitempty"`
}
type Categories struct {
	Necessary    int64 `json:"necessary"`
	Preferences  int64 `json:"preferences"`
	Statistics   int64 `json:"statistics"`
	Marketing    int64 `json:"marketing"`
	Unclassified int64 `json:"unclassified"`
}
type Algolia struct {
	IndexName     string    `json:"indexName"`
	Region        string    `json:"region"`
	Analytics     Analytics `json:"analytics"`
	TopSearches   *Terms    `json:"topSearches,omitempty"`
	NoResultTerms *Terms    `json:"noResultTerms,omitempty"`
}
type Terms struct {
	Status string `json:"status"`
	Limit  int    `json:"limit"`
	Rows   []Term `json:"rows,omitempty"`
}
type Term struct {
	Query          string   `json:"query"`
	Count          int64    `json:"count"`
	AverageResults *float64 `json:"averageResults,omitempty"`
}

func (t Terms) MarshalJSON() ([]byte, error) {
	if t.Status == "connected" {
		return json.Marshal(struct {
			Status string `json:"status"`
			Limit  int    `json:"limit"`
			Rows   []Term `json:"rows"`
		}{t.Status, t.Limit, t.Rows})
	}
	return json.Marshal(struct {
		Status string `json:"status"`
		Limit  int    `json:"limit"`
	}{t.Status, t.Limit})
}

type Analytics struct {
	Status           string   `json:"status"`
	ReportedDays     int      `json:"reportedDays"`
	Searches         *int64   `json:"searches,omitempty"`
	NoResultSearches *int64   `json:"noResultSearches,omitempty"`
	NoResultRate     *float64 `json:"noResultRate,omitempty"`
}

// An observed zero-search window has an explicitly null rate, while an
// unavailable component has no metrics at all. These are not interchangeable.
func (a Analytics) MarshalJSON() ([]byte, error) {
	type fields Analytics
	if a.Status != "connected" {
		return json.Marshal(fields(a))
	}
	return json.Marshal(struct {
		Status           string   `json:"status"`
		ReportedDays     int      `json:"reportedDays"`
		Searches         *int64   `json:"searches"`
		NoResultSearches *int64   `json:"noResultSearches"`
		NoResultRate     *float64 `json:"noResultRate"`
	}{a.Status, a.ReportedDays, a.Searches, a.NoResultSearches, a.NoResultRate})
}

type diskState struct {
	Version        int      `json:"version"`
	OrganizationID string   `json:"organizationId"`
	Entries        []Report `json:"entries"`
}

func provider(p string) bool     { return p == "cookiebot" || p == "algolia" }
func stamp(now time.Time) string { return now.UTC().Format(time.RFC3339Nano) }
func period(now time.Time) Window {
	u := now.UTC()
	day := time.Date(u.Year(), u.Month(), u.Day(), 0, 0, 0, 0, time.UTC)
	return Window{day.AddDate(0, 0, -7).Format("2006-01-02"), day.AddDate(0, 0, -1).Format("2006-01-02")}
}
func clone(r Report) Report {
	raw, _ := json.Marshal(r)
	var next Report
	_ = json.Unmarshal(raw, &next)
	return next
}
func Fingerprint(r Report) string {
	raw, _ := json.Marshal(r)
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
