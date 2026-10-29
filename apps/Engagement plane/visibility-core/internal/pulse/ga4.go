package pulse

import (
	"context"
	"encoding/json"
	"regexp"
	"strings"
	"time"
	_ "time/tzdata" // Minimal Alpine runtime has no system IANA timezone database.
)

type GA4Metrics struct {
	Users             int64   `json:"users"`
	Sessions          int64   `json:"sessions"`
	EngagedSessions   int64   `json:"engagedSessions"`
	PageViews         int64   `json:"pageViews"`
	KeyEvents         float64 `json:"keyEvents"`
	EngagementSeconds float64 `json:"engagementSeconds"`
}
type GA4Day struct {
	Date string `json:"date"`
	GA4Metrics
}
type GA4Quality struct {
	Thresholded bool `json:"thresholded"`
	Sampled     bool `json:"sampled"`
	DataLoss    bool `json:"dataLoss"`
}
type GA4Projection struct {
	Site          string      `json:"site"`
	ReportingSite string      `json:"reportingSite,omitempty"`
	Source        string      `json:"source"`
	Status        string      `json:"status"`
	Reason        string      `json:"reason,omitempty"`
	Period        Period      `json:"period"`
	Totals        *GA4Metrics `json:"totals"`
	Days          []GA4Day    `json:"days"`
	TimeZone      string      `json:"timeZone,omitempty"`
	UpdatedAt     string      `json:"updatedAt,omitempty"`
	Quality       *GA4Quality `json:"quality,omitempty"`
}

var ga4NumericID = regexp.MustCompile(`^[1-9][0-9]{0,29}$`)
var ga4ConnectionID = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$`)

func UnavailableGA4(site string, period Period, reason string) GA4Projection {
	return GA4Projection{Site: site, Source: "google_analytics", Status: StatusUnavailable, Reason: reason, Period: period, Days: []GA4Day{}}
}

// GetGA4 sends only the fixed, server-mapped reporting operation. The provider
// applies exact host/stream filters; requested dimensions/metrics are not caller input.
func (s *Service) GetGA4(ctx context.Context, quarter, requestedSite string) GA4Projection {
	period, periodErr := QuarterPeriod(quarter, s.now())
	site, siteErr := CanonicalSite(requestedSite)
	failure := func(reason string) GA4Projection { return UnavailableGA4(site, period, reason) }
	if periodErr != nil || siteErr != nil {
		return failure(ReasonSourceUnavailable)
	}
	if validateIntegrationURL(s.cfg.IntegrationCoreURL) != nil || s.cfg.IntegrationBearerTokenFile == "" || !ga4ConnectionID.MatchString(s.cfg.GoogleGA4ConnectionID) || !ga4NumericID.MatchString(s.cfg.GoogleGA4PropertyID) || !ga4NumericID.MatchString(s.cfg.GoogleGA4StreamID) {
		return failure(ReasonNotConfigured)
	}
	reportingSite := site
	if s.cfg.GoogleGA4ReportingSite != "" {
		reportingSite = s.cfg.GoogleGA4ReportingSite
		if !GA4ReportingSiteAllowed(site, reportingSite) {
			return failure(ReasonNotConfigured)
		}
	}
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		return failure(ReasonSourceUnavailable)
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	cfg := s.cfg
	cfg.GoogleConnectionID = cfg.GoogleGA4ConnectionID
	reader := NewService(cfg, s.client, s.now)
	params := map[string]any{"propertyId": cfg.GoogleGA4PropertyID, "streamId": cfg.GoogleGA4StreamID, "site": site}
	if reportingSite != site {
		params["reportingSite"] = reportingSite
	}
	result, err := reader.callAction(ctx, token, "ga4.report.run", params, map[string]any{"startDate": period.StartDate, "endDate": period.EndDate})
	if err != nil {
		return failure(ReasonSourceUnavailable)
	}
	var reports struct {
		Summary ga4Report `json:"summary"`
		Daily   ga4Report `json:"daily"`
	}
	if json.Unmarshal(result, &reports) != nil {
		return failure(ReasonSourceUnavailable)
	}
	totals, days, zone, quality, err := normalizeGA4Reports(reports.Summary, reports.Daily, period)
	if err != nil {
		return failure(ReasonSourceUnavailable)
	}
	projection := GA4Projection{Site: site, Source: "google_analytics", Status: StatusConnected, Period: period, Totals: totals, Days: days, TimeZone: zone, UpdatedAt: s.now().UTC().Format(time.RFC3339), Quality: quality}
	if reportingSite != site {
		projection.ReportingSite = reportingSite
	}
	return projection
}

func validGA4Zone(value string) bool {
	if value == "" || len(value) > 64 || value == "Local" || strings.ContainsAny(value, "\x00\r\n") {
		return false
	}
	_, err := time.LoadLocation(value)
	return err == nil
}
