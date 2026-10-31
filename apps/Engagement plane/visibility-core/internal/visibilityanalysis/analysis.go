// Package visibilityanalysis derives transparent SEO views from saved evidence.
// It never fetches providers or infers missing measurements.
package visibilityanalysis

import (
	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"math"
	"net/url"
	"sort"
	"strings"
	"time"
)

type Row struct {
	URL         string         `json:"url"`
	Title       string         `json:"title"`
	Description string         `json:"description"`
	Canonical   string         `json:"canonical"`
	AuditStatus string         `json:"auditStatus"`
	HTTPStatus  *int           `json:"httpStatus"`
	ObservedAt  string         `json:"observedAt"`
	Clicks      *int64         `json:"clicks"`
	Impressions *int64         `json:"impressions"`
	CTR         *float64       `json:"ctr"`
	Position    *float64       `json:"position"`
	Issues      *int           `json:"issues"`
	Inbound     int            `json:"inbound"`
	Outbound    *int           `json:"outbound"`
	Depth       *int           `json:"depth"`
	Discovered  bool           `json:"discovered"`
	SEO         *siteaudit.SEO `json:"seo,omitempty"`
}
type Item struct {
	Key         string   `json:"key"`
	URL         string   `json:"url,omitempty"`
	Clicks      *int64   `json:"clicks,omitempty"`
	Impressions *int64   `json:"impressions,omitempty"`
	CTR         *float64 `json:"ctr,omitempty"`
	Position    *float64 `json:"position,omitempty"`
}
type Group struct {
	Key    string `json:"key"`
	Entity string `json:"entity"`
	Items  []Item `json:"items"`
}
type Edge struct {
	From   string `json:"from"`
	To     string `json:"to"`
	Status string `json:"status"`
}
type Coverage struct {
	SearchStatus    string `json:"searchStatus"`
	SearchAt        string `json:"searchAt"`
	SearchTruncated bool   `json:"searchTruncated"`
	AuditStatus     string `json:"auditStatus"`
	AuditAt         string `json:"auditAt"`
	AuditID         string `json:"auditId"`
	Checked         int    `json:"checked"`
	Failed          int    `json:"failed"`
	DiscoveryStatus string `json:"discoveryStatus"`
	Partial         bool   `json:"partial"`
	LinkLimit       int    `json:"linkLimit"`
}
type FindingRef struct {
	ID       string `json:"id"`
	RuleID   string `json:"ruleId"`
	PageURL  string `json:"pageUrl"`
	Severity string `json:"severity"`
}
type Snapshot struct {
	Findings        []FindingRef `json:"findings"`
	WebsiteID       string       `json:"websiteId"`
	Site            string       `json:"site"`
	WebsiteRevision int64        `json:"websiteRevision"`
	Source          string       `json:"source"`
	Period          pulse.Period `json:"period"`
	GeneratedAt     string       `json:"generatedAt"`
	Coverage        Coverage     `json:"coverage"`
	Pages           []Row        `json:"pages"`
	Groups          []Group      `json:"groups"`
	Edges           []Edge       `json:"edges"`
}

func ptr[T any](v T) *T { return &v }

