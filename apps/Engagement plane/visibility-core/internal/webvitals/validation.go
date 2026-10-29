package webvitals

import (
	"math"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

func validOrigin(origin, site, env string) bool {
	u, err := url.Parse(origin)
	if err != nil || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.Path != "" || u.RawPath != "" {
		return false
	}
	if env == "development" {
		return origin == "http://localhost:3000" || origin == "http://127.0.0.1:3000" || origin == "http://localhost:3001" || origin == "http://127.0.0.1:3001"
	}
	if env != "production" || u.Scheme != "https" || u.Port() != "" || origin != "https://"+u.Host {
		return false
	}
	s, err := url.Parse(site)
	if err != nil {
		return false
	}
	base := strings.TrimPrefix(s.Host, "www.")
	return u.Host == base || u.Host == "www."+base
}
func validSample(s Sample) bool {
	if !uuidPattern.MatchString(s.SampleID) || s.Sequence < 1 || s.Sequence > 32 || s.Viewport != "narrow" && s.Viewport != "wide" || len(s.Path) > 160 || !pathPattern.MatchString(s.Path) || math.IsNaN(s.Value) || math.IsInf(s.Value, 0) || s.Value < 0 {
		return false
	}
	limit, ok := map[string]float64{"LCP": 120000, "INP": 60000, "CLS": 100}[s.Metric]
	_, err := time.Parse(time.RFC3339Nano, s.ReceivedAt)
	return ok && s.Value <= limit && err == nil
}
func validExport(e Export) bool {
	canonical, err := pulse.CanonicalSite(e.Site)
	if err != nil || canonical != e.Site || !idPattern.MatchString(e.WebsiteID) || e.WebsiteRevision < 1 || !environment(e.Environment) || e.Samples == nil || len(e.Samples) > maxSamples {
		return false
	}
	if _, err = time.Parse(time.RFC3339Nano, e.ObservedAt); err != nil {
		return false
	}
	if e.Status == "not_configured" {
		return e.ReportingOrigin == "" && len(e.Samples) == 0
	}
	if e.Status != "connected" || !validOrigin(e.ReportingOrigin, e.Site, e.Environment) {
		return false
	}
	for _, s := range e.Samples {
		if !validSample(s) {
			return false
		}
	}
	return true
}
func normalize(e Export, w websites.Website, env string, now time.Time) (Export, error) {
	if !validExport(e) || e.WebsiteID != w.ID || e.Site != w.Site || e.WebsiteRevision != w.Revision || e.Environment != env {
		return Export{}, ErrInvalid
	}
	observed, _ := time.Parse(time.RFC3339Nano, e.ObservedAt)
	if observed.After(now) || observed.Before(now.Add(-5*time.Minute)) {
		return Export{}, ErrInvalid
	}
	byMeasurement := map[string]Sample{}
	byPage := map[string]Sample{}
	for _, s := range e.Samples {
		received, _ := time.Parse(time.RFC3339Nano, s.ReceivedAt)
		if received.After(observed) || received.Before(windowStart(now)) {
			return Export{}, ErrInvalid
		}
		if prior, ok := byPage[s.SampleID]; ok && (prior.Path != s.Path || prior.Viewport != s.Viewport) {
			return Export{}, ErrInvalid
		}
		byPage[s.SampleID] = s
		key := s.SampleID + ":" + s.Metric
		if prior, ok := byMeasurement[key]; ok {
			if prior.Sequence == s.Sequence && prior != s {
				return Export{}, ErrInvalid
			}
			if prior.Sequence >= s.Sequence {
				continue
			}
		}
		byMeasurement[key] = s
	}
	next := e
	next.Samples = make([]Sample, 0, len(byMeasurement))
	for _, s := range byMeasurement {
		next.Samples = append(next.Samples, s)
	}
	return next, nil
}
