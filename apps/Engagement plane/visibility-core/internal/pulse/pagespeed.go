package pulse

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// PageSpeedReport contains point-in-time Lighthouse lab results, not CrUX field
// measurements. Null means unreported; TBT must never be presented as INP.
type PageSpeedReport struct {
	ReportingOrigin   string           `json:"reportingOrigin,omitempty"`
	Performance       *float64         `json:"performance"`
	Accessibility     *float64         `json:"accessibility"`
	BestPractices     *float64         `json:"bestPractices"`
	SEO               *float64         `json:"seo"`
	LCPMs             *float64         `json:"lcpMs"`
	CLS               *float64         `json:"cls"`
	TBTMs             *float64         `json:"tbtMs"`
	FCPMs             *float64         `json:"fcpMs"`
	SpeedIndexMs      *float64         `json:"speedIndexMs"`
	FetchTime         string           `json:"fetchTime"`
	LighthouseVersion string           `json:"lighthouseVersion"`
	Checks            []PageSpeedCheck `json:"checks,omitempty"`
}

type PageSpeedCache struct {
	Stale         bool   `json:"stale"`
	RefreshStatus string `json:"refreshStatus"`
	LastSuccessAt string `json:"lastSuccessAt,omitempty"`
	LastAttemptAt string `json:"lastAttemptAt,omitempty"`
	NextRefreshAt string `json:"nextRefreshAt,omitempty"`
	RefreshReason string `json:"refreshReason,omitempty"`
}

type PageSpeedProjection struct {
	Cache       *PageSpeedCache  `json:"cache,omitempty"`
	Site        string           `json:"site"`
	Source      string           `json:"source"`
	Strategy    string           `json:"strategy"`
	Status      string           `json:"status"`
	RequestedAt string           `json:"requestedAt,omitempty"`
	CompletedAt string           `json:"completedAt,omitempty"`
	Reason      string           `json:"reason,omitempty"`
	Report      *PageSpeedReport `json:"report,omitempty"`
}

func unavailablePageSpeed(site, strategy, reason string) PageSpeedProjection {
	return PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "failed", Reason: reason}
}

func (s *Service) GetPageSpeed(ctx context.Context, site, strategy string) PageSpeedProjection {
	return s.pageSpeed(ctx, http.MethodGet, site, strategy)
}

// StartPageSpeed starts a bounded asynchronous Integration Core audit. It never
// waits for Lighthouse; status reads cannot trigger new provider work.
func (s *Service) StartPageSpeed(ctx context.Context, site, strategy string) PageSpeedProjection {
	return s.pageSpeed(ctx, http.MethodPost, site, strategy)
}

func (s *Service) pageSpeed(ctx context.Context, method, site, strategy string) PageSpeedProjection {
	canonical, err := CanonicalSite(site)
	if err != nil || canonical != site || (strategy != "mobile" && strategy != "desktop") {
		return unavailablePageSpeed(canonical, strategy, ReasonSourceUnavailable)
	}
	// PageSpeed inspects public websites. It needs organization authorization to
	// Integration Core, but never a private Search Console ownership grant.
	if strings.TrimSpace(s.cfg.IntegrationBearerTokenFile) == "" || validateIntegrationURL(s.cfg.IntegrationCoreURL) != nil {
		return unavailablePageSpeed(site, strategy, ReasonNotConfigured)
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		return unavailablePageSpeed(site, strategy, ReasonSourceUnavailable)
	}
	result, err := s.readPageSpeed(ctx, token, method, site, strategy)
	if err != nil || result.Cache != nil {
		return unavailablePageSpeed(site, strategy, ReasonSourceUnavailable)
	}
	return result
}

func (s *Service) readPageSpeed(ctx context.Context, token, method, site, strategy string) (PageSpeedProjection, error) {
	endpoint := s.cfg.IntegrationCoreURL + "/api/v1/insights/pagespeed"
	var body io.Reader
	if method == http.MethodGet {
		endpoint += "?site=" + url.QueryEscape(site) + "&strategy=" + strategy
	} else {
		payload, _ := json.Marshal(map[string]string{"site": site, "strategy": strategy})
		body = bytes.NewReader(payload)
	}
	req, err := http.NewRequestWithContext(ctx, method, endpoint, body)
	if err != nil {
		return PageSpeedProjection{}, err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	if method == http.MethodPost {
		req.Header.Set("Content-Type", "application/json")
	}
	response, err := s.client.Do(req)
	if err != nil {
		return PageSpeedProjection{}, err
	}
	defer response.Body.Close()
	if response.StatusCode != 200 && response.StatusCode != 202 {
		return PageSpeedProjection{}, errors.New("PageSpeed source unavailable")
	}
	payload, err := io.ReadAll(io.LimitReader(response.Body, 32769))
	if err != nil || len(payload) > 32768 {
		return PageSpeedProjection{}, errors.New("PageSpeed response exceeds limit")
	}
	var envelope struct {
		Data PageSpeedProjection `json:"data"`
	}
	if json.Unmarshal(payload, &envelope) != nil || envelope.Data.Site != site || envelope.Data.Strategy != strategy || !validPageSpeedProjection(envelope.Data) {
		return PageSpeedProjection{}, errors.New("invalid PageSpeed projection")
	}
	return envelope.Data, nil
}

func validPageSpeedProjection(value PageSpeedProjection) bool {
	if value.Source != "google_pagespeed" || (value.Strategy != "mobile" && value.Strategy != "desktop") {
		return false
	}
	for _, stamp := range []string{value.RequestedAt, value.CompletedAt} {
		if stamp != "" {
			if _, err := time.Parse(time.RFC3339, stamp); err != nil {
				return false
			}
		}
	}
	switch value.Status {
	case "idle", "running":
		return value.Report == nil && value.Reason == ""
	case "failed":
		return value.Report == nil && (value.Reason == "quota_limited" || value.Reason == "not_configured" || value.Reason == "source_unavailable" || value.Reason == "audit_failed")
	case "complete":
		report := value.Report
		if report == nil || value.Reason != "" || report.LighthouseVersion == "" || len(report.LighthouseVersion) > 64 {
			return false
		}
		if report.ReportingOrigin != "" && !GA4ReportingSiteAllowed(value.Site, report.ReportingOrigin+"/") {
			return false
		}
		if _, err := time.Parse(time.RFC3339, report.FetchTime); err != nil {
			return false
		}
		for _, score := range []*float64{report.Performance, report.Accessibility, report.BestPractices, report.SEO} {
			if !pageSpeedNumber(score, 100) {
				return false
			}
		}
		for _, metric := range []*float64{report.LCPMs, report.CLS, report.TBTMs, report.FCPMs, report.SpeedIndexMs} {
			if !pageSpeedNumber(metric, math.MaxFloat64) {
				return false
			}
		}
		return validPageSpeedChecks(report.Checks)
	default:
		return false
	}
}

func pageSpeedNumber(value *float64, max float64) bool {
	return value == nil || (!math.IsNaN(*value) && !math.IsInf(*value, 0) && *value >= 0 && *value <= max)
}

// ValidPageSpeedProjection validates normalized reports at durable owner boundaries.
func ValidPageSpeedProjection(value PageSpeedProjection) bool { return validPageSpeedProjection(value) }
