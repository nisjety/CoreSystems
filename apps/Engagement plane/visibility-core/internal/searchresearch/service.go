// Package searchresearch stores date/query/page evidence and user-defined SEO
// groups. Integration Core remains the source of account authority and tokens.
package searchresearch

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"regexp"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode"
)

var ErrInvalid = errors.New("invalid search research request")
var ErrCapacity = errors.New("search research storage capacity reached")
var orgPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,199}$`)
var idPattern = regexp.MustCompile(`^web_[a-f0-9]{32}$`)
var rulePattern = regexp.MustCompile(`^[a-z][a-z0-9_-]{0,39}$`)

const maxBytes = 32 << 20

type Registry interface {
	OrganizationID() string
	Resolve(string, string) (websites.Record, error)
	List() []websites.Website
}
type Collector interface {
	CollectSearchResearch(context.Context, websites.Record) pulse.SearchResearch
}
type Rule struct {
	ID             string  `json:"id"`
	Name           string  `json:"name"`
	Entity         string  `json:"entity"`
	Contains       string  `json:"contains"`
	MinImpressions int64   `json:"minImpressions"`
	MaxPosition    float64 `json:"maxPosition"`
}
type Entry struct {
	WebsiteID       string                `json:"websiteId"`
	Site            string                `json:"site"`
	WebsiteRevision int64                 `json:"websiteRevision"`
	Binding         string                `json:"binding"`
	LastAttemptAt   string                `json:"lastAttemptAt"`
	NextRunAt       string                `json:"nextRunAt"`
	Reason          string                `json:"reason,omitempty"`
	Report          *pulse.SearchResearch `json:"report,omitempty"`
	Rules           []Rule                `json:"rules"`
}
type diskState struct {
	Version        int     `json:"version"`
	OrganizationID string  `json:"organizationId"`
	Entries        []Entry `json:"entries"`
}
type Aggregate struct {
	Key         string  `json:"key"`
	Clicks      int64   `json:"clicks"`
	Impressions int64   `json:"impressions"`
	Position    float64 `json:"position"`
	Days        int     `json:"days"`
}
type Relationship struct {
	Query       string      `json:"query"`
	Pages       []Aggregate `json:"pages"`
	OverlapDays int         `json:"overlapDays"`
	Candidate   bool        `json:"candidate"`
}
type Group struct {
	Total     int         `json:"total"`
	Truncated bool        `json:"truncated"`
	Rule      Rule        `json:"rule"`
	Items     []Aggregate `json:"items"`
}
type Snapshot struct {
	WebsiteID       string                    `json:"websiteId"`
	Site            string                    `json:"site"`
	WebsiteRevision int64                     `json:"websiteRevision"`
	Source          string                    `json:"source"`
	Status          string                    `json:"status"`
	Reason          string                    `json:"reason,omitempty"`
	Period          pulse.Period              `json:"period"`
	ObservedAt      string                    `json:"observedAt"`
	LastAttemptAt   string                    `json:"lastAttemptAt"`
	NextRunAt       string                    `json:"nextRunAt"`
	Stale           bool                      `json:"stale"`
	Truncated       bool                      `json:"truncated"`
	Partial         bool                      `json:"partial"`
	Rows            []pulse.SearchObservation `json:"rows"`
	Queries         []Aggregate               `json:"queries"`
	Pages           []Aggregate               `json:"pages"`
	Weeks           []Aggregate               `json:"weeks"`
	Relationships   []Relationship            `json:"relationships"`
	Groups          []Group                   `json:"groups"`
	Rules           []Rule                    `json:"rules"`
}

func cloneState(v diskState) diskState {
	raw, _ := json.Marshal(v)
	var out diskState
	_ = json.Unmarshal(raw, &out)
	return out
}
func stamp(t time.Time) string  { return t.UTC().Format(time.RFC3339Nano) }
func parsed(s string) time.Time { t, _ := time.Parse(time.RFC3339Nano, s); return t }
func binding(r websites.Record) string {
	raw, _ := json.Marshal(r)
	v := sha256.Sum256(raw)
	return hex.EncodeToString(v[:])
}
func ValidRule(v Rule) bool { return validRule(v) }
func validRule(v Rule) bool {
	return rulePattern.MatchString(v.ID) && len(v.Name) > 0 && len(v.Name) <= 100 && !strings.ContainsFunc(v.Name, unicode.IsControl) && (v.Entity == "query" || v.Entity == "page") && len(v.Contains) <= 200 && !strings.ContainsFunc(v.Contains, unicode.IsControl) && v.MinImpressions >= 0 && v.MinImpressions <= 1000000000 && v.MaxPosition >= 0 && v.MaxPosition <= 1000
}
func validReport(site string, r *pulse.SearchResearch) bool {
	if r == nil {
		return true
	}
	a, e := time.Parse("2006-01-02", r.Period.StartDate)
	b, f := time.Parse("2006-01-02", r.Period.EndDate)
	if r.Site != site || r.Status != "connected" || r.Reason != "" || r.Rows == nil || len(r.Rows) > 5000 || parsed(r.ObservedAt).IsZero() || e != nil || f != nil || b.Before(a) || b.Sub(a) > 90*24*time.Hour {
		return false
	}
	seen := map[string]bool{}
	for _, v := range r.Rows {
		d, err := time.Parse("2006-01-02", v.Date)
		key := v.Date + "\x00" + v.Query + "\x00" + v.Page
		if err != nil || d.Before(a) || d.After(b) || len(v.Query) == 0 || len(v.Query) > 512 || strings.ContainsFunc(v.Query, unicode.IsControl) || !discovery.SafeURL(site, v.Page) || v.Clicks < 0 || v.Impressions < 0 || v.Impressions > 1<<40 || v.Clicks > v.Impressions || !(v.Position >= 0 && v.Position <= 1000000) || seen[key] {
			return false
		}
		seen[key] = true
	}
	return true
}
func validState(v diskState) error {
	if v.Version != 1 || !orgPattern.MatchString(v.OrganizationID) || v.Entries == nil || len(v.Entries) > 64 {
		return ErrInvalid
	}
	seen := map[string]bool{}
	for _, e := range v.Entries {
		if !idPattern.MatchString(e.WebsiteID) || !discovery.SafeURL(e.Site, e.Site) || e.WebsiteRevision < 1 || len(e.Binding) != 64 || seen[e.WebsiteID] || e.Rules == nil || len(e.Rules) > 20 || !validReport(e.Site, e.Report) || (e.LastAttemptAt != "" && parsed(e.LastAttemptAt).IsZero()) || (e.NextRunAt != "" && parsed(e.NextRunAt).IsZero()) || (e.Reason != "" && e.Reason != "source_unavailable" && e.Reason != "authorization_required" && e.Reason != "storage_unavailable") {
			return ErrInvalid
		}
		seen[e.WebsiteID] = true
		rules := map[string]bool{}
		for _, r := range e.Rules {
			if !validRule(r) || rules[r.ID] {
				return ErrInvalid
			}
			rules[r.ID] = true
		}
	}
	return nil
}

type Service struct {
	repo         *Repository
	registry     Registry
	collector    Collector
	authority    analyticssync.Authority
	now          func() time.Time
	running      atomic.Bool
	wake         chan struct{}
	storageRetry sync.Map
}

func New(repo *Repository, r Registry, c Collector, a analyticssync.Authority, now func() time.Time) *Service {
	if now == nil {
		now = time.Now
	}
	return &Service{repo: repo, registry: r, collector: c, authority: a, now: now, wake: make(chan struct{}, 1)}
}
func (s *Service) record(id, site string, revision int64) (websites.Record, error) {
	if s.repo == nil || s.registry == nil || s.repo.org != s.registry.OrganizationID() || !idPattern.MatchString(id) {
		return websites.Record{}, ErrInvalid
	}
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return r, err
	}
	if r.Website.Site != site || revision > 0 && r.Website.Revision != revision {
		return r, websites.ErrConflict
	}
	return r, nil
}
func (s *Service) entry(r websites.Record) Entry {
	for _, e := range s.repo.read().Entries {
		if e.WebsiteID == r.Website.ID && e.Site == r.Website.Site && e.Binding == binding(r) {
			return e
		}
	}
	return Entry{WebsiteID: r.Website.ID, Site: r.Website.Site, WebsiteRevision: r.Website.Revision, Binding: binding(r), Rules: []Rule{}}
}
func (s *Service) Get(ctx context.Context, id, site string) (Snapshot, error) {
	r, err := s.record(id, site, 0)
	if err != nil {
		return Snapshot{}, err
	}
	e := s.entry(r)
	if retry, ok := s.storageRetry.Load(r.Website.ID + ":" + binding(r)); ok && s.now().Before(retry.(time.Time)) {
		e.Reason = "storage_unavailable"
	}
	access := s.authority != nil && s.authority.Check(ctx, r).SearchConsole
	current, err := s.record(id, site, r.Website.Revision)
	if err != nil || binding(current) != binding(r) {
		return Snapshot{}, websites.ErrConflict
	}
	if !access {
		e.Report = nil
		e.Reason = "authorization_required"
	}
	return build(e, s.now()), nil
}
func (s *Service) Command(ctx context.Context, id, site string, rev int64, operation string, rule *Rule) (Snapshot, error) {
	r, err := s.record(id, site, rev)
	if err != nil {
		return Snapshot{}, err
	}
	if rev < 1 || (operation != "refresh" && operation != "save_rule" && operation != "remove_rule") || operation != "refresh" && (rule == nil || !validRule(*rule)) {
		return Snapshot{}, ErrInvalid
	}
	e := s.entry(r)
	if operation == "refresh" {
		if !parsed(e.LastAttemptAt).IsZero() && s.now().Sub(parsed(e.LastAttemptAt)) < time.Minute {
			return Snapshot{}, ErrCapacity
		}
		e.NextRunAt = stamp(s.now())
	} else {
		kept := []Rule{}
		for _, v := range e.Rules {
			if v.ID != rule.ID {
				kept = append(kept, v)
			}
		}
		if operation == "save_rule" {
			kept = append(kept, *rule)
		}
		if len(kept) > 20 {
			return Snapshot{}, ErrCapacity
		}
		e.Rules = kept
	}
	if err = s.save(e); err != nil {
		return Snapshot{}, err
	}
	select {
	case s.wake <- struct{}{}:
	default:
	}
	return s.Get(ctx, id, site)
}
func (s *Service) save(e Entry) error {
	return s.repo.update(func(v diskState) (diskState, error) {
		for i, old := range v.Entries {
			if old.WebsiteID == e.WebsiteID {
				v.Entries[i] = e
				return v, nil
			}
		}
		v.Entries = append(v.Entries, e)
		return v, nil
	})
}

// Serial, restart-safe daily collection. GET only reads storage and checks
// current account authority; it never issues a provider reporting call.
func (s *Service) Run(ctx context.Context) {
	if !s.running.CompareAndSwap(false, true) {
		return
	}
	defer s.running.Store(false)
	timer := time.NewTicker(time.Minute)
	defer timer.Stop()
	for {
		for _, w := range s.registry.List() {
			if ctx.Err() != nil {
				return
			}
			r, err := s.record(w.ID, w.Site, w.Revision)
			if err != nil {
				continue
			}
			e := s.entry(r)
			retry, blocked := s.storageRetry.Load(r.Website.ID + ":" + binding(r))
			if blocked && s.now().Before(retry.(time.Time)) {
				continue
			}
			if !parsed(e.NextRunAt).After(s.now()) {
				s.collect(ctx, r, e)
			}
		}
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
		case <-s.wake:
		}
	}
}
func (s *Service) collect(parent context.Context, r websites.Record, e Entry) {
	ctx, cancel := context.WithTimeout(parent, 65*time.Second)
	defer cancel()
	start := s.now()
	access := s.authority != nil && s.authority.Check(ctx, r).SearchConsole
	e.LastAttemptAt = stamp(start)
	e.NextRunAt = stamp(start.Add(time.Hour))
	e.Reason = "authorization_required"
	if access && s.collector != nil {
		report := s.collector.CollectSearchResearch(ctx, r)
		e.Reason = "source_unavailable"
		if validReport(e.Site, &report) {
			e.Report = &report
			e.Reason = ""
			e.NextRunAt = stamp(start.Add(24 * time.Hour))
		}
	}
	current, err := s.record(e.WebsiteID, e.Site, e.WebsiteRevision)
	if parent.Err() != nil || err != nil || binding(current) != e.Binding {
		return
	}
	if access && !s.authority.Check(ctx, r).SearchConsole {
		e.Report = nil
		e.Reason = "authorization_required"
	}
	saveErr := s.repo.update(func(v diskState) (diskState, error) {
		for i, old := range v.Entries {
			if old.WebsiteID == e.WebsiteID {
				if old.Binding != e.Binding {
					return v, nil
				}
				e.Rules = old.Rules
				v.Entries[i] = e
				return v, nil
			}
		}
		v.Entries = append(v.Entries, e)
		return v, nil
	})
	if saveErr != nil {
		s.storageRetry.Store(r.Website.ID+":"+binding(r), s.now().Add(time.Hour))
	} else {
		s.storageRetry.Delete(r.Website.ID + ":" + binding(r))
	}
}
func build(e Entry, now time.Time) Snapshot {
	out := Snapshot{WebsiteID: e.WebsiteID, Site: e.Site, WebsiteRevision: e.WebsiteRevision, Source: "gsc_date_query_page", Status: "unavailable", Reason: e.Reason, LastAttemptAt: e.LastAttemptAt, NextRunAt: e.NextRunAt, Partial: true, Rows: []pulse.SearchObservation{}, Queries: []Aggregate{}, Pages: []Aggregate{}, Weeks: []Aggregate{}, Relationships: []Relationship{}, Groups: []Group{}, Rules: e.Rules}
	if e.Report == nil {
		return out
	}
	r := e.Report
	out.Status = "connected"
	out.Period = r.Period
	out.ObservedAt = r.ObservedAt
	out.Rows = r.Rows
	out.Truncated = r.Truncated
	out.Stale = now.Sub(parsed(r.ObservedAt)) >= 24*time.Hour
	out.Queries = aggregate(r.Rows, func(v pulse.SearchObservation) string { return v.Query })
	out.Pages = aggregate(r.Rows, func(v pulse.SearchObservation) string { return v.Page })
	out.Weeks = aggregate(r.Rows, func(v pulse.SearchObservation) string {
		d, _ := time.Parse("2006-01-02", v.Date)
		day := (int(d.Weekday()) + 6) % 7
		return d.AddDate(0, 0, -day).Format("2006-01-02")
	})
	sort.Slice(out.Weeks, func(i, j int) bool { return out.Weeks[i].Key < out.Weeks[j].Key })
	byQuery := map[string][]pulse.SearchObservation{}
	for _, v := range r.Rows {
		byQuery[v.Query] = append(byQuery[v.Query], v)
	}
	for _, q := range out.Queries {
		rows := byQuery[q.Key]
		pages := aggregate(rows, func(v pulse.SearchObservation) string { return v.Page })
		dates := map[string]map[string]bool{}
		for _, v := range rows {
			if v.Impressions == 0 {
				continue
			}
			if dates[v.Date] == nil {
				dates[v.Date] = map[string]bool{}
			}
			dates[v.Date][v.Page] = true
		}
		overlap := 0
		for _, p := range dates {
			if len(p) > 1 {
				overlap++
			}
		}
		strong := 0
		for _, p := range pages {
			if p.Impressions >= 10 {
				strong++
			}
		}
		out.Relationships = append(out.Relationships, Relationship{q.Key, pages, overlap, overlap >= 3 && strong >= 2})
	}
	for _, rule := range e.Rules {
		rows := out.Queries
		if rule.Entity == "page" {
			rows = out.Pages
		}
		g := Group{Rule: rule, Items: []Aggregate{}}
		for _, v := range rows {
			if strings.Contains(strings.ToLower(v.Key), strings.ToLower(rule.Contains)) && v.Impressions >= rule.MinImpressions && (rule.MaxPosition == 0 || v.Position <= rule.MaxPosition) {
				g.Total++
				if len(g.Items) < 250 {
					g.Items = append(g.Items, v)
				} else {
					g.Truncated = true
				}
			}
		}
		out.Groups = append(out.Groups, g)
	}
	return out
}
func aggregate(rows []pulse.SearchObservation, key func(pulse.SearchObservation) string) []Aggregate {
	values := map[string]*Aggregate{}
	days := map[string]map[string]bool{}
	for _, v := range rows {
		k := key(v)
		if values[k] == nil {
			values[k] = &Aggregate{Key: k}
			days[k] = map[string]bool{}
		}
		a := values[k]
		a.Clicks += v.Clicks
		a.Impressions += v.Impressions
		a.Position += v.Position * float64(v.Impressions)
		days[k][v.Date] = true
	}
	out := []Aggregate{}
	for k, v := range values {
		v.Days = len(days[k])
		if v.Impressions > 0 {
			v.Position /= float64(v.Impressions)
		}
		out = append(out, *v)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Impressions != out[j].Impressions {
			return out[i].Impressions > out[j].Impressions
		}
		return out[i].Key < out[j].Key
	})
	return out
}
