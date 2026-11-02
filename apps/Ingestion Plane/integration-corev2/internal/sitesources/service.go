package sitesources

import (
	"context"
	"crypto/sha256"
	"io"
	"net/http"
	"time"

	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/webvitals"
)

type Service struct {
	path      string
	client    *http.Client
	now       func() time.Time
	admission chan struct{}
}

func NewService(path string, client *http.Client, now func() time.Time) (*Service, error) {
	if _, err := loadBindings(path); err != nil {
		return nil, err
	}
	if now == nil {
		now = time.Now
	}
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{Guard: egress.New(), RequestTimeout: 8 * time.Second})
	}
	closed := *client
	closed.Timeout = 8 * time.Second
	closed.Jar = nil
	closed.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Service{path: path, client: &closed, now: now, admission: make(chan struct{}, 2)}, nil
}
func (s *Service) Collect(ctx context.Context, org string, in Input) (Report, error) {
	if !orgPattern.MatchString(org) || !validInput(in) {
		return Report{}, ErrInvalid
	}
	started := s.now().UTC()
	report := Report{WebsiteID: in.WebsiteID, Site: in.Site, WebsiteRevision: in.WebsiteRevision, Provider: in.Provider, Window: period(started), ObservedAt: started.Format(time.RFC3339Nano), Status: "not_configured"}
	bindings, err := loadBindings(s.path)
	if err != nil {
		return Report{}, ErrUnavailable
	}
	item, ok := bindings[bindingKey(org, in.WebsiteID)]
	if !ok || item.Site != in.Site || item.WebsiteRevision != in.WebsiteRevision || in.Provider == "cookiebot" && item.Cookiebot == nil || in.Provider == "algolia" && item.Algolia == nil {
		return report, nil
	}
	select {
	case s.admission <- struct{}{}:
		defer func() { <-s.admission }()
	default:
		return Report{}, ErrBusy
	}
	ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	keyFile, epoch := "", item.cookieEpoch
	if in.Provider == "cookiebot" {
		keyFile = item.Cookiebot.APIKeyFile
	} else {
		keyFile = item.Algolia.AnalyticsKeyFile
		epoch = item.algoliaEpoch
	}
	key, err := readKey(keyFile)
	if err != nil || sha256.Sum256([]byte(key)) != epoch {
		return Report{}, ErrUnavailable
	}
	stillBound := func() bool {
		bindings, err := loadBindings(s.path)
		latest, ok := bindings[bindingKey(org, in.WebsiteID)]
		return err == nil && ok && sameBinding(latest, item)
	}
	if in.Provider == "cookiebot" {
		report.Cookiebot = s.cookiebot(ctx, *item.Cookiebot, key, report.Window, stillBound)
		a, b := successful(report.Cookiebot.Consents.Status), successful(report.Cookiebot.Inventory.Status)
		switch {
		case a && b:
			report.Status = "connected"
		case a || b:
			report.Status = "partial"
		default:
			report.Status = "unavailable"
		}
	} else {
		report.Algolia = s.algolia(ctx, *item.Algolia, key, report.Window, stillBound)
		report.Status = "unavailable"
		if successful(report.Algolia.Analytics.Status) {
			report.Status = "connected"
		}
	}
	current, err := loadBindings(s.path)
	latest, ok := current[bindingKey(org, in.WebsiteID)]
	if err != nil || !ok || !sameBinding(latest, item) {
		return Report{}, ErrUnavailable
	}
	completed := s.now().UTC()
	if period(completed) != report.Window {
		return Report{}, ErrUnavailable
	}
	report.ObservedAt = completed.Format(time.RFC3339Nano)
	return report, nil
}
func successful(status string) bool { return status == "connected" || status == "no_data" }
func (s *Service) fetch(ctx context.Context, endpoint string, headers map[string]string, stillBound func() bool) ([]byte, string) {
	raw, reason, _ := s.fetchResponse(ctx, endpoint, headers, stillBound)
	return raw, reason
}
func (s *Service) fetchResponse(ctx context.Context, endpoint string, headers map[string]string, stillBound func() bool) ([]byte, string, int) {
	if !stillBound() {
		return nil, "unavailable", 0
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, "unavailable", 0
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "Integration-Core-SiteSources/1.0")
	for name, value := range headers {
		req.Header.Set(name, value)
	}
	response, err := s.client.Do(req)
	if err != nil {
		return nil, "unavailable", 0
	}
	defer response.Body.Close()
	if !stillBound() {
		return nil, "unavailable", response.StatusCode
	}
	switch response.StatusCode {
	case 200:
	case 401, 403:
		return nil, "access_denied", response.StatusCode
	case 402:
		return nil, "plan_required", response.StatusCode
	case 429:
		return nil, "rate_limited", response.StatusCode
	default:
		return nil, "unavailable", response.StatusCode
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, (2<<20)+1))
	if err != nil || len(raw) > 2<<20 || webvitals.UniqueJSON(raw) != nil || !stillBound() {
		return nil, "unavailable", response.StatusCode
	}
	return raw, "", response.StatusCode
}
