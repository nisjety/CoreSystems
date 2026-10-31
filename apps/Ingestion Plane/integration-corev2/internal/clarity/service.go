package clarity

import (
	"context"
	"net/http"
	"sync"
	"time"

	"github.com/triodelab/integration-corev2/internal/pagespeed"
)

const second = time.Second
const successTTL = 6 * time.Hour
const failureTTL = time.Hour
const maxProjectRequestsPerDay = 8

type Window struct {
	NumOfDays  int    `json:"numOfDays"`
	TimeZone   string `json:"timeZone"`
	ObservedAt string `json:"observedAt,omitempty"`
}
type Snapshot struct {
	Site           string   `json:"site"`
	Source         string   `json:"source"`
	Status         string   `json:"status"`
	Scope          string   `json:"scope"`
	ProjectID      string   `json:"projectId,omitempty"`
	BindingVersion string   `json:"bindingVersion,omitempty"`
	Window         Window   `json:"window"`
	Metrics        *Metrics `json:"metrics"`
	UpdatedAt      string   `json:"updatedAt,omitempty"`
	Reason         string   `json:"reason,omitempty"`
}
type key struct {
	org, site, project, version string
	epoch                       [32]byte
	days                        int
}
type cached struct {
	value Snapshot
	until time.Time
}
type budget struct {
	day   string
	count int
}

// Service caches project-scoped UTC exports, not exact-origin reports. Quota
// guards are per process; provider 429 remains authoritative across restarts.
type Service struct {
	bindingsPath string
	client       *http.Client
	now          func() time.Time
	mu           sync.Mutex
	cache        map[key]cached
	requests     map[string]budget
	inflight     map[key]chan struct{}
}

func NewService(path string, client *http.Client, now func() time.Time) (*Service, error) {
	_, err := loadBindings(path)
	if err != nil {
		return nil, err
	}
	if now == nil {
		now = time.Now
	}
	return &Service{bindingsPath: path, client: exportClient(client), now: now, cache: map[key]cached{}, requests: map[string]budget{}, inflight: map[key]chan struct{}{}}, nil
}

func empty(site string, days int, reason string) Snapshot {
	return Snapshot{Site: site, Source: "microsoft_clarity", Status: "unavailable", Scope: "clarity_project", Window: Window{NumOfDays: days, TimeZone: "UTC"}, Reason: reason}
}

func (s *Service) Get(ctx context.Context, org, site string, days int) Snapshot {
	if org == "" || days < 1 || days > 3 || pagespeed.ValidateInput(site, "mobile") != nil {
		return empty(site, days, "source_unavailable")
	}
	item, exists, err := s.currentBinding(org, site)
	if err != nil {
		return empty(site, days, "source_unavailable")
	}
	if !exists {
		return empty(site, days, "not_configured")
	}
	ctx, cancel := context.WithTimeout(ctx, 8*second)
	defer cancel()
	snapshot := s.get(ctx, item, days)
	snapshot.Site = site // Project-wide export, scoped to the requesting site alias.
	current, exists, err := s.currentBinding(org, site)
	if err != nil || (exists && current != item) {
		return empty(site, days, "source_unavailable")
	}
	if !exists {
		return empty(site, days, "not_configured")
	}
	return snapshot
}

func (s *Service) get(ctx context.Context, item binding, days int) Snapshot {
	requestKey := key{org: item.OrganizationID, site: item.Site, project: item.ProjectID, version: bindingVersion(item), epoch: item.credentialEpoch, days: days}
	project := item.ProjectID
	now := s.now().UTC()
	fallback := empty(item.Site, days, "source_unavailable")
	fallback.ProjectID = project
	fallback.BindingVersion = requestKey.version
	current, exists, err := s.currentBinding(item.OrganizationID, item.Site)
	if err != nil || !exists || current != item {
		return empty(item.Site, days, "source_unavailable")
	}
	s.mu.Lock()
	if prior, exists := s.cache[requestKey]; exists && prior.until.After(now) {
		s.mu.Unlock()
		return clone(prior.value)
	}
	if running, exists := s.inflight[requestKey]; exists {
		s.mu.Unlock()
		select {
		case <-running:
			return s.get(ctx, item, days)
		case <-ctx.Done():
			return fallback
		}
	}
	budget := s.requests[project]
	day := now.Format("2006-01-02")
	if budget.day != day {
		budget = struct {
			day   string
			count int
		}{day: day}
	}
	if budget.count >= maxProjectRequestsPerDay {
		s.mu.Unlock()
		fallback.Reason = "quota_limited"
		return fallback
	}
	if len(s.inflight) >= 4 {
		s.mu.Unlock()
		return fallback
	}
	s.requests[project] = struct {
		day   string
		count int
	}{day: day, count: budget.count + 1}
	done := make(chan struct{})
	s.inflight[requestKey] = done
	s.mu.Unlock()
	metrics, reason := fetch(ctx, s.client, item, days)
	snapshot := fallback
	snapshot.Reason = reason
	snapshot.Metrics = metrics
	snapshot.Window.ObservedAt = now.Format(time.RFC3339)
	snapshot.UpdatedAt = s.now().UTC().Format(time.RFC3339)
	ttl := failureTTL
	if metrics != nil && reason == "" {
		snapshot.Status = "connected"
		ttl = successTTL
	}
	s.mu.Lock()
	if len(s.cache) >= 128 {
		for old := range s.cache {
			delete(s.cache, old)
			break
		}
	}
	s.cache[requestKey] = cached{value: clone(snapshot), until: now.Add(ttl)}
	delete(s.inflight, requestKey)
	close(done)
	s.mu.Unlock()
	return snapshot
}

func clone(value Snapshot) Snapshot {
	if value.Metrics == nil {
		return value
	}
	metrics := *value.Metrics
	if metrics.Sessions != nil {
		copy := *metrics.Sessions
		metrics.Sessions = &copy
	}
	if metrics.BotSessions != nil {
		copy := *metrics.BotSessions
		metrics.BotSessions = &copy
	}
	if metrics.Users != nil {
		copy := *metrics.Users
		metrics.Users = &copy
	}
	if metrics.PagesPerSession != nil {
		copy := *metrics.PagesPerSession
		metrics.PagesPerSession = &copy
	}
	if metrics.UX != nil {
		ux := *metrics.UX
		for _, target := range []**float64{&ux.DeadClickPercent, &ux.RageClickPercent, &ux.ExcessiveScrollPercent, &ux.QuickBackPercent, &ux.ScriptErrorPercent, &ux.ErrorClickPercent, &ux.AverageScrollDepth, &ux.ActiveSeconds, &ux.TotalSeconds} {
			if *target != nil {
				value := **target
				*target = &value
			}
		}
		metrics.UX = &ux
	}
	value.Metrics = &metrics
	return value
}
