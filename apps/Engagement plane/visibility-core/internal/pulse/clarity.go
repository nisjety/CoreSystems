package pulse

import (
	"context"
	"encoding/json"
	"io"
	"math"
	"net/http"
	"net/url"
	"regexp"
	"time"
)

type ClarityMetrics struct {
	Sessions        *int64            `json:"sessions"`
	BotSessions     *int64            `json:"botSessions"`
	Users           *int64            `json:"users"`
	PagesPerSession *float64          `json:"pagesPerSession"`
	UX              *ClarityUXMetrics `json:"ux,omitempty"`
}

type ClarityUXMetrics struct {
	DeadClickPercent       *float64 `json:"deadClickPercent"`
	RageClickPercent       *float64 `json:"rageClickPercent"`
	ExcessiveScrollPercent *float64 `json:"excessiveScrollPercent"`
	QuickBackPercent       *float64 `json:"quickBackPercent"`
	ScriptErrorPercent     *float64 `json:"scriptErrorPercent"`
	ErrorClickPercent      *float64 `json:"errorClickPercent"`
	AverageScrollDepth     *float64 `json:"averageScrollDepth"`
	ActiveSeconds          *float64 `json:"activeSeconds"`
	TotalSeconds           *float64 `json:"totalSeconds"`
}

func ValidClarityUX(ux *ClarityUXMetrics) bool {
	if ux == nil {
		return true
	}
	for _, value := range []*float64{ux.DeadClickPercent, ux.RageClickPercent, ux.ExcessiveScrollPercent, ux.QuickBackPercent, ux.ScriptErrorPercent, ux.ErrorClickPercent, ux.AverageScrollDepth, ux.ActiveSeconds, ux.TotalSeconds} {
		if value != nil && (math.IsNaN(*value) || math.IsInf(*value, 0) || *value < 0 || *value > maxSafeInteger) {
			return false
		}
	}
	for _, value := range []*float64{ux.DeadClickPercent, ux.RageClickPercent, ux.ExcessiveScrollPercent, ux.QuickBackPercent, ux.ScriptErrorPercent, ux.ErrorClickPercent, ux.AverageScrollDepth} {
		if value != nil && *value > 100 {
			return false
		}
	}
	return ux.ActiveSeconds == nil || ux.TotalSeconds == nil || *ux.ActiveSeconds <= *ux.TotalSeconds
}

type ClarityWindow struct {
	NumOfDays  int    `json:"numOfDays"`
	TimeZone   string `json:"timeZone"`
	ObservedAt string `json:"observedAt,omitempty"`
}
type ClarityProjection struct {
	Site           string          `json:"site"`
	Source         string          `json:"source"`
	Status         string          `json:"status"`
	Scope          string          `json:"scope"`
	ProjectID      string          `json:"projectId,omitempty"`
	BindingVersion string          `json:"bindingVersion,omitempty"`
	Window         ClarityWindow   `json:"window"`
	Metrics        *ClarityMetrics `json:"metrics"`
	UpdatedAt      string          `json:"updatedAt,omitempty"`
	Reason         string          `json:"reason,omitempty"`
}

var clarityProjectPattern = regexp.MustCompile(`^[a-z0-9]{4,32}$`)
var clarityBindingVersionPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)

func UnavailableClarity(site, reason string) ClarityProjection {
	return ClarityProjection{Site: site, Source: "microsoft_clarity", Status: StatusUnavailable, Scope: "clarity_project", Window: ClarityWindow{NumOfDays: 3, TimeZone: "UTC"}, Reason: reason}
}

// GetClarity reads a provider-bound project projection, never a caller token or
// raw browsing/session data. Its rolling UTC window is independent of quarters.
func (s *Service) GetClarity(ctx context.Context, site string) ClarityProjection {
	failure := func(reason string) ClarityProjection { return UnavailableClarity(site, reason) }
	canonical, err := CanonicalSite(site)
	if err != nil || canonical != site {
		return failure(ReasonSourceUnavailable)
	}
	if validateIntegrationURL(s.cfg.IntegrationCoreURL) != nil || s.cfg.IntegrationBearerTokenFile == "" {
		return failure(ReasonNotConfigured)
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		return failure(ReasonSourceUnavailable)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.cfg.IntegrationCoreURL+"/api/v1/insights/clarity?site="+url.QueryEscape(site)+"&numOfDays=3", nil)
	if err != nil {
		return failure(ReasonSourceUnavailable)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	response, err := s.client.Do(req)
	if err != nil {
		return failure(ReasonSourceUnavailable)
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return failure(ReasonSourceUnavailable)
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, 32769))
	if err != nil || len(raw) > 32768 {
		return failure(ReasonSourceUnavailable)
	}
	var envelope struct {
		Data ClarityProjection `json:"data"`
	}
	if json.Unmarshal(raw, &envelope) != nil || !validClarityProjection(envelope.Data, site, s.cfg.ClarityProjectID) {
		return failure(ReasonSourceUnavailable)
	}
	return envelope.Data
}
func validClarityProjection(value ClarityProjection, site, expectedProject string) bool {
	if value.BindingVersion != "" && !clarityBindingVersionPattern.MatchString(value.BindingVersion) {
		return false
	}
	if value.Site != site || value.Source != "microsoft_clarity" || value.Scope != "clarity_project" || value.Window.NumOfDays != 3 || value.Window.TimeZone != "UTC" {
		return false
	}
	if value.ProjectID != "" && (!clarityProjectPattern.MatchString(value.ProjectID) || expectedProject != "" && value.ProjectID != expectedProject) {
		return false
	}
	for _, stamp := range []string{value.Window.ObservedAt, value.UpdatedAt} {
		if stamp != "" {
			if _, err := time.Parse(time.RFC3339Nano, stamp); err != nil {
				return false
			}
		}
	}
	if value.Status == StatusUnavailable {
		return value.Metrics == nil && clarityReason(value.Reason)
	}
	if value.Status != StatusConnected || value.Reason != "" || value.ProjectID == "" || value.Metrics == nil {
		return false
	}
	m := value.Metrics
	if m.Sessions == nil && m.BotSessions == nil && m.Users == nil && m.PagesPerSession == nil {
		return false
	}
	for _, count := range []*int64{m.Sessions, m.BotSessions, m.Users} {
		if count != nil && (*count < 0 || *count > maxSafeInteger) {
			return false
		}
	}
	return ValidClarityUX(m.UX) && (m.PagesPerSession == nil || (!math.IsNaN(*m.PagesPerSession) && !math.IsInf(*m.PagesPerSession, 0) && *m.PagesPerSession >= 0 && *m.PagesPerSession <= maxSafeInteger))
}
func clarityReason(reason string) bool {
	switch reason {
	case "not_connected", "pending_verification", "not_configured", "quota_limited", "source_unavailable", "authorization_required", "no_data":
		return true
	}
	return false
}
