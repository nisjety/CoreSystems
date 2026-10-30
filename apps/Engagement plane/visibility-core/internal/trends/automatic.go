package trends

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type Registry interface {
	List() []websites.Website
	Resolve(string, string) (websites.Record, error)
	OrganizationID() string
}
type AutomaticSource interface {
	TrendsSearch(context.Context, websites.Record) (pulse.SearchProjection, error)
	GetTrends(context.Context, string, string, int64, Query) (Report, error)
}
type Seed struct {
	Source     string       `json:"source"`
	Selection  string       `json:"selection"`
	Terms      []string     `json:"terms"`
	ObservedAt string       `json:"observedAt"`
	Period     pulse.Period `json:"period"`
}
type Snapshot struct {
	WebsiteID       string  `json:"websiteId"`
	Site            string  `json:"site"`
	WebsiteRevision int64   `json:"websiteRevision"`
	Source          string  `json:"source"`
	Status          string  `json:"status"`
	Reason          string  `json:"reason,omitempty"`
	UpdatedAt       string  `json:"updatedAt,omitempty"`
	NextSyncAt      string  `json:"nextSyncAt,omitempty"`
	ExpiresAt       string  `json:"expiresAt,omitempty"`
	Seed            *Seed   `json:"seed,omitempty"`
	Report          *Report `json:"report,omitempty"`
}
type autoEntry struct {
	Snapshot
	SeedKey string `json:"seedKey,omitempty"`
}
type Automatic struct {
	mu       sync.Mutex
	repo     *autoRepository
	registry Registry
	source   AutomaticSource
	now      func() time.Time
}

func NewAutomatic(path, org string, registry Registry, source AutomaticSource, now func() time.Time) (*Automatic, error) {
	if registry == nil || source == nil || registry.OrganizationID() != org {
		return nil, ErrInvalid
	}
	repo, err := openAutomatic(path, org)
	if err != nil {
		return nil, err
	}
	if now == nil {
		now = time.Now
	}
	return &Automatic{repo: repo, registry: registry, source: source, now: now}, nil
}
func automaticEmpty(w websites.Website, status string) Snapshot {
	return Snapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "marketing_trends_auto", Status: status}
}
func sameContext(s Snapshot, w websites.Website) bool {
	return s.WebsiteID == w.ID && s.Site == w.Site && s.WebsiteRevision == w.Revision
}
func timeValue(v string) time.Time { t, _ := time.Parse(time.RFC3339Nano, v); return t }
func utc(t time.Time) string       { return t.UTC().Format(time.RFC3339Nano) }

// SelectTerms uses only observed queries, never URL guesses or synthesized text.
// Canonical ordering stabilizes the comparison when ranks change within a set.
func SelectTerms(rows []pulse.SearchRow) []string {
	ranked := append([]pulse.SearchRow(nil), rows...)
	sort.SliceStable(ranked, func(i, j int) bool {
		if ranked[i].Impressions != ranked[j].Impressions {
			return ranked[i].Impressions > ranked[j].Impressions
		}
		if ranked[i].Clicks != ranked[j].Clicks {
			return ranked[i].Clicks > ranked[j].Clicks
		}
		return ranked[i].Key < ranked[j].Key
	})
	terms := []string{}
	seen := map[string]bool{}
	for _, row := range ranked {
		term := strings.TrimSpace(row.Key)
		key := strings.ToLower(term)
		if row.Impressions <= 0 || seen[key] || !Valid(Query{Terms: []string{term}, Timeframe: "today 12-m"}) {
			continue
		}
		terms = append(terms, term)
		seen[key] = true
		if len(terms) == 5 {
			break
		}
	}
	sort.Strings(terms)
	return terms
}
func (a *Automatic) seed(ctx context.Context, r websites.Record) (*Seed, string, string) {
	search, err := a.source.TrendsSearch(ctx, r)
	if err != nil || search.Site != r.Website.Site || search.Source != "google_search_console" || search.Status != "connected" || !timeValue(search.UpdatedAt).Add(6*time.Hour).After(a.now()) || timeValue(search.UpdatedAt).After(a.now().Add(time.Minute)) {
		return nil, "", "waiting_for_search_console"
	}
	terms := SelectTerms(search.Queries)
	if len(terms) == 0 {
		return nil, "", "waiting_for_keywords"
	}
	seed := &Seed{Source: "google_search_console", Selection: "impressions_then_clicks", Terms: terms, ObservedAt: search.UpdatedAt, Period: search.Period}
	raw, _ := json.Marshal(struct {
		Terms                []string
		Connection, Property string
	}{terms, r.GSCConnectionID, r.Website.Integrations.SearchConsole.Property})
	sum := sha256.Sum256(raw)
	return seed, hex.EncodeToString(sum[:]), ""
}