// Key joins only an exact host/www alias and exact escaped path/query. It does
// not merge different paths, query strings, HTTP and HTTPS, or other subdomains.
func Key(site, raw string) string {
	if !discovery.SafeURL(site, raw) {
		return ""
	}
	s, _ := url.Parse(site)
	u, err := url.Parse(raw)
	if err != nil {
		return ""
	}
	u.Host = s.Host
	u.Fragment = ""
	if u.Path == "" {
		u.Path = "/"
	}
	return u.String()
}
func Build(w websites.Website, search pulse.SearchProjection, audit siteaudit.Snapshot, inventory discovery.Snapshot, now time.Time) Snapshot {
	v := Snapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Source: "visibility_analysis", Period: search.Period, GeneratedAt: now.UTC().Format(time.RFC3339), Pages: []Row{}, Groups: []Group{}, Edges: []Edge{}, Coverage: Coverage{SearchStatus: search.Status, SearchAt: search.UpdatedAt, SearchTruncated: search.PagesAtLimit || search.QueriesAtLimit, AuditStatus: audit.Status, AuditAt: audit.CompletedAt, AuditID: audit.ID, DiscoveryStatus: inventory.Status, Partial: true, LinkLimit: 100}}
	v.Findings = []FindingRef{}
	for _, f := range audit.Findings {
		v.Findings = append(v.Findings, FindingRef{f.ID, f.RuleID, f.PageURL, f.Severity})
	}
	rows := map[string]*Row{}
	ensure := func(raw string) *Row {
		k := Key(w.Site, raw)
		if k == "" {
			return nil
		}
		if rows[k] == nil {
			rows[k] = &Row{URL: k, AuditStatus: "not_checked"}
		}
		return rows[k]
	}
	for _, p := range inventory.Pages {
		if row := ensure(p.URL); row != nil {
			row.Discovered = true
			row.Title = p.Title
		}
	}
	// Weighted averages when the property returns both exact www host variants.
	invalidMetrics := map[string]bool{}
	if search.Status == "connected" {
		for _, p := range search.Pages {
			if row := ensure(p.Key); row != nil {
				if invalidMetrics[row.URL] {
					continue
				}
				oldI := int64(0)
				oldC := int64(0)
				weighted := float64(0)
				if row.Impressions != nil {
					oldI = *row.Impressions
					oldC = *row.Clicks
					weighted = *row.Position
				}
				total := oldI + p.Impressions
				clicks := oldC + p.Clicks
				ctr := float64(0)
				position := float64(0)
				if total > 0 {
					ctr = float64(clicks) / float64(total)
					position = weighted*(float64(oldI)/float64(total)) + p.Position*(float64(p.Impressions)/float64(total))
				}
				if total > 9007199254740991 || clicks > 9007199254740991 || math.IsNaN(position) || math.IsInf(position, 0) {
					invalidMetrics[row.URL] = true
					row.Clicks = nil
					row.Impressions = nil
					row.CTR = nil
					row.Position = nil
					v.Coverage.SearchTruncated = true
					continue
				}
				row.Clicks = ptr(clicks)
				row.Impressions = ptr(total)
				row.CTR = ptr(ctr)
				row.Position = ptr(position)
			}
		}
	}
	aliases := map[string]string{}
	checked := map[string]siteaudit.Page{}
	// Exact host aliases may appear twice in one audit. Use the newest
	// observation for the joined row and count each joined page only once.
	latest := map[string]int{}
	observed := func(p siteaudit.Page) time.Time {
		at := p.ObservedAt
		if at == "" {
			at = audit.CompletedAt
		}
		t, _ := time.Parse(time.RFC3339Nano, at)
		return t
	}
	for i, p := range audit.Pages {
		k := Key(w.Site, p.URL)
		if old, ok := latest[k]; !ok || !observed(p).Before(observed(audit.Pages[old])) {
			latest[k] = i
		}
	}
	for i, p := range audit.Pages {
		if latest[Key(w.Site, p.URL)] != i {
			continue
		}
		row := ensure(p.URL)
		if row == nil {
			continue
		}
		row.AuditStatus = p.Status
		if p.Status != "checked" {
			v.Coverage.Failed++
			continue
		}
		v.Coverage.Checked++
		row.Title = p.Title
		row.Description = p.Description
		row.Canonical = p.Canonical
		row.HTTPStatus = ptr(p.HTTPStatus)
		row.ObservedAt = p.ObservedAt
		row.SEO = siteaudit.CloneSEO(p.SEO)
		issues := 0
		for _, c := range p.Checks {
			if c.Status == "failed" || c.Status == "review" {
				issues++
			}
		}
		row.Issues = ptr(issues)
		final := Key(w.Site, p.FinalURL)
		if final != "" {
			aliases[final] = row.URL
		}
		checked[row.URL] = p
	}
	resolve := func(raw string) string {
		k := Key(w.Site, raw)
		if a := aliases[k]; a != "" {
			return a
		}
		return k
	}
	seen := map[string]bool{}
	for from, p := range checked {
		out := map[string]bool{}
		for _, raw := range p.Links {
			to := resolve(raw)
			if to == "" {
				continue
			}
			out[to] = true
			key := from + "\n" + to
			if seen[key] {
				continue
			}
			seen[key] = true
			status := "not_checked"
			if target := rows[to]; target != nil {
				target.Inbound++
				if target.HTTPStatus != nil {
					status = "checked"
					if *target.HTTPStatus == 404 || *target.HTTPStatus == 410 {
						status = "broken"
					}
				}
			}
			v.Edges = append(v.Edges, Edge{From: from, To: to, Status: status})
		}
		rows[from].Outbound = ptr(len(out))
	}
	root := resolve(w.Site)
	if p := rows[root]; p != nil {
		p.Depth = ptr(0)
		queue := []string{root}
		for len(queue) > 0 {
			from := queue[0]
			queue = queue[1:]
			for _, e := range v.Edges {
				if e.From != from {
					continue
				}
				if target := rows[e.To]; target != nil && target.Depth == nil {
					target.Depth = ptr(*rows[from].Depth + 1)
					queue = append(queue, e.To)
				}
			}
		}
	}
	groups := []Group{{Key: "near_page_one", Entity: "query", Items: []Item{}}, {Key: "low_ctr", Entity: "query", Items: []Item{}}, {Key: "technical_review", Entity: "page", Items: []Item{}}, {Key: "few_incoming_links", Entity: "page", Items: []Item{}}}
	if search.Status == "connected" {
		for _, q := range search.Queries {
			item := Item{Key: q.Key, Clicks: ptr(q.Clicks), Impressions: ptr(q.Impressions), CTR: ptr(q.CTR), Position: ptr(q.Position)}
			if q.Impressions >= 20 && q.Position > 10 && q.Position <= 20 {
				groups[0].Items = append(groups[0].Items, item)
			}
			if q.Impressions >= 100 && q.Position >= 1 && q.Position <= 10 && q.CTR < .02 {
				groups[1].Items = append(groups[1].Items, item)
			}
		}
	}
	for _, r := range rows {
		if r.Issues != nil && *r.Issues > 0 {
			groups[2].Items = append(groups[2].Items, Item{Key: r.URL, URL: r.URL})
		}
		if r.URL != root && r.Inbound <= 1 && v.Coverage.Checked > 0 {
			groups[3].Items = append(groups[3].Items, Item{Key: r.URL, URL: r.URL})
		}
		v.Pages = append(v.Pages, *r)
	}
	sort.Slice(v.Pages, func(i, j int) bool { return strings.Compare(v.Pages[i].URL, v.Pages[j].URL) < 0 })
	sort.Slice(v.Edges, func(i, j int) bool { return v.Edges[i].From+v.Edges[i].To < v.Edges[j].From+v.Edges[j].To })
	for i := range groups {
		sort.Slice(groups[i].Items, func(a, b int) bool { return groups[i].Items[a].Key < groups[i].Items[b].Key })
	}
	v.Groups = groups
	return v
}
