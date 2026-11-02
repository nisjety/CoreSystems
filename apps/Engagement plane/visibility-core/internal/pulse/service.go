// Package pulse projects a verified Search Console property into dashboard metrics.
// It never exposes provider tokens, connection IDs, or raw provider responses.
package pulse

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	StatusConnected   = "connected"
	StatusUnavailable = "unavailable"

	ReasonNotConfigured      = "not_configured"
	ReasonPropertyUnverified = "property_unverified"
	ReasonSourceUnavailable  = "source_unavailable"

	dateLayout = "2006-01-02"
)

type Config struct {
	IntegrationCoreURL         string
	GoogleConsentReturnURL     string
	GoogleOAuthCallbackURL     string
	GoogleConnectionID         string
	GoogleGSCProperty          string
	IntegrationBearerTokenFile string
	GoogleGA4ConnectionID      string
	GoogleGA4PropertyID        string
	GoogleGA4StreamID          string
	GoogleGA4ReportingSite     string
	ClarityProjectID           string
}

type Period struct {
	StartDate string `json:"startDate"`
	EndDate   string `json:"endDate"`
}

type Totals struct {
	Clicks      int64 `json:"clicks"`
	Impressions int64 `json:"impressions"`
}

type Day struct {
	Date        string `json:"date"`
	Clicks      int64  `json:"clicks"`
	Impressions int64  `json:"impressions"`
}

type Projection struct {
	Site              string  `json:"site"`
	Source            string  `json:"source"`
	Status            string  `json:"status"`
	Period            Period  `json:"period"`
	Totals            *Totals `json:"totals"`
	Days              []Day   `json:"days"`
	PropertyScope     string  `json:"propertyScope,omitempty"`
	ReportingProperty string  `json:"reportingProperty,omitempty"`
	UpdatedAt         string  `json:"updatedAt,omitempty"`
	Reason            string  `json:"reason,omitempty"`
}

type Service struct {
	cfg    Config
	client *http.Client
	now    func() time.Time
}

func NewService(cfg Config, client *http.Client, now func() time.Time) *Service {
	if client == nil {
		client = &http.Client{
			Timeout: 12 * time.Second,
			// Never forward an org-scoped bearer token to a redirected host.
			CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
		}
	}
	if now == nil {
		now = time.Now
	}
	return &Service{cfg: cfg, client: client, now: now}
}

// QuarterPeriod bounds a selected current-year quarter or the current full year
// to yesterday, because Search Console's current day is not yet complete.
// Future quarters are invalid.
func QuarterPeriod(quarter string, now time.Time) (Period, error) {
	quarter = strings.ToUpper(strings.TrimSpace(quarter))
	if quarter == "" {
		quarter = "Q" + strconv.Itoa((int(now.UTC().Month())-1)/3+1)
	}
	if quarter == "FY" {
		utcNow := now.UTC()
		start := time.Date(utcNow.Year(), time.January, 1, 0, 0, 0, 0, time.UTC)
		end := time.Date(utcNow.Year(), utcNow.Month(), utcNow.Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -1)
		if end.Before(start) {
			return Period{}, errors.New("full year has not started")
		}
		return Period{StartDate: start.Format("2006-01-02"), EndDate: end.Format("2006-01-02")}, nil
	}
	if len(quarter) != 2 || quarter[0] != 'Q' || quarter[1] < '1' || quarter[1] > '4' {
		return Period{}, errors.New("quarter must be Q1, Q2, Q3, Q4, or FY")
	}
	index := int(quarter[1] - '1')
	start := time.Date(now.UTC().Year(), time.Month(index*3+1), 1, 0, 0, 0, 0, time.UTC)
	end := start.AddDate(0, 3, 0).AddDate(0, 0, -1)
	yesterday := time.Date(now.UTC().Year(), now.UTC().Month(), now.UTC().Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -1)
	if start.After(yesterday) {
		return Period{}, errors.New("quarter has not started")
	}
	if end.After(yesterday) {
		end = yesterday
	}
	return Period{StartDate: start.Format(dateLayout), EndDate: end.Format(dateLayout)}, nil
}

