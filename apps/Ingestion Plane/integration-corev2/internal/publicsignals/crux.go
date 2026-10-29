package publicsignals

import (
	"bytes"
	"context"
	"encoding/json"
	"math"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

func (s *Service) crux(ctx context.Context, site, factor string) Device {
	if pagespeed.ValidateInput(site, "mobile") != nil || (factor != "PHONE" && factor != "DESKTOP") {
		return unavailableDevice(factor, "source_unavailable")
	}
	result := s.cruxOrigin(ctx, site, factor)
	// Only missing coverage permits discovery. Never mask quota, authorization,
	// malformed responses or timeouts by querying more origins.
	if result.Reason != "no_data" || ctx.Err() != nil {
		return result
	}
	if alternate := cruxAlternateSite(site); alternate != "" {
		return s.cruxOrigin(ctx, alternate, factor)
	}
	return result
}

// One exact leading-www toggle, not redirects, arbitrary parents or siblings.
// All network requests still go to Google's fixed public-data endpoint.
func cruxAlternateSite(site string) string {
	host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
	if strings.HasPrefix(host, "www.") {
		host = strings.TrimPrefix(host, "www.")
	} else {
		host = "www." + host
	}
	alternate := "https://" + host + "/"
	if pagespeed.ValidateInput(alternate, "mobile") != nil {
		return ""
	}
	return alternate
}

func (s *Service) cruxOrigin(ctx context.Context, site, factor string) Device {
	failure := func(reason string) Device { return unavailableDevice(factor, reason) }
	if s.key == "" {
		return failure("not_configured")
	}
	origin := strings.TrimSuffix(site, "/")
	body, _ := json.Marshal(map[string]any{"origin": origin, "formFactor": factor, "metrics": []string{"largest_contentful_paint", "interaction_to_next_paint", "cumulative_layout_shift"}})
	req, err := http.NewRequestWithContext(ctx, "POST", "https://chromeuxreport.googleapis.com/v1/records:queryRecord", bytes.NewReader(body))
	if err != nil {
		return failure("source_unavailable")
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Goog-Api-Key", s.key)
	raw, reason := s.request(req, maxResponse)
	if reason != "" {
		return failure(reason)
	}
	var response struct {
		Record struct {
			Key     struct{ Origin, FormFactor string }
			Metrics map[string]struct {
				Percentiles struct {
					P75 json.RawMessage `json:"p75"`
				}
			}
			CollectionPeriod struct{ FirstDate, LastDate cruxDate }
		}
	}
	if json.Unmarshal(raw, &response) != nil {
		return failure("source_unavailable")
	}
	record := response.Record
	start, firstOK := record.CollectionPeriod.FirstDate.date()
	end, lastOK := record.CollectionPeriod.LastDate.date()
	if record.Key.Origin != origin || record.Key.FormFactor != factor || !firstOK || !lastOK || end.After(s.now().UTC()) || end.Sub(start) != 27*24*time.Hour {
		return failure("source_unavailable")
	}
	metrics := Metrics{}
	for _, requested := range []struct {
		name    string
		target  **float64
		integer bool
	}{{"largest_contentful_paint", &metrics.LCPMs, true}, {"interaction_to_next_paint", &metrics.INPMs, true}, {"cumulative_layout_shift", &metrics.CLS, false}} {
		metric, exists := record.Metrics[requested.name]
		if !exists {
			continue
		}
		value, ok := percentile(metric.Percentiles.P75, requested.integer)
		if !ok {
			return failure("source_unavailable")
		}
		*requested.target = &value
	}
	if metrics.LCPMs == nil && metrics.INPMs == nil && metrics.CLS == nil {
		return failure("no_data")
	}
	return Device{FormFactor: factor, ReportingOrigin: record.Key.Origin, Status: "connected", Period: &Period{StartDate: start.Format("2006-01-02"), EndDate: end.Format("2006-01-02")}, Metrics: &metrics}
}

type cruxDate struct{ Year, Month, Day int }

func (d cruxDate) date() (time.Time, bool) {
	if d.Year < 2008 || d.Year > 9999 || d.Month < 1 || d.Month > 12 || d.Day < 1 || d.Day > 31 {
		return time.Time{}, false
	}
	t := time.Date(d.Year, time.Month(d.Month), d.Day, 0, 0, 0, 0, time.UTC)
	return t, t.Year() == d.Year && int(t.Month()) == d.Month && t.Day() == d.Day
}
func percentile(raw json.RawMessage, integer bool) (float64, bool) {
	if len(raw) == 0 || len(raw) > 64 || bytes.Equal(raw, []byte("null")) {
		return 0, false
	}
	value := string(raw)
	if integer {
		// CrUX duration percentiles are integer JSON values. Validate their
		// exact decimal representation before float conversion, so a hostile
		// fractional number near MaxSafe cannot round into an accepted integer.
		for _, char := range value {
			if char < '0' || char > '9' {
				return 0, false
			}
		}
		whole, err := strconv.ParseUint(value, 10, 64)
		return float64(whole), err == nil && whole <= maxSafe
	}
	if raw[0] == '"' {
		if json.Unmarshal(raw, &value) != nil {
			return 0, false
		}
	}
	v, err := strconv.ParseFloat(value, 64)
	return v, err == nil && !math.IsNaN(v) && !math.IsInf(v, 0) && v >= 0 && v <= maxSafe && (!integer || math.Trunc(v) == v)
}
