// Package timeouts holds the Temporal execution timeout of a job, by kind (WS15.L2,
// the typed timeout). It is a leaf package: the jobs dispatcher and the control
// sweep both read it, and the workflows must not import either.
package timeouts

import (
	"os"
	"strconv"
	"time"
)

// Environment names. Each one, when set to a positive number of seconds, replaces
// the default for its kind. Every result is clamped to the ceiling below.
const (
	EnvScrapeS = "QUARRY_JOB_TIMEOUT_SCRAPE_S"
	EnvBatchS  = "QUARRY_JOB_TIMEOUT_BATCH_S"
	EnvCrawlS  = "QUARRY_JOB_TIMEOUT_CRAWL_S"
)

const (
	scrapeDefaultS = 300
	// batchPerURLS and batchCapS: a batch gets 20 seconds per URL, at most 2 hours.
	batchPerURLS = 20
	batchCapS    = 7200
	// crawlPerPageS and crawlCapS: a crawl gets 10 seconds per page, at most 6 hours.
	crawlPerPageS = 10
	crawlCapS     = 21600
	// crawlUnknownS is a crawl whose page bound is not in its params.
	crawlUnknownS = 1800
	// ceilingS is the longest any job may run, whatever the environment says.
	ceilingS = 21600
	// floorS keeps a computed default (a batch of one URL) from being too short
	// for one page to finish. A decision of this package, not in the plan text.
	floorS = 60
)

// ExecutionTimeoutFor returns the workflow execution timeout of a job of kind,
// computed from its params unless the kind's environment variable overrides it.
// Unknown kinds get the scrape default.
func ExecutionTimeoutFor(kind string, params map[string]any) time.Duration {
	return time.Duration(seconds(kind, params)) * time.Second
}

func seconds(kind string, params map[string]any) int64 {
	var secs int64
	switch kind {
	case "crawl":
		def := int64(crawlUnknownS)
		if pages := paramInt(params, "max_pages"); pages > 0 {
			def = min64(crawlCapS, crawlPerPageS*pages)
		}
		secs = envOr(EnvCrawlS, def)
	case "batch":
		urls := int64(len(paramStrings(params, "urls")))
		secs = envOr(EnvBatchS, min64(batchCapS, batchPerURLS*urls))
	default:
		secs = envOr(EnvScrapeS, scrapeDefaultS)
	}
	if secs < floorS {
		secs = floorS
	}
	if secs > ceilingS {
		secs = ceilingS
	}
	return secs
}

// envOr returns the variable's value when it is a positive integer, otherwise def.
func envOr(name string, def int64) int64 {
	n, err := strconv.ParseInt(os.Getenv(name), 10, 64)
	if err != nil || n <= 0 {
		return def
	}
	return n
}

func min64(a, b int64) int64 {
	if a < b {
		return a
	}
	return b
}

// paramInt reads a number param from JSON-decoded params (float64) or Go ints.
func paramInt(params map[string]any, key string) int64 {
	switch v := params[key].(type) {
	case float64:
		return int64(v)
	case int:
		return int64(v)
	case int64:
		return v
	}
	return 0
}

// paramStrings reads a list-of-strings param, as JSON decodes it.
func paramStrings(params map[string]any, key string) []string {
	switch v := params[key].(type) {
	case []string:
		return v
	case []any:
		out := make([]string, 0, len(v))
		for _, item := range v {
			if s, ok := item.(string); ok {
				out = append(out, s)
			}
		}
		return out
	}
	return nil
}
