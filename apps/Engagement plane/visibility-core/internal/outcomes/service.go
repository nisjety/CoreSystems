// Package outcomes projects first-party receipts owned by Leads Core. Person
// fields and installation write credentials never enter this service.
package outcomes

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

var ErrInvalid = errors.New("invalid outcomes context")

type Registry interface {
	Resolve(string, string) (websites.Record, error)
}
type Metrics struct {
	Inquiries int64  `json:"inquiries"`
	Leads     int64  `json:"leads"`
	Bookings  *int64 `json:"bookings"`
}
type Projection struct {
	WebsiteID       string       `json:"websiteId"`
	Site            string       `json:"site"`
	WebsiteRevision int64        `json:"websiteRevision"`
	Source          string       `json:"source"`
	Environment     string       `json:"environment"`
	Status          string       `json:"status"`
	Reason          string       `json:"reason,omitempty"`
	Quarter         string       `json:"quarter"`
	Period          pulse.Period `json:"period"`
	TimeZone        string       `json:"timeZone"`
	Metrics         *Metrics     `json:"metrics"`
	LastReceivedAt  *string      `json:"lastReceivedAt"`
	AsOf            string       `json:"asOf,omitempty"`
}
type Service struct {
	registry      Registry
	base, keyFile string
	client        *http.Client
	now           func() time.Time
}

func New(registry Registry, base, keyFile string) (*Service, error) {
	if base != "" {
		u, err := url.Parse(base)
		if err != nil || u.User != nil || u.Path != "" || u.RawQuery != "" || u.Fragment != "" || u.Scheme != "https" && !(u.Scheme == "http" && (u.Host == "leads-core:3164" || u.Host == "127.0.0.1:3164")) {
			return nil, ErrInvalid
		}
	}
	if (base == "") != (keyFile == "") {
		return nil, ErrInvalid
	}
	return &Service{registry: registry, base: base, keyFile: keyFile, client: &http.Client{Timeout: 8 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}, now: time.Now}, nil
}
func (s *Service) Get(ctx context.Context, id, site, q, environment string) (Projection, error) {
	if environment != "development" && environment != "production" {
		return Projection{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return Projection{}, err
	}
	period, err := outcomePeriod(q, s.now())
	if err != nil || r.Website.ID != id || r.Website.Site != site {
		return Projection{}, ErrInvalid
	}
	result := Projection{WebsiteID: id, Site: site, WebsiteRevision: r.Website.Revision, Source: "leads_core", Environment: environment, Quarter: q, Period: period, TimeZone: "Europe/Oslo", Status: "unavailable", Reason: "not_connected"}
	if s.base == "" {
		return result, nil
	}
	info, err := os.Lstat(s.keyFile)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || info.Size() > 512 {
		return result, nil
	}
	f, err := os.Open(s.keyFile)
	if err != nil {
		return result, nil
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(info, actual) {
		return result, nil
	}
	raw, err := io.ReadAll(io.LimitReader(f, 513))
	token := strings.TrimSpace(string(raw))
	if err != nil || len(token) < 32 || len(token) > 256 || strings.ContainsAny(token, " \r\n\t") {
		return result, nil
	}
	query := url.Values{"websiteId": {id}, "websiteRevision": {strconv.FormatInt(r.Website.Revision, 10)}, "site": {site}, "environment": {environment}, "startDate": {period.StartDate}, "endDate": {period.EndDate}}
	req, err := http.NewRequestWithContext(ctx, "GET", s.base+"/api/v1/website-outcomes?"+query.Encode(), nil)
	if err != nil {
		return result, nil
	}
	req.Header.Set("Authorization", "Bearer "+token)
	resp, err := s.client.Do(req)
	if err != nil {
		result.Reason = "source_unavailable"
		return result, nil
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		if resp.StatusCode >= 500 {
			result.Reason = "source_unavailable"
		}
		return result, nil
	}
	var body struct {
		Data struct {
			WebsiteID       string  `json:"websiteId"`
			Site            string  `json:"site"`
			WebsiteRevision int64   `json:"websiteRevision"`
			Environment     string  `json:"environment"`
			Source          string  `json:"source"`
			Status          string  `json:"status"`
			StartDate       string  `json:"startDate"`
			EndDate         string  `json:"endDate"`
			TimeZone        string  `json:"timeZone"`
			Inquiries       int64   `json:"inquiries"`
			Leads           int64   `json:"leads"`
			Bookings        *int64  `json:"bookings"`
			LastReceivedAt  *string `json:"lastReceivedAt"`
			AsOf            string  `json:"asOf"`
		} `json:"data"`
	}
	d := json.NewDecoder(io.LimitReader(resp.Body, 8193))
	if d.Decode(&body) != nil || d.Decode(new(any)) != io.EOF {
		result.Reason = "source_unavailable"
		return result, nil
	}
	v := body.Data
	asof, e := time.Parse(time.RFC3339Nano, v.AsOf)
	validLast := v.LastReceivedAt == nil
	if v.LastReceivedAt != nil {
		last, lastErr := time.Parse(time.RFC3339Nano, *v.LastReceivedAt)
		validLast = lastErr == nil && !last.After(asof)
	}
	if v.WebsiteID != id || v.Site != site || v.WebsiteRevision != r.Website.Revision || v.Environment != environment || v.Source != "leads_core" || v.Status != "connected" || v.StartDate != period.StartDate || v.EndDate != period.EndDate || v.TimeZone != "Europe/Oslo" || v.Inquiries < 0 || v.Inquiries > 9007199254740991 || v.Leads != v.Inquiries || v.Bookings != nil || e != nil || asof.After(s.now().Add(time.Minute)) || asof.Before(s.now().Add(-time.Minute)) || !validLast {
		result.Reason = "source_unavailable"
		return result, nil
	}
	latest, err := s.registry.Resolve(id, site)
	if err != nil || latest.Website.Revision != r.Website.Revision {
		return Projection{}, websites.ErrConflict
	}
	result.Status = "connected"
	result.Reason = ""
	result.Metrics = &Metrics{Inquiries: v.Inquiries, Leads: v.Leads}
	result.LastReceivedAt = v.LastReceivedAt
	result.AsOf = v.AsOf
	return result, nil
}

// Confirmed first-party receipts include today; Google's delayed reporting
// window deliberately remains unchanged. Boundaries use the business timezone.
func outcomePeriod(q string, now time.Time) (pulse.Period, error) {
	zone, err := time.LoadLocation("Europe/Oslo")
	if err != nil {
		return pulse.Period{}, err
	}
	local := now.In(zone)
	today := time.Date(local.Year(), local.Month(), local.Day(), 0, 0, 0, 0, zone)
	start := time.Date(local.Year(), 1, 1, 0, 0, 0, 0, zone)
	end := today
	if q != "FY" {
		if len(q) != 2 || q[0] != 'Q' || q[1] < '1' || q[1] > '4' {
			return pulse.Period{}, ErrInvalid
		}
		start = time.Date(local.Year(), time.Month(int(q[1]-'1')*3+1), 1, 0, 0, 0, 0, zone)
		end = start.AddDate(0, 3, 0).AddDate(0, 0, -1)
		if end.After(today) {
			end = today
		}
	}
	if start.After(end) {
		return pulse.Period{}, ErrInvalid
	}
	return pulse.Period{StartDate: start.Format("2006-01-02"), EndDate: end.Format("2006-01-02")}, nil
}
