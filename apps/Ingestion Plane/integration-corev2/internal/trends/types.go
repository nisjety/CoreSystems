// Package trends defines bounded, unofficial development research reports.
package trends

import (
	"encoding/json"
	"errors"
	"regexp"
	"strings"
	"time"
	"unicode"
)

var ErrInvalid = errors.New("invalid trends request")
var ErrUnavailable = errors.New("trends unavailable")
var country = regexp.MustCompile(`^([A-Z]{2})?$`)

type Query struct {
	Terms     []string `json:"terms"`
	Geo       string   `json:"geo"`
	Timeframe string   `json:"timeframe"`
}
type Row struct {
	Date    string `json:"date"`
	Values  []int  `json:"values"`
	Partial bool   `json:"partial"`
}
type Report struct {
	Query
	Source     string `json:"source"`
	Scale      string `json:"scale"`
	Status     string `json:"status"`
	ObservedAt string `json:"observedAt"`
	Cached     bool   `json:"cached"`
	RetryAt    string `json:"retryAt,omitempty"`
	Rows       []Row  `json:"rows"`
}

func Valid(q Query) bool {
	if len(q.Terms) < 1 || len(q.Terms) > 5 || !country.MatchString(q.Geo) || (q.Timeframe != "today 3-m" && q.Timeframe != "today 12-m" && q.Timeframe != "today 5-y") {
		return false
	}
	seen := map[string]bool{}
	for _, term := range q.Terms {
		if term == "" || len(term) > 80 || strings.TrimSpace(term) != term || seen[term] {
			return false
		}
		for _, r := range term {
			if !unicode.IsPrint(r) {
				return false
			}
		}
		seen[term] = true
	}
	return true
}
func ValidReport(r Report, q Query, now time.Time) bool {
	a, _ := json.Marshal(r.Query)
	b, _ := json.Marshal(q)
	observed, err := time.Parse(time.RFC3339, r.ObservedAt)
	if !Valid(q) || string(a) != string(b) || r.Source != "pytrends_dev" || r.Scale != "relative_0_100_per_request" || err != nil || observed.After(now.Add(time.Minute)) || observed.Before(now.Add(-25*time.Hour)) || len(r.Rows) > 400 || r.Rows == nil {
		return false
	}
	switch r.Status {
	case "available":
		if len(r.Rows) == 0 || r.RetryAt != "" {
			return false
		}
	case "no_data", "disabled", "rate_limited", "blocked", "timeout", "upstream_error", "cooldown":
		if len(r.Rows) != 0 {
			return false
		}
	default:
		return false
	}
	if r.RetryAt != "" {
		t, e := time.Parse(time.RFC3339, r.RetryAt)
		if e != nil || t.After(now.Add(2*time.Hour)) {
			return false
		}
	}
	previous := ""
	for _, row := range r.Rows {
		d, e := time.Parse(time.DateOnly, row.Date)
		if e != nil || d.After(now) || row.Date <= previous || len(row.Values) != len(q.Terms) {
			return false
		}
		for _, v := range row.Values {
			if v < 0 || v > 100 {
				return false
			}
		}
		previous = row.Date
	}
	return true
}
