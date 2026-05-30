package crawl

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/temoto/robotstxt"
)

type FetchPageFunc func(context.Context, Item, Spec) (*FetchedPage, error)

type pageOutcome struct {
	item     Item
	page     *FetchedPage
	err      error
	blocked  bool
	blockURL string
}

func Execute(ctx context.Context, store Store, run *Run, spec Spec, fetch FetchPageFunc) error {
	if store == nil {
		return fmt.Errorf("crawl store is not initialized")
	}
	if run == nil || strings.TrimSpace(run.ID) == "" {
		return fmt.Errorf("run is required")
	}
	if fetch == nil {
		return fmt.Errorf("fetch callback is required")
	}

	matcher, err := NewMatcher(spec)
	if err != nil {
		return err
	}

	robots, _ := loadRobotsGroup(ctx, spec.URL)

	// Honour the robots.txt Crawl-delay directive (if any) as the floor for
	// the inter-level delay, so we never crawl faster than the site requests.
	effectiveDelay := spec.Delay
	if robots != nil && robots.CrawlDelay > 0 && robots.CrawlDelay > effectiveDelay {
		effectiveDelay = robots.CrawlDelay
	}

	seedItems, err := buildSeedItems(ctx, spec, matcher)
	if err != nil {
		return err
	}

	seen := make(map[string]Item, len(seedItems))
	currentLevel := make([]Item, 0, len(seedItems))
	for _, item := range seedItems {
		if _, ok := seen[item.URL]; ok {
			continue
		}
		seen[item.URL] = item
		currentLevel = append(currentLevel, item)
	}

	run.Status = StatusRunning
	if spec.ZDRMode {
		run.Spec = sanitizeSpecForStorage(spec)
		run.URL = ""
	} else {
		run.Spec = spec
		run.URL = spec.URL
	}
	run.Queued = len(currentLevel)
	run.Active = 0
	run.Total = len(seen)
	run.UpdatedAt = time.Now().UTC()
	if err := store.SetRun(ctx, run); err != nil {
		return err
	}

	maxConcurrency := spec.MaxConcurrency
	if maxConcurrency <= 0 {
		maxConcurrency = 4
	}

	for len(currentLevel) > 0 {
		if ctx.Err() != nil {
			return markCancelled(ctx, store, run, ctx.Err())
		}
		if run.Completed >= spec.Limit && spec.Limit > 0 {
			break
		}
		if effectiveDelay > 0 {
			timer := time.NewTimer(effectiveDelay)
			select {
			case <-ctx.Done():
				timer.Stop()
				return markCancelled(ctx, store, run, ctx.Err())
			case <-timer.C:
			}
		}

		run.Queued = len(currentLevel)
		run.Active = len(currentLevel)
		run.UpdatedAt = time.Now().UTC()
		if err := store.SetRun(ctx, run); err != nil {
			return err
		}

		resultsCh := make(chan pageOutcome, len(currentLevel))
		sem := make(chan struct{}, maxConcurrency)
		var wg sync.WaitGroup

		for _, item := range currentLevel {
			wg.Add(1)
			sem <- struct{}{}
			go func(item Item) {
				defer wg.Done()
				defer func() { <-sem }()

				if ctx.Err() != nil {
					resultsCh <- pageOutcome{item: item, err: ctx.Err()}
					return
				}

				if !spec.IgnoreRobotsTxt && robots != nil && !allowedByRobots(robots, item.URL) {
					resultsCh <- pageOutcome{item: item, blocked: true, blockURL: item.URL}
					return
				}

				page, fetchErr := fetch(ctx, item, spec)
				resultsCh <- pageOutcome{item: item, page: page, err: fetchErr}
			}(item)
		}

		go func() {
			wg.Wait()
			close(resultsCh)
		}()

		nextLevel := make([]Item, 0)
		for outcome := range resultsCh {
			run.Active--
			run.Queued--

			if outcome.blocked {
				run.Blocked++
				run.UpdatedAt = time.Now().UTC()
				_ = store.AddRobotsBlocked(ctx, run.ID, []string{outcome.blockURL})
				_ = store.SetRun(ctx, run)
				continue
			}

			if outcome.err != nil {
				if ctx.Err() != nil {
					return markCancelled(ctx, store, run, ctx.Err())
				}
				run.Failed++
				run.UpdatedAt = time.Now().UTC()
				_ = store.AppendError(ctx, run.ID, &PageError{
					URL:       outcome.item.URL,
					Code:      "FETCH_ERROR",
					Error:     outcome.err.Error(),
					Timestamp: time.Now().UTC().Format(time.RFC3339),
				})
				_ = store.SetRun(ctx, run)
				continue
			}

			if outcome.page == nil {
				run.Failed++
				run.UpdatedAt = time.Now().UTC()
				_ = store.AppendError(ctx, run.ID, &PageError{
					URL:       outcome.item.URL,
					Code:      "EMPTY_PAGE",
					Error:     "crawl page fetch returned no data",
					Timestamp: time.Now().UTC().Format(time.RFC3339),
				})
				_ = store.SetRun(ctx, run)
				continue
			}

			if spec.Limit <= 0 || run.Completed < spec.Limit {
				document := &Document{
					URL:      outcome.page.URL,
					Metadata: map[string]interface{}{"depth": outcome.item.Depth, "sourceURL": outcome.item.SourceURL},
					Outputs:  outcome.page.Outputs,
				}
				if outcome.page.Metadata != nil {
					for key, value := range outcome.page.Metadata {
						document.Metadata[key] = value
					}
				}
				if err := store.AppendDocument(ctx, run.ID, document); err != nil {
					return err
				}
				run.Completed++
				run.UpdatedAt = time.Now().UTC()
			}

			if spec.Limit > 0 && run.Completed >= spec.Limit {
				_ = store.SetRun(ctx, run)
				continue
			}

			if spec.MaxDiscoveryDepth != nil && outcome.item.Depth >= *spec.MaxDiscoveryDepth {
				_ = store.SetRun(ctx, run)
				continue
			}

			currentURL, parseErr := url.Parse(outcome.page.URL)
			if parseErr != nil {
				_ = store.SetRun(ctx, run)
				continue
			}

			for _, rawLink := range outcome.page.Links {
				canonical, canonicalErr := CanonicalizeURL(rawLink, currentURL, spec.IgnoreQueryParameters)
				if canonicalErr != nil || !matcher.Match(canonical) {
					continue
				}
				if _, ok := seen[canonical]; ok {
					continue
				}
				nextItem := Item{
					URL:       canonical,
					Depth:     outcome.item.Depth + 1,
					SourceURL: outcome.page.URL,
				}
				seen[canonical] = nextItem
				nextLevel = append(nextLevel, nextItem)
			}

			run.Total = len(seen)
			run.UpdatedAt = time.Now().UTC()
			if err := store.SetRun(ctx, run); err != nil {
				return err
			}
		}

		currentLevel = nextLevel
	}

	run.Status = StatusCompleted
	run.Active = 0
	run.Queued = 0
	run.Total = max(run.Total, run.Completed+run.Failed+run.Blocked)
	run.Warning = deriveWarning(run)
	run.UpdatedAt = time.Now().UTC()
	return store.SetRun(ctx, run)
}

