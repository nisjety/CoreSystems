package jobs

import (
	"testing"

	"github.com/stretchr/testify/require"
)

func TestResolve_ZeroUsesDefault(t *testing.T) {
	r := DefaultCrawlLimits().Resolve(0, 0)
	require.Equal(t, Resolved{Pages: 1000, Depth: 5, PagesBy: ByDefault, DepthBy: ByDefault}, r)
}

func TestResolve_InRangeIsUsedAsAsked(t *testing.T) {
	r := DefaultCrawlLimits().Resolve(42, 3)
	require.Equal(t, Resolved{Pages: 42, Depth: 3, PagesBy: ByNone, DepthBy: ByNone}, r)
	// The ceiling itself is in range.
	r = DefaultCrawlLimits().Resolve(5000, 10)
	require.Equal(t, uint32(5000), r.Pages)
	require.Equal(t, ByNone, r.PagesBy)
}

func TestResolve_ClampsToHardAndReports(t *testing.T) {
	r := DefaultCrawlLimits().Resolve(1_000_000, 99)
	require.Equal(t, Resolved{Pages: 5000, Depth: 10, PagesBy: ByHard, DepthBy: ByHard}, r)
}

func TestResolve_DefaultZeroDisablesDefaultNotHard(t *testing.T) {
	l := CrawlLimits{DefaultPages: 0, HardPages: 5000, DefaultDepth: 0, HardDepth: 10}
	r := l.Resolve(0, 0)
	// An absent request takes the ceiling: still finite.
	require.Equal(t, uint32(5000), r.Pages)
	require.Equal(t, uint32(10), r.Depth)
	// And a request above the ceiling is still clamped.
	require.Equal(t, uint32(5000), l.Resolve(9999, 0).Pages)
}

func TestLoadCrawlLimits_EnvOverridesAndValidation(t *testing.T) {
	env := func(m map[string]string) func(string) string {
		return func(k string) string { return m[k] }
	}

	got, err := loadCrawlLimits(env(nil))
	require.NoError(t, err)
	require.Equal(t, DefaultCrawlLimits(), got)

	got, err = loadCrawlLimits(env(map[string]string{
		"QUARRY_CRAWL_DEFAULT_MAX_PAGES": "7",
		"QUARRY_CRAWL_HARD_MAX_PAGES":    " 20 ",
	}))
	require.NoError(t, err)
	require.Equal(t, uint32(7), got.DefaultPages)
	require.Equal(t, uint32(20), got.HardPages)

	for name, m := range map[string]map[string]string{
		"unparsable":               {"QUARRY_CRAWL_HARD_MAX_PAGES": "lots"},
		"negative":                 {"QUARRY_CRAWL_DEFAULT_MAX_DEPTH": "-1"},
		"hard pages disabled":      {"QUARRY_CRAWL_HARD_MAX_PAGES": "0"},
		"hard depth disabled":      {"QUARRY_CRAWL_HARD_MAX_DEPTH": "0"},
		"default above hard":       {"QUARRY_CRAWL_DEFAULT_MAX_PAGES": "9000"},
		"default depth above hard": {"QUARRY_CRAWL_DEFAULT_MAX_DEPTH": "11"},
	} {
		_, err := loadCrawlLimits(env(m))
		require.Error(t, err, name)
		for k := range m {
			require.Contains(t, err.Error(), k, "the error names the variable: %s", name)
		}
	}
}

func TestCrawlInputFromJob_BoundsLegacyRows(t *testing.T) {
	limits := DefaultCrawlLimits()
	for name, tc := range map[string]struct {
		params map[string]any
		pages  uint32
		depth  uint32
	}{
		"absent":        {map[string]any{"url": "https://a.example/"}, 1000, 5},
		"zero":          {map[string]any{"url": "https://a.example/", "max_pages": float64(0), "max_depth": float64(0)}, 1000, 5},
		"in range":      {map[string]any{"url": "https://a.example/", "max_pages": float64(25), "max_depth": float64(2)}, 25, 2},
		"above the cap": {map[string]any{"url": "https://a.example/", "max_pages": float64(900000), "max_depth": float64(50)}, 5000, 10},
	} {
		in, err := crawlInputFromJob(job{ID: "job_1", Kind: "crawl", Params: tc.params}, "run_1", limits)
		require.NoError(t, err, name)
		require.Equal(t, tc.pages, in.MaxPages, name)
		require.Equal(t, tc.depth, in.MaxDepth, name)
	}
}
