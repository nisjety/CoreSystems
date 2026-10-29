// Package sitesources owns fixed-endpoint, read-only aggregate site reports.
// API keys and raw consent/cookie/hit records never leave this boundary.
// Explicitly authorized Algolia terms use a separate bounded, typed projection.
package sitesources

import (
	"encoding/json"
	"errors"
	"regexp"
	"time"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
	"github.com/triodelab/integration-corev2/internal/webvitals"
)

var (
	ErrInvalid     = errors.New("invalid site sources input")
	ErrUnavailable = errors.New("site sources unavailable")
	ErrBusy        = errors.New("site sources capacity busy")
	websitePattern = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
	orgPattern     = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)
)

const safeMax int64 = 9007199254740991

type Input struct {
	WebsiteID       string `json:"websiteId"`
	Site            string `json:"site"`
	WebsiteRevision int64  `json:"websiteRevision"`
	Provider        string `json:"provider"`
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
type Totals struct {
	OptIn       int64 `json:"optIn"`
	OptOut      int64 `json:"optOut"`
	Preferences int64 `json:"preferences"`
	Statistics  int64 `json:"statistics"`
	Marketing   int64 `json:"marketing"`
}
type Consents struct {
	Status       string  `json:"status"`
	ReportedDays int     `json:"reportedDays"`
	Totals       *Totals `json:"totals,omitempty"`
}
type Categories struct {
	Necessary    int64 `json:"necessary"`
	Preferences  int64 `json:"preferences"`
	Statistics   int64 `json:"statistics"`
	Marketing    int64 `json:"marketing"`
	Unclassified int64 `json:"unclassified"`
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
type Cookiebot struct {
	ReportingDomain string    `json:"reportingDomain"`
	Consents        Consents  `json:"consents"`
	Inventory       Inventory `json:"inventory"`
}
type Analytics struct {
	Status           string `json:"status"`
	ReportedDays     int    `json:"reportedDays"`
	Searches         *int64 `json:"searches,omitempty"`
	NoResultSearches *int64 `json:"noResultSearches,omitempty"`
	// Outer nil means absent metrics; inner nil means observed zero searches.
	NoResultRate **float64 `json:"noResultRate,omitempty"`
}
type Algolia struct {
	IndexName     string       `json:"indexName"`
	Region        string       `json:"region"`
	Analytics     Analytics    `json:"analytics"`
	TopSearches   *SearchTerms `json:"topSearches,omitempty"`
	NoResultTerms *SearchTerms `json:"noResultTerms,omitempty"`
}

type SearchTermRow struct {
	Query          string   `json:"query"`
	Count          int64    `json:"count"`
	AverageResults *float64 `json:"averageResults,omitempty"`
}
type SearchTerms struct {
	Status string           `json:"status"`
	Limit  int              `json:"limit"`
	Rows   *[]SearchTermRow `json:"rows,omitempty"`
}

func period(now time.Time) Window {
	utc := now.UTC()
	end := time.Date(utc.Year(), utc.Month(), utc.Day()-1, 0, 0, 0, 0, time.UTC)
	return Window{end.AddDate(0, 0, -6).Format(time.DateOnly), end.Format(time.DateOnly)}
}
func validInput(in Input) bool {
	return websitePattern.MatchString(in.WebsiteID) && in.WebsiteRevision > 0 && in.WebsiteRevision <= safeMax && (in.Provider == "cookiebot" || in.Provider == "algolia") && pagespeed.ValidateInput(in.Site, "mobile") == nil
}
func exactKeys(raw []byte, required, optional []string) bool {
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return false
	}
	allowed := map[string]bool{}
	for _, name := range required {
		if _, ok := fields[name]; !ok {
			return false
		}
		allowed[name] = true
	}
	for _, name := range optional {
		allowed[name] = true
	}
	for name := range fields {
		if !allowed[name] {
			return false
		}
	}
	return true
}
func ParseInput(raw []byte) (Input, error) {
	var in Input
	if len(raw) > 2048 || webvitals.UniqueJSON(raw) != nil || !exactKeys(raw, []string{"websiteId", "site", "websiteRevision", "provider"}, nil) || json.Unmarshal(raw, &in) != nil || !validInput(in) {
		return Input{}, ErrInvalid
	}
	return in, nil
}
