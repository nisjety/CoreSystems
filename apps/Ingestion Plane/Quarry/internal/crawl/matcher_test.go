package crawl

import "testing"

func TestMatcher_RestrictsToStartPathByDefault(t *testing.T) {
	t.Parallel()

	spec := Spec{
		URL:   "https://docs.example.com/products/start",
		Limit: 100,
	}

	matcher, err := NewMatcher(spec)
	if err != nil {
		t.Fatalf("NewMatcher() error = %v", err)
	}

	if !matcher.Match("https://docs.example.com/products/start/guide") {
		t.Fatal("Match() = false, want true for path under seed")
	}
	if matcher.Match("https://docs.example.com/blog/post") {
		t.Fatal("Match() = true, want false for sibling path")
	}
}

func TestMatcher_AllowsBaseDomainAndSubdomainsWhenConfigured(t *testing.T) {
	t.Parallel()

	spec := Spec{
		URL:               "https://docs.example.com/products/start",
		Limit:             100,
		CrawlEntireDomain: true,
		AllowSubdomains:   true,
	}

	matcher, err := NewMatcher(spec)
	if err != nil {
		t.Fatalf("NewMatcher() error = %v", err)
	}

	if !matcher.Match("https://docs.example.com/blog/post") {
		t.Fatal("Match() = false, want true for same host when crawlEntireDomain=true")
	}
	if !matcher.Match("https://api.example.com/reference") {
		t.Fatal("Match() = false, want true for sibling subdomain when allowSubdomains=true")
	}
	if matcher.Match("https://other.example.net/reference") {
		t.Fatal("Match() = true, want false for external domain")
	}
}

func TestMatcher_UsesPathScopedPatternsWhenRegexOnFullURLIsDisabled(t *testing.T) {
	t.Parallel()

	spec := Spec{
		URL:          "https://example.com/docs",
		Limit:        100,
		IncludePaths: []string{"/docs/**"},
		ExcludePaths: []string{"/docs/private/**"},
	}

	matcher, err := NewMatcher(spec)
	if err != nil {
		t.Fatalf("NewMatcher() error = %v", err)
	}

	if !matcher.Match("https://example.com/docs/public/start") {
		t.Fatal("Match() = false, want true for included path")
	}
	if matcher.Match("https://example.com/docs/private/secret") {
		t.Fatal("Match() = true, want false for excluded path")
	}
}

func TestMatcher_RejectsInvalidRegexPatterns(t *testing.T) {
	t.Parallel()

	spec := Spec{
		URL:            "https://example.com/docs",
		Limit:          100,
		RegexOnFullURL: true,
		IncludePaths:   []string{"["},
	}

	if _, err := NewMatcher(spec); err == nil {
		t.Fatal("NewMatcher() error = nil, want error")
	}
}

func TestMatcher_RejectsStaticAssets(t *testing.T) {
	t.Parallel()

	spec := Spec{
		URL:              "https://triodelab.no",
		CrawlEntireDomain: true,
		Limit:            100,
	}

	matcher, err := NewMatcher(spec)
	if err != nil {
		t.Fatalf("NewMatcher() error = %v", err)
	}

	staticURLs := []string{
		"https://triodelab.no/favicon.ico",
		"https://triodelab.no/favicon.svg",
		"https://triodelab.no/favicon-32x32.png",
		"https://triodelab.no/apple-touch-icon.png",
		"https://triodelab.no/site.webmanifest",
		"https://triodelab.no/_next/static/css/d25f29a4f5c41b15.css",
		"https://triodelab.no/_next/static/chunks/webpack-b9c202e70674d764.js",
		"https://triodelab.no/_next/static/media/font.woff2",
	}
	for _, u := range staticURLs {
		if matcher.Match(u) {
			t.Errorf("Match(%q) = true, want false (static asset should be excluded)", u)
		}
	}

	contentURLs := []string{
		"https://triodelab.no/",
		"https://triodelab.no/tjenester",
		"https://triodelab.no/om-oss",
		"https://triodelab.no/blog/post-1",
	}
	for _, u := range contentURLs {
		if !matcher.Match(u) {
			t.Errorf("Match(%q) = false, want true (content page should be included)", u)
		}
	}
}
