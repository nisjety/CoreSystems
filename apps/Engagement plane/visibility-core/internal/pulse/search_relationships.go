package pulse

import (
	"context"
	"encoding/json"
	"errors"
	"time"
)

// SearchObservation is an observed date/query/page tuple, not a rank tracker or
// a market-volume estimate. An absent tuple is unknown, never a measured zero.
type SearchObservation struct {
	Date        string  `json:"date"`
	Query       string  `json:"query"`
	Page        string  `json:"page"`
	Clicks      int64   `json:"clicks"`
	Impressions int64   `json:"impressions"`
	Position    float64 `json:"position"`
}
type SearchResearch struct {
	Site       string              `json:"site"`
	Status     string              `json:"status"`
	Period     Period              `json:"period"`
	Rows       []SearchObservation `json:"rows"`
	Truncated  bool                `json:"truncated"`
	ObservedAt string              `json:"observedAt"`
	Reason     string              `json:"reason,omitempty"`
}

func (s *Service) GetSearchResearch(ctx context.Context, site string) SearchResearch {
	now := s.now().UTC()
	end := now.AddDate(0, 0, -3)
	period := Period{end.AddDate(0, 0, -89).Format(dateLayout), end.Format(dateLayout)}
	out := SearchResearch{Site: site, Status: StatusUnavailable, Period: period, Rows: []SearchObservation{}, Reason: ReasonSourceUnavailable}
	if canonical, err := CanonicalSite(site); err != nil || canonical != site || !s.cfg.complete() || validateConfig(s.cfg) != nil {
		out.Reason = ReasonNotConfigured
		return out
	}
	ctx, cancel := context.WithTimeout(ctx, 50*time.Second)
	defer cancel()
	token, err := readBearerToken(s.cfg.IntegrationBearerTokenFile)
	if err != nil {
		return out
	}
	property, err := s.verifiedProperty(ctx, token, site)
	if err != nil {
		return out
	}
	if property == "" {
		out.Reason = ReasonPropertyUnverified
		return out
	}
	seen := map[string]bool{}
	rows := []SearchObservation{}
	for offset := 0; offset < 5000; offset += 1000 {
		raw, err := s.callAction(ctx, token, "gsc.search_analytics.query", map[string]any{"siteUrl": property}, map[string]any{"startDate": period.StartDate, "endDate": period.EndDate, "dimensions": []string{"date", "query", "page"}, "rowLimit": 1000, "startRow": offset, "dataState": "final"})
		if err != nil {
			return out
		}
		var response struct {
			Rows []providerSearchRow `json:"rows"`
		}
		if json.Unmarshal(raw, &response) != nil || len(response.Rows) > 1000 {
			return out
		}
		for _, r := range response.Rows {
			row, err := normalizeObservation(r, property, period)
			if err != nil {
				return out
			}
			key := row.Date + "\x00" + row.Query + "\x00" + row.Page
			if seen[key] {
				return out
			}
			seen[key] = true
			rows = append(rows, row)
		}
		if len(response.Rows) < 1000 {
			break
		}
		if offset == 4000 {
			out.Truncated = true
		}
	}
	out.Status = StatusConnected
	out.Reason = ""
	out.Rows = rows
	out.ObservedAt = now.Format(time.RFC3339)
	return out
}
func normalizeObservation(r providerSearchRow, property string, p Period) (SearchObservation, error) {
	if len(r.Keys) != 3 {
		return SearchObservation{}, errors.New("invalid relationship keys")
	}
	day, err := time.Parse(dateLayout, r.Keys[0])
	if err != nil || day.Format(dateLayout) != r.Keys[0] || r.Keys[0] < p.StartDate || r.Keys[0] > p.EndDate || !validSearchKey(r.Keys[1], property, "query") || !validSearchKey(r.Keys[2], property, "page") {
		return SearchObservation{}, errors.New("invalid relationship context")
	}
	metric := r
	metric.Keys = []string{r.Keys[1]}
	normalized, err := normalizeSearchRow(metric, property, "query")
	if err != nil {
		return SearchObservation{}, err
	}
	return SearchObservation{r.Keys[0], r.Keys[1], r.Keys[2], normalized.Clicks, normalized.Impressions, normalized.Position}, nil
}
