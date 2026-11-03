package activities

// Sitemap discovery for a crawl (INNH-6a). The orchestrator cannot fetch a sitemap
// safely, so SitemapSeeds asks the edge (POST /v1/internal/crawl/sitemap), which
// reads the sitemap files through its guarded driver chain, checks every URL against
// the crawl's scope and robots.txt, and returns the URLs the crawl may seed.
//
// The activity either returns the URLs (the in-memory frontier keeps them in
// workflow state, so the workflow caps the count) or, given a Queue, enqueues them
// itself in batches of 500 and returns counts only. That keeps a sitemap of tens of
// thousands of URLs out of the workflow history. Re-running the activity on a retry
// sends the same request ids, so the queue does not take a URL twice.

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// FrontierRequestID is the queue request id of a URL in one run's frontier. The
// workflow and SitemapSeeds both derive ids through it, so a URL reached by a link
// and by a sitemap is one queue entry.
func FrontierRequestID(runID, url string) string {
	sum := sha256.Sum256([]byte(runID + "\n" + url))
	return "frontier-" + hex.EncodeToString(sum[:])
}

// SitemapSeedsInput asks for the sitemap URLs of one crawl.
type SitemapSeedsInput struct {
	RunID string `json:"run_id"`
	OrgID string `json:"org_id"`
	// Seed is the crawl's seed URL: the origin whose robots.txt and sitemaps are read.
	Seed string `json:"seed"`
	// Crawl is the crawl's scope context, as run_page takes it.
	Crawl *CrawlContext `json:"crawl"`
	// MaxURLs is the most URLs to return or enqueue (the crawl's page bound).
	MaxURLs uint32 `json:"max_urls"`
	// Queue, when set, is the durable frontier queue the activity enqueues into at Depth.
	// Empty: the URLs come back in the result.
	Queue string `json:"queue,omitempty"`
	Depth uint32 `json:"depth,omitempty"`
}

// SitemapSkipped is what the edge left out, by reason. Counts only.
type SitemapSkipped struct {
	OutOfScope          uint32 `json:"out_of_scope"`
	RobotsDisallowed    uint32 `json:"robots_disallowed"`
	Duplicate           uint32 `json:"duplicate"`
	Cap                 uint32 `json:"cap"`
	Invalid             uint32 `json:"invalid"`
	SitemapsRefused     uint32 `json:"sitemaps_refused"`
	SitemapsUnavailable uint32 `json:"sitemaps_unavailable"`
}

// SitemapSeedsResult is the discovery summary. URLs is set only when no Queue was given.
type SitemapSeedsResult struct {
	URLs            []string       `json:"urls,omitempty"`
	Discovered      uint32         `json:"discovered"`
	SitemapsFetched uint32         `json:"sitemaps_fetched"`
	NestedFetched   uint32         `json:"nested_fetched"`
	Seeded          uint32         `json:"seeded"`
	Skipped         SitemapSkipped `json:"skipped"`
	Truncated       bool           `json:"truncated,omitempty"`
}

// edgeSitemapResponse is the edge's answer: SitemapResponse in crawl_sitemap.rs.
type edgeSitemapResponse struct {
	URLs            []string       `json:"urls"`
	Discovered      uint32         `json:"discovered"`
	SitemapsFetched uint32         `json:"sitemaps_fetched"`
	NestedFetched   uint32         `json:"nested_fetched"`
	Skipped         SitemapSkipped `json:"skipped"`
	Truncated       bool           `json:"truncated"`
}

// SitemapSeeds discovers the sitemap URLs of a crawl through the edge.
func (a *Activities) SitemapSeeds(ctx context.Context, in SitemapSeedsInput) (SitemapSeedsResult, error) {
	const op = "activities.SitemapSeeds"
	if in.Crawl == nil {
		return SitemapSeedsResult{}, errs.New(errs.CategoryValidation, op, errors.New("sitemap discovery needs the crawl scope")).Temporal()
	}
	body, err := json.Marshal(map[string]any{
		"url":      in.Seed,
		"org_id":   in.OrgID,
		"crawl":    in.Crawl,
		"max_urls": in.MaxURLs,
	})
	if err != nil {
		return SitemapSeedsResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	url := strings.TrimRight(a.cfg.RuntimeBaseURL, "/") + "/v1/internal/crawl/sitemap"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return SitemapSeedsResult{}, errs.New(errs.CategoryValidation, op, err).Temporal()
	}
	req.Header.Set("Content-Type", "application/json")
	setAuth(req, a.cfg.RuntimeAuthToken)
	if a.cfg.EdgeRunSecret != "" {
		req.Header.Set("X-Quarry-Run-Sig", runBindingSig(a.cfg.EdgeRunSecret, in.OrgID, in.Seed))
	}
	resp, err := a.http.Do(req)
	if err != nil {
		if errors.Is(err, context.Canceled) {
			return SitemapSeedsResult{}, err
		}
		cat := errs.CategoryNetwork
		if errors.Is(err, context.DeadlineExceeded) {
			cat = errs.CategoryTimeout
		}
		return SitemapSeedsResult{}, errs.New(cat, op, err).Temporal()
	}
	defer resp.Body.Close()
	respBody, _ := io.ReadAll(resp.Body)
	if resp.StatusCode == http.StatusNotImplemented {
		// The edge serves the route only with QUARRY_EDGE__CRAWL_SITEMAP on. Retrying
		// does not turn the flag on.
		return SitemapSeedsResult{}, errs.New(errs.CategoryValidation, op, errors.New("the edge does not serve sitemap discovery (501)")).Temporal()
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return SitemapSeedsResult{}, errs.FromHTTPResponse(op, resp.StatusCode, respBody).Temporal()
	}
	var edge edgeSitemapResponse
	if err := json.Unmarshal(respBody, &edge); err != nil {
		return SitemapSeedsResult{}, errs.New(errs.CategoryValidation, op, fmt.Errorf("decode sitemap response: %w", err)).Temporal()
	}

	// The edge never returns more than MaxURLs; a response that does is cut here.
	urls := edge.URLs
	if in.MaxURLs > 0 && uint32(len(urls)) > in.MaxURLs {
		urls = urls[:in.MaxURLs]
	}
	out := SitemapSeedsResult{
		Discovered:      edge.Discovered,
		SitemapsFetched: edge.SitemapsFetched,
		NestedFetched:   edge.NestedFetched,
		Seeded:          uint32(len(urls)),
		Skipped:         edge.Skipped,
		Truncated:       edge.Truncated,
	}
	if in.Queue == "" {
		out.URLs = urls
		return out, nil
	}
	items := make([]FrontierBatchItem, 0, len(urls))
	for _, u := range urls {
		if strings.TrimSpace(u) == "" {
			continue
		}
		items = append(items, FrontierBatchItem{RequestID: FrontierRequestID(in.RunID, u), URL: u, Depth: in.Depth})
	}
	if len(items) > 0 {
		if _, err := a.FrontierEnqueueBatch(ctx, FrontierEnqueueBatchInput{Queue: in.Queue, OrgID: in.OrgID, Items: items}); err != nil {
			return SitemapSeedsResult{}, err
		}
	}
	return out, nil
}