func sanitizeSpecForStorage(spec Spec) Spec {
	sanitized := spec
	sanitized.URL = ""
	sanitized.Prompt = ""
	sanitized.PageOptions.Headers = nil
	sanitized.PageOptions.Actions = nil
	sanitized.PageOptions.ProxyURL = ""
	if sanitized.ChangeTracking != nil {
		cloned := *sanitized.ChangeTracking
		cloned.Schema = nil
		cloned.Prompt = ""
		sanitized.ChangeTracking = &cloned
	}
	return sanitized
}

func buildSeedItems(ctx context.Context, spec Spec, matcher *Matcher) ([]Item, error) {
	seeds := make([]string, 0, 8)
	startCanonical, err := CanonicalizeURL(spec.URL, nil, spec.IgnoreQueryParameters)
	if err != nil {
		return nil, err
	}

	if spec.Sitemap != SitemapOnly {
		seeds = append(seeds, startCanonical)
	}
	if spec.Sitemap != SitemapSkip {
		sitemapURLs, sitemapErr := FetchSitemapURLs(ctx, startCanonical)
		if sitemapErr == nil {
			seeds = append(seeds, sitemapURLs...)
		}
	}

	items := make([]Item, 0, len(seeds))
	for _, seed := range seeds {
		if seed != startCanonical && !matcher.Match(seed) {
			continue
		}
		items = append(items, Item{URL: seed, Depth: 0})
	}
	return items, nil
}

func markCancelled(ctx context.Context, store Store, run *Run, cancelErr error) error {
	run.Status = StatusCancelled
	run.Active = 0
	run.Queued = 0
	run.UpdatedAt = time.Now().UTC()
	if err := store.SetRun(ctx, run); err != nil {
		return err
	}
	if cancelErr != nil {
		return cancelErr
	}
	return context.Canceled
}

func loadRobotsGroup(ctx context.Context, rawURL string) (*robotstxt.Group, error) {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return nil, err
	}
	robotsURL := parsed.Scheme + "://" + parsed.Host + "/robots.txt"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, robotsURL, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", "Quarry/1.0")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode >= http.StatusBadRequest {
		return nil, fmt.Errorf("robots.txt returned status %d", resp.StatusCode)
	}

	data, err := robotstxt.FromResponse(resp)
	if err != nil {
		return nil, err
	}
	return data.FindGroup("Quarry/1.0"), nil
}

func allowedByRobots(group *robotstxt.Group, rawURL string) bool {
	if group == nil {
		return true
	}
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return true
	}
	pathWithQuery := parsed.EscapedPath()
	if parsed.RawQuery != "" {
		pathWithQuery += "?" + parsed.RawQuery
	}
	return group.Test(pathWithQuery)
}

func deriveWarning(run *Run) string {
	if run == nil {
		return ""
	}
	if run.Blocked > 0 && run.Status != StatusRunning {
		return "One or more pages were unable to be crawled because robots.txt prevented this. Use /v2/scrape for blocked pages."
	}
	if run.Status == StatusRunning || run.Completed > 1 || run.Spec.CrawlEntireDomain {
		return ""
	}
	parsed, err := url.Parse(run.URL)
	if err != nil {
		return ""
	}
	if normalizePath(parsed.Path) == "/" {
		return ""
	}
	return fmt.Sprintf("Only %d result(s) found. For broader coverage, try crawlEntireDomain=true or start from %s://%s", run.Completed, parsed.Scheme, parsed.Host)
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}
