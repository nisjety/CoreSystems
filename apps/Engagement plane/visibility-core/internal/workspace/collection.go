package workspace

import (
	"context"
	"sort"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/collection"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

// CollectionInventory reads existing owner projections only. Never substitute
// uncached adapters here: opening this view must not trigger reporting calls.
func (s *Service) CollectionInventory(ctx context.Context, id, site, quarter string) (collection.Inventory, error) {
	r, err := s.registry.Resolve(id, site)
	if err != nil {
		return collection.Inventory{}, err
	}
	if _, err = pulse.QuarterPeriod(quarter, time.Now()); err != nil {
		return collection.Inventory{}, websites.ErrInvalid
	}
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	now := time.Now().UTC()
	out := collection.Inventory{WebsiteID: id, Site: site, WebsiteRevision: r.Website.Revision, Source: "visibility_collection_inventory", AsOf: now.Format(time.RFC3339Nano), Quarter: quarter, Datasets: []collection.Dataset{}}
	base := func(key, provider, category, coverage, unit, history, mode string) collection.Dataset {
		return collection.Dataset{Key: key, Provider: provider, Category: category, Coverage: coverage, Unit: unit, History: history, CollectionMode: mode, Partial: true, Status: "setup", Reason: "setup_required"}
	}
	var mu sync.Mutex
	var wg sync.WaitGroup
	var access map[string]string
	add := func(rows ...collection.Dataset) { mu.Lock(); out.Datasets = append(out.Datasets, rows...); mu.Unlock() }
	run := func(fn func()) { wg.Add(1); go func() { defer wg.Done(); fn() }() }
	run(func() { access = s.SourceStatuses(ctx, r.Website) })
	run(func() {
		g := base("gsc", "google_search_console", "account", "sampled_queries", "query_rows", "latest_snapshot", "scheduled")
		a := base("ga4", "google_analytics", "account", "host_filtered", "reporting_days", "latest_snapshot", "scheduled")
		c := base("clarity", "microsoft_clarity", "account", "project_aggregate", "reports", "latest_snapshot", "scheduled")
		if s.analytics != nil {
			state, e := s.analytics.Get(id, site, quarter)
			bundle, re := s.analytics.Read(ctx, id, site, quarter)
			if e == nil && re == nil {
				g.ReportingResource = bundle.Search.ReportingProperty
				a.ReportingResource = bundle.GA4.ReportingSite
				g.Period = &collection.Period{StartDate: bundle.Search.Period.StartDate, EndDate: bundle.Search.Period.EndDate}
				a.Period = &collection.Period{StartDate: bundle.GA4.Period.StartDate, EndDate: bundle.GA4.Period.EndDate}
				g = collection.Finish(g, bundle.Search.Status, bundle.Search.Reason, bundle.Search.UpdatedAt, collection.N(len(bundle.Search.Queries)), 6*time.Hour, now)
				a = collection.Finish(a, bundle.GA4.Status, bundle.GA4.Reason, bundle.GA4.UpdatedAt, collection.N(len(bundle.GA4.Days)), 6*time.Hour, now)
				c = collection.Finish(c, bundle.Clarity.Status, bundle.Clarity.Reason, bundle.Clarity.UpdatedAt, nil, 6*time.Hour, now)
				for _, step := range state.Steps {
					var d *collection.Dataset
					switch step.Key {
					case "gsc":
						d = &g
					case "ga4":
						d = &a
					case "clarity":
						d = &c
					}
					if d == nil {
						continue
					}
					d.LastAttemptAt = step.ObservedAt
					d.NextSyncAt = state.NextSyncAt
					if d.LastSuccessAt == "" {
						d.LastSuccessAt = step.LastSuccessAt
					}
					if step.Status == "failed" && d.Status != "reconnect" {
						d.Status = "failed"
						d.Reason = "source_unavailable"
					}
				}
				if !state.Enabled {
					g.CollectionMode = "paused"
					a.CollectionMode = "paused"
					c.CollectionMode = "paused"
				}
			} else {
				g.Status = "failed"
				a.Status = "failed"
				c.Status = "failed"
				g.Reason = "source_unavailable"
				a.Reason = g.Reason
				c.Reason = g.Reason
			}
		}
		add(g, a, c)
	})
	run(func() {
		for _, strategy := range []string{"mobile", "desktop"} {
			d := base("lighthouse_"+strategy, "google_pagespeed", "public", "lab_single_page", "reports", "latest_snapshot", "scheduled")
			if s.labReports != nil {
				v := s.labReports.Get(ctx, site, strategy)
				observed := ""
				var n *int
				if v.Report != nil {
					observed = v.Report.FetchTime
					n = collection.N(1)
					d.ReportingResource = v.Report.ReportingOrigin
				}
				d = collection.Finish(d, v.Status, v.Reason, observed, n, 24*time.Hour, now)
				if v.Cache != nil {
					d.LastAttemptAt = v.Cache.LastAttemptAt
					d.NextSyncAt = v.Cache.NextRefreshAt
					if d.LastSuccessAt == "" {
						d.LastSuccessAt = v.Cache.LastSuccessAt
					}
					if v.Cache.RefreshStatus == "failed" {
						d.Status = "failed"
						d.Reason = "source_unavailable"
					}
				}
			}
			add(d)
		}
	})
	run(func() {
		m := base("crux_mobile", "chrome_ux_report", "public", "origin_field", "reports", "latest_snapshot", "scheduled")
		d := m
		d.Key = "crux_desktop"
		cc := base("common_crawl", "common_crawl", "public", "archive_sample", "captures", "latest_snapshot", "scheduled")
		if s.publicSignals != nil {
			v, e := s.publicSignals.Get(id, site)
			if e == nil {
				for _, row := range []*collection.Dataset{&m, &d, &cc} {
					row.LastAttemptAt = v.StartedAt
					row.NextSyncAt = v.NextSyncAt
					row.Status = "pending"
					row.Reason = ""
					if !v.Enabled {
						row.CollectionMode = "paused"
					}
					if v.Status == "failed" {
						row.Status = "failed"
						row.Reason = "source_unavailable"
					}
					if v.Report == nil && v.ExpiresAt != "" && v.Status == "complete" {
						row.Status = "stale"
						row.Reason = "expired"
					}
				}
				if v.Report != nil {
					for _, cr := range v.Report.CrUX {
						row := &m
						if cr.FormFactor == "DESKTOP" {
							row = &d
						}
						*row = collection.Finish(*row, cr.Status, cr.Reason, v.Report.ObservedAt, nil, 24*time.Hour, now)
						row.ReportingResource = cr.ReportingOrigin
						if cr.Period != nil {
							row.Period = &collection.Period{StartDate: cr.Period.StartDate, EndDate: cr.Period.EndDate}
						}
					}
					c := v.Report.CommonCrawl
					cc = collection.Finish(cc, c.Status, c.Reason, v.Report.ObservedAt, collection.N(len(c.Captures)), 24*time.Hour, now)
					cc.ReportingResource = c.ReportingHost
				}
			}
		}
		add(m, d, cc)
	})
	run(func() {
		d := base("site_audit", "quarry_v2", "public", "bounded_crawl", "pages", "bounded_audits", "manual")
		if s.audit != nil {
			v, e := s.audit.Get(id, site)
			if e == nil {
				n := 0
				for _, p := range v.Pages {
					if p.Status == "checked" {
						n++
					}
				}
				d = collection.Finish(d, v.Status, v.Reason, v.CompletedAt, collection.N(n), 24*time.Hour, now)
				d.LastAttemptAt = v.StartedAt
				if s.monitoring != nil {
					m, e := s.monitoring.Get(id, site)
					if e == nil && m.Enabled {
						d.CollectionMode = "scheduled"
						d.NextSyncAt = m.NextRunAt
					}
				}
			}
		}
		add(d)
	})
	run(func() {
		d := base("search_history", "google_search_console", "account", "rolling_queries", "query_page_dates", "rolling_90_days", "scheduled")
		if s.research != nil {
			v, e := s.research.Get(ctx, id, site)
			if e == nil {
				d = collection.Finish(d, v.Status, v.Reason, v.ObservedAt, collection.N(len(v.Rows)), 24*time.Hour, now)
				d.LastAttemptAt = v.LastAttemptAt
				d.NextSyncAt = v.NextRunAt
				d.Period = &collection.Period{StartDate: v.Period.StartDate, EndDate: v.Period.EndDate}
			} else {
				d.Status = "failed"
				d.Reason = "source_unavailable"
			}
		}
		add(d)
	})
	run(func() {
		d := base("trends", "google_trends_pytrends", "market", "relative_interest", "terms", "latest_snapshot", "scheduled")
		if s.automaticTrends != nil {
			v, e := s.automaticTrends.Get(ctx, id, site)
			if e == nil {
				observed := ""
				var n *int
				if v.Report != nil {
					observed = v.Report.ObservedAt
					n = collection.N(len(v.Report.Terms))
				}
				d = collection.Finish(d, v.Status, v.Reason, observed, n, 24*time.Hour, now)
				d.NextSyncAt = v.NextSyncAt
			}
		}
		add(d)
	})
	run(func() {
		for _, provider := range []string{"algolia", "cookiebot"} {
			keys := []string{"algolia", "algolia_terms", "algolia_no_results"}
			coverage, unit := "index_aggregate", "reporting_days"
			if provider == "cookiebot" {
				keys = []string{"cookiebot_consents", "cookiebot_inventory"}
				coverage, unit = "consent_aggregate", "reporting_days"
			}
			rows := make([]collection.Dataset, len(keys))
			for i, key := range keys {
				rows[i] = base(key, provider, "account", coverage, unit, "latest_snapshot", "source_managed")
			}
			if s.siteSources != nil {
				v, e := s.siteSources.GetContext(ctx, id, site, provider)
				if e != nil {
					for i := range rows {
						rows[i].Status = "failed"
						rows[i].Reason = "source_unavailable"
					}
				} else {
					for i := range rows {
						status := v.Status
						var n *int
						if v.Algolia != nil {
							rows[i].ReportingResource = v.Algolia.IndexName
							switch i {
							case 0:
								status = v.Algolia.Analytics.Status
								if status == "connected" {
									n = collection.N(v.Algolia.Analytics.ReportedDays)
								}
							case 1:
								rows[i].Unit = "query_rows"
								if v.Algolia.TopSearches != nil {
									status = v.Algolia.TopSearches.Status
									n = collection.N(len(v.Algolia.TopSearches.Rows))
								} else {
									status = "not_loaded"
								}
							case 2:
								rows[i].Unit = "query_rows"
								if v.Algolia.NoResultTerms != nil {
									status = v.Algolia.NoResultTerms.Status
									n = collection.N(len(v.Algolia.NoResultTerms.Rows))
								} else {
									status = "not_loaded"
								}
							}
						}
						if v.Cookiebot != nil {
							rows[i].ReportingResource = v.Cookiebot.ReportingDomain
							if i == 0 {
								status = v.Cookiebot.Consents.Status
								if status == "connected" {
									n = collection.N(v.Cookiebot.Consents.ReportedDays)
								}
							} else {
								status = v.Cookiebot.Inventory.Status
								rows[i].Unit = "cookies"
								if v.Cookiebot.Inventory.Total != nil {
									n = collection.N(int(*v.Cookiebot.Inventory.Total))
								}
							}
						}
						rows[i] = collection.Finish(rows[i], status, "", v.ObservedAt, n, 6*time.Hour, now)
						rows[i].Period = &collection.Period{StartDate: v.Window.StartDate, EndDate: v.Window.EndDate}
					}
				}
			}
			add(rows...)
		}
	})
	run(func() {
		for _, environment := range []string{"development", "production"} {
			d := base("web_vitals_"+environment, "first_party_web_vitals", "first_party", "browser_samples", "samples", "retained_samples", "manual")
			if s.webVitals != nil {
				v, e := s.webVitals.Get(id, site, environment)
				if e == nil {
					d = collection.Finish(d, v.Status, "", v.LastCollectedAt, collection.N(v.SampleCount), 0, now)
					d.Period = &collection.Period{StartDate: v.Window.StartDate, EndDate: v.Window.EndDate}
					d.ReportingResource = v.ReportingOrigin
				}
			}
			add(d)
		}
	})
	run(func() {
		d := base("competitor_pages", "quarry_v2", "public", "selected_public_pairs", "comparisons", "bounded_comparisons", "scheduled")
		if s.competitors != nil {
			v, e := s.competitors.Get(id, site)
			if e != nil {
				d.Status = "failed"
				d.Reason = "source_unavailable"
			} else {
				n := 0
				latest := ""
				failed := false
				for _, entry := range v.Entries {
					if entry.LastAttemptAt > d.LastAttemptAt {
						d.LastAttemptAt = entry.LastAttemptAt
					}
					if d.NextSyncAt == "" || entry.NextRunAt < d.NextSyncAt {
						d.NextSyncAt = entry.NextRunAt
					}
					failed = failed || entry.Status == "failed"
					if entry.Own != nil && entry.Competitor != nil {
						n++
						at := entry.Own.ObservedAt
						if entry.Competitor.ObservedAt > at {
							at = entry.Competitor.ObservedAt
						}
						if at > latest {
							latest = at
						}
					}
				}
				d.Status = "pending"
				d.Reason = ""
				if n > 0 {
					d = collection.Finish(d, "complete", "", latest, collection.N(n), 24*time.Hour, now)
				}
				if failed {
					d.Status = "failed"
					d.Reason = "source_unavailable"
				}
			}
		}
		add(d)
	})
	wg.Wait()
	for i := range out.Datasets {
		d := &out.Datasets[i]
		if d.Status != "pending" {
			continue
		}
		key := d.Key
		switch key {
		case "gsc":
			key = "search_console"
		case "algolia_terms", "algolia_no_results":
			key = "algolia"
		case "cookiebot_consents", "cookiebot_inventory":
			key = "cookiebot"
		case "web_vitals_development", "web_vitals_production":
			key = "web_vitals"
		}
		status, known := access[key]
		if !known {
			switch key {
			case "search_console":
				status = r.Website.Integrations.SearchConsole.Status
				known = true
			case "ga4":
				status = r.Website.Integrations.GA4.Status
				known = true
			case "clarity":
				status = r.Website.Integrations.Clarity.Status
				known = true
			}
		}
		if known {
			switch status {
			case "authorization_required", "requires_consent":
				d.Status = "reconnect"
				d.Reason = "authorization_required"
			case "source_unavailable":
				d.Status = "failed"
				d.Reason = "source_unavailable"
			case "connected", "available":
			default:
				d.Status = "setup"
				d.Reason = "setup_required"
			}
		}
	}
	for _, key := range []string{"market_keyword_volume", "competitor_rankings", "backlink_index"} {
		d := base(key, "external_source", "market", "external_source_required", "records", "none", "unconfigured")
		d.Status = "unsupported"
		d.Reason = "additional_source_required"
		add(d)
	}
	current, err := s.registry.Resolve(id, site)
	if err != nil {
		return collection.Inventory{}, err
	}
	if current.Website.Revision != r.Website.Revision {
		return collection.Inventory{}, websites.ErrConflict
	}
	if ctx.Err() != nil {
		return collection.Inventory{}, ctx.Err()
	}
	// Stable keys keep the same dataset in the same row across concurrent reads.
	sort.Slice(out.Datasets, func(i, j int) bool { return out.Datasets[i].Key < out.Datasets[j].Key })
	out.Summary = collection.Count(out.Datasets)
	return out, nil
}