// Get reads saved Trends only. Fresh Integration metadata checks inside the
// Search Console projection may hide a revoked grant; they never collect metrics.
func (a *Automatic) Get(ctx context.Context, id, site string) (Snapshot, error) {
	ctx, cancel := context.WithTimeout(ctx, 12*time.Second)
	defer cancel()
	r, err := a.registry.Resolve(id, site)
	if err != nil {
		return Snapshot{}, err
	}
	if r.Website.ID != id || r.Website.Site != site {
		return Snapshot{}, ErrInvalid
	}
	seed, key, waiting := a.seed(ctx, r)
	latest, err := a.registry.Resolve(id, site)
	if err != nil || latest.Website.Revision != r.Website.Revision {
		return Snapshot{}, websites.ErrConflict
	}
	if waiting != "" {
		return automaticEmpty(r.Website, waiting), nil
	}
	current := a.repo.get(id)
	if !sameContext(current.Snapshot, r.Website) || current.SeedKey != key {
		out := automaticEmpty(r.Website, "pending")
		out.Seed = seed
		return out, nil
	}
	out := current.Snapshot
	if out.Report != nil && !timeValue(out.ExpiresAt).After(a.now()) {
		out.Report = nil
		if out.Status == "complete" {
			out.Status = "pending"
			out.Reason = "report_expired"
			out.Seed = seed
		}
	}
	return out, nil
}

// Run is a single background writer, independent of page visits. Receipts,
// retry times and original provider observation ages survive process restarts.
func (a *Automatic) Run(ctx context.Context) {
	for {
		a.cycle(ctx)
		timer := time.NewTimer(time.Minute)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
	}
}
func (a *Automatic) cycle(ctx context.Context) {
	a.mu.Lock()
	defer a.mu.Unlock()
	sites := a.registry.List()
	if len(sites) > 64 {
		sites = sites[:64]
	}
	for _, w := range sites {
		if ctx.Err() != nil {
			return
		}
		r, err := a.registry.Resolve(w.ID, w.Site)
		if err != nil {
			continue
		}
		old := a.repo.get(w.ID)
		bounded, cancel := context.WithTimeout(ctx, 50*time.Second)
		a.collect(bounded, r, old)
		cancel()
	}
}
func (a *Automatic) collect(ctx context.Context, r websites.Record, old autoEntry) {
	now := a.now()
	seed, key, waiting := a.seed(ctx, r)
	state := automaticEmpty(r.Website, waiting)
	state.UpdatedAt = utc(now)
	state.NextSyncAt = utc(now.Add(time.Minute))
	state.Seed = seed
	if waiting != "" {
		if sameContext(old.Snapshot, r.Website) && old.Status == waiting && old.Report == nil {
			return
		}
		// Revoke/missing evidence replaces a saved result, never presents it as zero.
		_ = a.repo.save(autoEntry{Snapshot: state})
		return
	}
	if sameContext(old.Snapshot, r.Website) && old.SeedKey == key && timeValue(old.NextSyncAt).After(now) {
		return
	}
	if sameContext(old.Snapshot, r.Website) && old.SeedKey == key && old.Report != nil && timeValue(old.ExpiresAt).After(now) && old.Status == "complete" {
		return
	}
	state.Status = "running"
	state.NextSyncAt = utc(now.Add(time.Minute))
	entry := autoEntry{Snapshot: state, SeedKey: key}
	if a.repo.save(entry) != nil {
		return
	}
	query := Query{Terms: seed.Terms, Geo: "", Timeframe: "today 12-m"}
	report, err := a.source.GetTrends(ctx, r.Website.ID, r.Website.Site, r.Website.Revision, query)
	// Re-check both authority and the selected keywords after provider work.
	again, resolveErr := a.registry.Resolve(r.Website.ID, r.Website.Site)
	if resolveErr != nil || again.Website.Revision != r.Website.Revision {
		return
	}
	_, newKey, why := a.seed(ctx, again)
	if why != "" || newKey != key {
		entry.Snapshot = automaticEmpty(r.Website, "pending")
		entry.SeedKey = ""
		if why != "" {
			entry.Status = why
		}
		entry.NextSyncAt = utc(a.now().Add(time.Minute))
		entry.UpdatedAt = utc(a.now())
		_ = a.repo.save(entry)
		return
	}
	entry.Status = "failed"
	entry.Reason = "source_unavailable"
	entry.UpdatedAt = utc(a.now())
	entry.NextSyncAt = utc(a.now().Add(15 * time.Minute))
	if err == nil && ValidReport(report, query, a.now()) {
		entry.Report = &report
		entry.ExpiresAt = utc(timeValue(report.ObservedAt).Add(24 * time.Hour))
		entry.Reason = report.Status
		if report.Status == "available" || report.Status == "no_data" {
			entry.Status = "complete"
			entry.Reason = ""
			entry.NextSyncAt = entry.ExpiresAt
		} else {
			entry.ExpiresAt = utc(timeValue(report.ObservedAt).Add(time.Hour))
			if report.Status == "disabled" {
				entry.NextSyncAt = utc(a.now().Add(6 * time.Hour))
			}
			if retry := timeValue(report.RetryAt); retry.After(a.now()) {
				entry.NextSyncAt = utc(retry)
			}
		}
	}
	if ctx.Err() != nil {
		entry.Report = nil
		entry.ExpiresAt = ""
		entry.Reason = "interrupted"
	}
	_ = a.repo.save(entry)
}

var errAutoCommitted = errors.New("Trends snapshot committed but directory sync failed")