func (s *Service) Get(ctx context.Context, quarter, requestedSite string) Projection {
	now := s.now().UTC()
	site, siteErr := CanonicalSite(requestedSite)
	period, err := QuarterPeriod(quarter, now)
	if err != nil {
		// The HTTP boundary validates the quarter before calling Get.
		return Projection{Site: site, Source: "google_search_console", Status: StatusUnavailable, Reason: ReasonSourceUnavailable, Days: []Day{}}
	}
	base := Projection{Site: site, Source: "google_search_console", Status: StatusUnavailable, Period: period, Days: []Day{}}
	if siteErr != nil {
		base.Reason = ReasonPropertyUnverified
		return base
	}
	if !s.cfg.complete() {
		base.Reason = ReasonNotConfigured
		return base
	}
	if err := validateConfig(s.cfg); err != nil {
		base.Reason = ReasonNotConfigured
		return base
	}
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	property, err := s.verifiedProperty(ctx, token, site)
	if err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	if property == "" {
		base.Reason = ReasonPropertyUnverified
		return base
	}
	days, totals, err := s.readDays(ctx, token, period, property)
	if err != nil {
		base.Reason = ReasonSourceUnavailable
		return base
	}
	base.Status = StatusConnected
	base.ReportingProperty = property
	base.Reason = ""
	if strings.HasPrefix(property, "sc-domain:") {
		base.PropertyScope = "domain"
	} else {
		base.PropertyScope = "url_prefix"
	}
	base.Days = days
	base.Totals = &totals
	base.UpdatedAt = now.Format(time.RFC3339)
	return base
}

func (c Config) complete() bool {
	return strings.TrimSpace(c.IntegrationCoreURL) != "" && strings.TrimSpace(c.GoogleConnectionID) != "" &&
		strings.TrimSpace(c.IntegrationBearerTokenFile) != ""
}

// ValidateConfig allows an org-authorized Integration Core broker for public
// diagnostics. Private Search Console reads still require their full grant.
func ValidateConfig(cfg Config) error {
	if cfg.GoogleConnectionID == "" && cfg.GoogleGSCProperty == "" {
		return validateIntegrationURL(cfg.IntegrationCoreURL)
	}
	return validateConfig(cfg)
}

func validateConfig(cfg Config) error {
	if !cfg.complete() {
		return errors.New("pulse integration config is incomplete")
	}
	if err := validateIntegrationURL(cfg.IntegrationCoreURL); err != nil {
		return err
	}
	if strings.ContainsAny(cfg.GoogleConnectionID, "/?#% \\\r\n") || len(cfg.GoogleConnectionID) > 128 {
		return errors.New("GOOGLE_CONNECTION_ID is invalid")
	}
	if cfg.GoogleGSCProperty != "" {
		if strings.HasPrefix(cfg.GoogleGSCProperty, "sc-domain:") {
			domain := strings.TrimPrefix(cfg.GoogleGSCProperty, "sc-domain:")
			if !validPublicHostname(domain) || domain != strings.ToLower(domain) {
				return errors.New("GOOGLE_GSC_PROPERTY must be a public HTTPS root or domain property")
			}
		} else if site, err := CanonicalSite(cfg.GoogleGSCProperty); err != nil || site != cfg.GoogleGSCProperty {
			return errors.New("GOOGLE_GSC_PROPERTY must be a public HTTPS root or domain property")
		}
	}
	return nil
}

func validateIntegrationURL(destination string) error {
	parsed, err := url.Parse(destination)
	if err != nil || parsed.Host == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || parsed.Path != "" {
		return errors.New("INTEGRATION_CORE_URL is invalid")
	}
	if parsed.Scheme != "https" && !(parsed.Scheme == "http" && (parsed.Hostname() == "localhost" || parsed.Hostname() == "127.0.0.1" || parsed.Hostname() == "integration-core" || parsed.Hostname() == "integration-api")) {
		return errors.New("INTEGRATION_CORE_URL must be HTTPS or approved local HTTP")
	}
	return nil
}

func readBearerToken(path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, 8193))
	if err != nil || len(data) > 8192 {
		return "", errors.New("bearer token file is unreadable or too large")
	}
	token := strings.TrimSpace(string(data))
	if token == "" || strings.ContainsAny(token, "\r\n \t") {
		return "", errors.New("bearer token file is empty or invalid")
	}
	return token, nil
}

type actionEnvelope struct {
	Success bool `json:"success"`
	Data    struct {
		Action struct {
			ProviderKey string          `json:"providerKey"`
			Operation   string          `json:"operation"`
			Result      json.RawMessage `json:"result"`
		} `json:"action"`
	} `json:"data"`
}

func (s *Service) callAction(ctx context.Context, token, operation string, params, body any) (json.RawMessage, error) {
	payload, err := json.Marshal(map[string]any{"operation": operation, "params": params, "body": body})
	if err != nil {
		return nil, err
	}
	endpoint := strings.TrimRight(s.cfg.IntegrationCoreURL, "/") + "/api/v1/connections/" + url.PathEscape(s.cfg.GoogleConnectionID) + "/actions"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(payload))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	resp, err := s.client.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("integration action status %d", resp.StatusCode)
	}
	var envelope actionEnvelope
	limited := io.LimitReader(resp.Body, (2<<20)+1)
	responseBody, err := io.ReadAll(limited)
	if err != nil || len(responseBody) > 2<<20 {
		return nil, errors.New("integration action response too large")
	}
	if err := json.Unmarshal(responseBody, &envelope); err != nil || !envelope.Success || envelope.Data.Action.ProviderKey != "google" || envelope.Data.Action.Operation != operation || len(envelope.Data.Action.Result) == 0 {
		return nil, errors.New("invalid integration action response")
	}
	return envelope.Data.Action.Result, nil
}

