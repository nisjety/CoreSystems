package timeouts

import (
	"testing"
	"time"
)

// TestExecutionTimeoutPerKind pins the default timeout of each kind (WS15.L2).
func TestExecutionTimeoutPerKind(t *testing.T) {
	t.Setenv(EnvScrapeS, "")
	t.Setenv(EnvBatchS, "")
	t.Setenv(EnvCrawlS, "")

	cases := []struct {
		name   string
		kind   string
		params map[string]any
		want   time.Duration
	}{
		{"scrape default", "scrape", nil, 300 * time.Second},
		{"batch 20 urls", "batch", map[string]any{"urls": []any{"a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t"}}, 400 * time.Second},
		{"batch capped at two hours", "batch", map[string]any{"urls": makeURLs(1000)}, 7200 * time.Second},
		{"batch single url keeps the floor", "batch", map[string]any{"urls": []any{"a"}}, 60 * time.Second},
		{"crawl 50 pages", "crawl", map[string]any{"max_pages": float64(50)}, 500 * time.Second},
		{"crawl 5000 pages capped at six hours", "crawl", map[string]any{"max_pages": float64(5000)}, 21600 * time.Second},
		{"crawl without a page bound", "crawl", nil, 1800 * time.Second},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := ExecutionTimeoutFor(tc.kind, tc.params); got != tc.want {
				t.Fatalf("ExecutionTimeoutFor(%s) = %s, want %s", tc.kind, got, tc.want)
			}
		})
	}
}

// TestExecutionTimeoutEnvOverridesDefault checks that a positive variable replaces
// the default, and that the six-hour ceiling still applies.
func TestExecutionTimeoutEnvOverridesDefault(t *testing.T) {
	t.Setenv(EnvCrawlS, "20")
	if got := ExecutionTimeoutFor("crawl", map[string]any{"max_pages": float64(5000)}); got != 20*time.Second {
		t.Fatalf("crawl override = %s, want 20s", got)
	}
	t.Setenv(EnvCrawlS, "999999")
	if got := ExecutionTimeoutFor("crawl", nil); got != 21600*time.Second {
		t.Fatalf("crawl over ceiling = %s, want 6h", got)
	}
	t.Setenv(EnvCrawlS, "not-a-number")
	if got := ExecutionTimeoutFor("crawl", nil); got != 1800*time.Second {
		t.Fatalf("crawl bad value = %s, want the default 1800s", got)
	}
}

func makeURLs(n int) []any {
	out := make([]any, n)
	for i := range out {
		out[i] = "https://example.test/" + string(rune('a'+i%26))
	}
	return out
}
