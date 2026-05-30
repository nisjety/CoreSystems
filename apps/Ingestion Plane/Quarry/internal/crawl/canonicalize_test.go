package crawl

import (
	"net/url"
	"testing"
)

func TestCanonicalizeURL_NormalizesFragmentsPortsAndPaths(t *testing.T) {
	t.Parallel()

	base, err := url.Parse("https://example.com/docs/start/")
	if err != nil {
		t.Fatalf("url.Parse() error = %v", err)
	}

	got, err := CanonicalizeURL("../guide//intro/?b=2&a=1#section", base, false)
	if err != nil {
		t.Fatalf("CanonicalizeURL() error = %v", err)
	}

	if got != "https://example.com/docs/guide/intro?a=1&b=2" {
		t.Fatalf("CanonicalizeURL() = %q, want %q", got, "https://example.com/docs/guide/intro?a=1&b=2")
	}
}

func TestCanonicalizeURL_IgnoresQueryWhenConfigured(t *testing.T) {
	t.Parallel()

	got, err := CanonicalizeURL("https://example.com:443/docs/?b=2&a=1#frag", nil, true)
	if err != nil {
		t.Fatalf("CanonicalizeURL() error = %v", err)
	}

	if got != "https://example.com/docs" {
		t.Fatalf("CanonicalizeURL() = %q, want %q", got, "https://example.com/docs")
	}
}

func TestCanonicalizeURL_RejectsUnsupportedSchemes(t *testing.T) {
	t.Parallel()

	if _, err := CanonicalizeURL("mailto:test@example.com", nil, false); err == nil {
		t.Fatal("CanonicalizeURL() error = nil, want error")
	}
}