func (s *Service) verifiedProperty(ctx context.Context, token, site string) (string, error) {
	result, err := s.callAction(ctx, token, "gsc.sites.list", nil, nil)
	if err != nil {
		return "", err
	}
	var sites struct {
		SiteEntry []struct {
			SiteURL         string `json:"siteUrl"`
			PermissionLevel string `json:"permissionLevel"`
		} `json:"siteEntry"`
	}
	if err := json.Unmarshal(result, &sites); err != nil {
		return "", err
	}
	bestProperty := ""
	bestScore := -1
	for _, entry := range sites.SiteEntry {
		if entry.PermissionLevel != "siteOwner" && entry.PermissionLevel != "siteFullUser" && entry.PermissionLevel != "siteRestrictedUser" {
			continue
		}
		score, matches := propertyMatchesSite(entry.SiteURL, site)
		if !matches {
			continue
		}
		if entry.SiteURL == s.cfg.GoogleGSCProperty {
			score += 20_000
		}
		if score > bestScore {
			bestScore = score
			bestProperty = entry.SiteURL
		}
	}
	return bestProperty, nil
}

func periodDayLimit(period Period) (int, error) {
	start, startErr := time.Parse(dateLayout, period.StartDate)
	end, endErr := time.Parse(dateLayout, period.EndDate)
	if startErr != nil || endErr != nil || end.Before(start) {
		return 0, errors.New("invalid analytics period")
	}
	limit := int(end.Sub(start).Hours()/24) + 1
	if limit < 1 || limit > 366 {
		return 0, errors.New("invalid analytics period")
	}
	return limit, nil
}

func (s *Service) readDays(ctx context.Context, token string, period Period, property string) ([]Day, Totals, error) {
	rowLimit, err := periodDayLimit(period)
	if err != nil {
		return nil, Totals{}, err
	}
	result, err := s.callAction(ctx, token, "gsc.search_analytics.query", map[string]any{"siteUrl": property}, map[string]any{
		"startDate": period.StartDate, "endDate": period.EndDate, "dimensions": []string{"date"}, "rowLimit": rowLimit, "dataState": "final",
	})
	if err != nil {
		return nil, Totals{}, err
	}
	var response struct {
		Rows []struct {
			Keys        []string    `json:"keys"`
			Clicks      json.Number `json:"clicks"`
			Impressions json.Number `json:"impressions"`
		} `json:"rows"`
	}
	decoder := json.NewDecoder(bytes.NewReader(result))
	decoder.UseNumber()
	if err := decoder.Decode(&response); err != nil {
		return nil, Totals{}, err
	}
	if len(response.Rows) > rowLimit {
		return nil, Totals{}, errors.New("too many date rows")
	}
	days := make([]Day, 0, len(response.Rows))
	var totals Totals
	seen := make(map[string]bool, len(response.Rows))
	for _, row := range response.Rows {
		if len(row.Keys) != 1 || seen[row.Keys[0]] || row.Keys[0] < period.StartDate || row.Keys[0] > period.EndDate {
			return nil, Totals{}, errors.New("invalid date row")
		}
		date, err := time.Parse(dateLayout, row.Keys[0])
		if err != nil || date.Format(dateLayout) != row.Keys[0] {
			return nil, Totals{}, errors.New("invalid date row")
		}
		clicks, err := wholeNonnegative(row.Clicks)
		if err != nil {
			return nil, Totals{}, err
		}
		impressions, err := wholeNonnegative(row.Impressions)
		if err != nil || clicks > impressions || totals.Clicks > math.MaxInt64-clicks || totals.Impressions > math.MaxInt64-impressions {
			return nil, Totals{}, errors.New("invalid analytics metric")
		}
		seen[row.Keys[0]] = true
		days = append(days, Day{Date: row.Keys[0], Clicks: clicks, Impressions: impressions})
		totals.Clicks += clicks
		totals.Impressions += impressions
	}
	sort.Slice(days, func(i, j int) bool { return days[i].Date < days[j].Date })
	return days, totals, nil
}

func wholeNonnegative(value json.Number) (int64, error) {
	parsed, err := value.Int64()
	if err != nil || parsed < 0 {
		return 0, errors.New("invalid analytics metric")
	}
	return parsed, nil
}
