package webvitals

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"io"
	"math"
	"net/http"
	"time"

	"github.com/triodelab/integration-corev2/internal/egress"
)

type Service struct {
	path, runtime             string
	client, developmentClient *http.Client
	now                       func() time.Time
	admission                 chan struct{}
}

func NewService(path, runtime string, client *http.Client, now func() time.Time) (*Service, error) {
	if _, err := loadBindings(path, runtime); err != nil {
		return nil, err
	}
	if now == nil {
		now = time.Now
	}
	prod := client
	dev := client
	if prod == nil {
		prod = egress.SafeClient(egress.ClientConfig{Guard: egress.New(), RequestTimeout: 15 * time.Second})
	}
	if dev == nil {
		dev = egress.SafeClient(egress.ClientConfig{Guard: egress.New(egress.WithAllowlist([]string{"host.docker.internal", "localhost"}, nil)), RequestTimeout: 15 * time.Second})
	}
	return &Service{path: path, runtime: runtime, client: closedClient(prod), developmentClient: closedClient(dev), now: now, admission: make(chan struct{}, 2)}, nil
}
func closedClient(in *http.Client) *http.Client {
	next := *in
	next.Timeout = 15 * time.Second
	next.Jar = nil
	next.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &next
}
func (s *Service) Get(ctx context.Context, org string, in Input) (Report, error) {
	if !orgPattern.MatchString(org) || !validInput(in) {
		return Report{}, ErrInvalid
	}
	bindings, err := loadBindings(s.path, s.runtime)
	if err != nil {
		return Report{}, ErrUnavailable
	}
	b, exists := bindings[bindingKey(org, in)]
	if !exists || b.Site != in.Site || b.WebsiteRevision != in.WebsiteRevision {
		return empty(in, s.now()), nil
	}
	select {
	case s.admission <- struct{}{}:
		defer func() { <-s.admission }()
	default:
		return Report{}, ErrBusy
	}
	ctx, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	token, err := readToken(b.TokenFile)
	if err != nil || sha256.Sum256([]byte(token)) != b.epoch {
		return Report{}, ErrUnavailable
	}
	origin := b.ReportingOrigin
	client := s.client
	if b.Environment == "development" {
		client = s.developmentClient
		if b.DevelopmentBackendOrigin != "" {
			origin = b.DevelopmentBackendOrigin
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, origin+"/api/web-vitals/export", nil)
	if err != nil {
		return Report{}, ErrUnavailable
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	response, err := client.Do(req)
	if err != nil {
		return Report{}, ErrUnavailable
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return Report{}, ErrUnavailable
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, (2<<20)+1))
	if err != nil || len(raw) > 2<<20 || UniqueJSON(raw) != nil {
		return Report{}, ErrUnavailable
	}
	report, err := decodeExport(raw, b, s.now())
	if err != nil {
		return Report{}, ErrUnavailable
	}
	current, err := loadBindings(s.path, s.runtime)
	if err != nil {
		return Report{}, ErrUnavailable
	}
	latest, ok := current[bindingKey(org, in)]
	if !ok || !sameBinding(latest, b) {
		return Report{}, ErrUnavailable
	}
	return report, nil
}
func sameBinding(a, b binding) bool {
	if a.OrganizationID != b.OrganizationID || a.WebsiteID != b.WebsiteID || a.Site != b.Site || a.WebsiteRevision != b.WebsiteRevision || a.Environment != b.Environment || a.ReportingOrigin != b.ReportingOrigin || a.DevelopmentBackendOrigin != b.DevelopmentBackendOrigin || a.TokenFile != b.TokenFile || a.epoch != b.epoch || len(a.Paths) != len(b.Paths) {
		return false
	}
	for i := range a.Paths {
		if a.Paths[i] != b.Paths[i] {
			return false
		}
	}
	return true
}
func empty(in Input, now time.Time) Report {
	return Report{WebsiteID: in.WebsiteID, Site: in.Site, WebsiteRevision: in.WebsiteRevision, Environment: in.Environment, Status: "not_configured", Samples: []Sample{}, ObservedAt: now.UTC().Format(time.RFC3339Nano)}
}
func decodeExport(raw []byte, b binding, now time.Time) (Report, error) {
	var envelope struct {
		Success bool   `json:"success"`
		Data    Report `json:"data"`
	}
	if json.Unmarshal(raw, &envelope) != nil || !envelope.Success {
		return Report{}, ErrUnavailable
	}
	r := envelope.Data
	if r.WebsiteID != b.WebsiteID || r.Site != b.Site || r.WebsiteRevision != b.WebsiteRevision || r.Environment != b.Environment || r.ReportingOrigin != b.ReportingOrigin || r.Status != "connected" || r.Samples == nil || len(r.Samples) > 5000 {
		return Report{}, ErrUnavailable
	}
	observed, err := time.Parse(time.RFC3339Nano, r.ObservedAt)
	if err != nil || observed.After(now) || observed.Before(now.Add(-5*time.Minute)) {
		return Report{}, ErrUnavailable
	}
	paths := map[string]bool{}
	for _, p := range b.Paths {
		paths[p] = true
	}
	seen := map[string]bool{}
	pageContext := map[string]string{}
	for _, item := range r.Samples {
		identity := item.SampleID + ":" + item.Metric
		page := item.Path + ":" + item.Viewport
		stamp, err := time.Parse(time.RFC3339Nano, item.ReceivedAt)
		if err != nil || stamp.After(observed) || stamp.Before(windowStart(now)) || !samplePattern.MatchString(item.SampleID) || seen[identity] || item.Sequence < 1 || item.Sequence > 32 || !paths[item.Path] || !validPath(item.Path) || item.Viewport != "narrow" && item.Viewport != "wide" || math.IsNaN(item.Value) || math.IsInf(item.Value, 0) || item.Value < 0 {
			return Report{}, ErrUnavailable
		}
		max := 0.
		switch item.Metric {
		case "LCP":
			max = 120000
		case "INP":
			max = 60000
		case "CLS":
			max = 100
		default:
			return Report{}, ErrUnavailable
		}
		if item.Value > max {
			return Report{}, ErrUnavailable
		}
		if previous, ok := pageContext[item.SampleID]; ok && previous != page {
			return Report{}, ErrUnavailable
		}
		seen[identity] = true
		pageContext[item.SampleID] = page
	}
	return r, nil
}

// Retention matches Marketing's displayed UTC calendar window exactly: today
// and the six preceding dates, rather than seven rolling 24-hour periods.
func windowStart(now time.Time) time.Time {
	u := now.UTC()
	return time.Date(u.Year(), u.Month(), u.Day(), 0, 0, 0, 0, time.UTC).AddDate(0, 0, -6)
}
