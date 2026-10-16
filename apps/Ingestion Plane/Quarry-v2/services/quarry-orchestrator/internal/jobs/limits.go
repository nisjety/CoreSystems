package jobs

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

// Crawl bounds. A crawl with no page limit used to run until the 30-minute
// workflow timeout ended it, and a request of `max_pages: 0` meant "unlimited".
// Every crawl now has a finite page, depth and failure bound: the request's
// value when it is in range, the default when it is absent or zero, the hard
// ceiling when it is above it. The resolved values are what the workflow runs
// with, so a legacy job row (written before this existed, possibly with a zero
// or a huge value) is bounded too.
const (
	envCrawlDefaultMaxPages = "QUARRY_CRAWL_DEFAULT_MAX_PAGES"
	envCrawlHardMaxPages    = "QUARRY_CRAWL_HARD_MAX_PAGES"
	envCrawlDefaultMaxDepth = "QUARRY_CRAWL_DEFAULT_MAX_DEPTH"
	envCrawlHardMaxDepth    = "QUARRY_CRAWL_HARD_MAX_DEPTH"

	defaultCrawlMaxPages uint32 = 1000
	hardCrawlMaxPages    uint32 = 5000
	defaultCrawlMaxDepth uint32 = 5
	hardCrawlMaxDepth    uint32 = 10
)

// Why a resolved value is what it is.
const (
	// ByNone: the request's own value was used unchanged.
	ByNone = "none"
	// ByDefault: the request was absent or zero and took the default.
	ByDefault = "default"
	// ByHard: the request was above the ceiling and was clamped to it.
	ByHard = "hard"
)

// CrawlLimits are the operator-set bounds. The hard ceilings are never
// disabled; a zero DefaultPages or DefaultDepth means "no default", so an
// absent request takes the hard ceiling instead.
type CrawlLimits struct {
	DefaultPages uint32
	HardPages    uint32
	DefaultDepth uint32
	HardDepth    uint32
}

// DefaultCrawlLimits returns the built-in bounds (1000 pages, depth 5; hard
// ceilings 5000 pages, depth 10).
func DefaultCrawlLimits() CrawlLimits {
	return CrawlLimits{
		DefaultPages: defaultCrawlMaxPages,
		HardPages:    hardCrawlMaxPages,
		DefaultDepth: defaultCrawlMaxDepth,
		HardDepth:    hardCrawlMaxDepth,
	}
}

// LoadCrawlLimits reads the bounds from the environment. An unset variable
// keeps the built-in value; an unparsable value, a zero hard ceiling or a
// default above its ceiling is an error naming the variable, so the process
// refuses to start with an ambiguous bound rather than guessing.
func LoadCrawlLimits() (CrawlLimits, error) {
	return loadCrawlLimits(os.Getenv)
}

func loadCrawlLimits(getenv func(string) string) (CrawlLimits, error) {
	limits := DefaultCrawlLimits()
	for _, field := range []struct {
		env  string
		dest *uint32
	}{
		{envCrawlDefaultMaxPages, &limits.DefaultPages},
		{envCrawlHardMaxPages, &limits.HardPages},
		{envCrawlDefaultMaxDepth, &limits.DefaultDepth},
		{envCrawlHardMaxDepth, &limits.HardDepth},
	} {
		raw := strings.TrimSpace(getenv(field.env))
		if raw == "" {
			continue
		}
		value, err := strconv.ParseUint(raw, 10, 32)
		if err != nil {
			return CrawlLimits{}, fmt.Errorf("%s=%q is not a non-negative integer: %w", field.env, raw, err)
		}
		*field.dest = uint32(value)
	}
	if limits.HardPages == 0 {
		return CrawlLimits{}, fmt.Errorf("%s must be greater than zero: the hard page ceiling cannot be disabled", envCrawlHardMaxPages)
	}
	if limits.HardDepth == 0 {
		return CrawlLimits{}, fmt.Errorf("%s must be greater than zero: the hard depth ceiling cannot be disabled", envCrawlHardMaxDepth)
	}
	if limits.DefaultPages > limits.HardPages {
		return CrawlLimits{}, fmt.Errorf("%s (%d) exceeds %s (%d)", envCrawlDefaultMaxPages, limits.DefaultPages, envCrawlHardMaxPages, limits.HardPages)
	}
	if limits.DefaultDepth > limits.HardDepth {
		return CrawlLimits{}, fmt.Errorf("%s (%d) exceeds %s (%d)", envCrawlDefaultMaxDepth, limits.DefaultDepth, envCrawlHardMaxDepth, limits.HardDepth)
	}
	return limits, nil
}

// Resolved is the outcome of applying the bounds to one request.
type Resolved struct {
	Pages   uint32
	Depth   uint32
	PagesBy string
	DepthBy string
}

// Resolve applies the bounds. A request of zero counts as absent and takes the
// default (or the hard ceiling when the default is disabled); a request above
// the ceiling is clamped to it; anything else is used as asked. The result is
// never zero, so a crawl is never unbounded.
func (l CrawlLimits) Resolve(reqPages, reqDepth uint32) Resolved {
	pages, pagesBy := resolveBound(reqPages, l.DefaultPages, l.HardPages)
	depth, depthBy := resolveBound(reqDepth, l.DefaultDepth, l.HardDepth)
	return Resolved{Pages: pages, Depth: depth, PagesBy: pagesBy, DepthBy: depthBy}
}

func resolveBound(requested, def, hard uint32) (uint32, string) {
	switch {
	case requested == 0:
		if def == 0 {
			return hard, ByDefault
		}
		return def, ByDefault
	case requested > hard:
		return hard, ByHard
	default:
		return requested, ByNone
	}
}

// LoadClaimBatch reads QUARRY_JOB_CLAIM_BATCH, how many jobs one claim asks
// control for. Unset is the default (50); a value that is not a positive
// integer is an error naming the variable.
func LoadClaimBatch() (int, error) {
	raw := strings.TrimSpace(os.Getenv("QUARRY_JOB_CLAIM_BATCH"))
	if raw == "" {
		return defaultClaimBatch, nil
	}
	n, err := strconv.Atoi(raw)
	if err != nil || n < 1 || n > 500 {
		return 0, fmt.Errorf("QUARRY_JOB_CLAIM_BATCH must be an integer from 1 to 500")
	}
	return n, nil
}
