package crawl

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestNormalizeSpec_DefaultsAndSchemaAlias(t *testing.T) {
	t.Parallel()

	spec, err := NormalizeSpec(NormalizeInput{
		URL:    "https://example.com/docs",
		Schema: `{"type":"object"}`,
		Prompt: "extract page name",
	})
	if err != nil {
		t.Fatalf("NormalizeSpec() error = %v", err)
	}

	if spec.Limit != DefaultLimit {
		t.Fatalf("limit = %d, want %d", spec.Limit, DefaultLimit)
	}
	if spec.MaxConcurrency != DefaultMaxConcurrency {
		t.Fatalf("maxConcurrency = %d, want %d", spec.MaxConcurrency, DefaultMaxConcurrency)
	}
	if !spec.DeduplicateSimilarURLs {
		t.Fatal("DeduplicateSimilarURLs = false, want true")
	}
	if spec.Prompt != "" {
		t.Fatalf("Prompt = %q, want empty because schema alias consumed it", spec.Prompt)
	}
	if len(spec.PageOptions.Formats) != 1 || spec.PageOptions.Formats[0].Type != "json" {
		t.Fatalf("formats = %+v, want one json format", spec.PageOptions.Formats)
	}
}

func TestNormalizeSpec_DefaultsToMarkdownWhenNoFormatsProvided(t *testing.T) {
	t.Parallel()

	spec, err := NormalizeSpec(NormalizeInput{
		URL: "https://example.com/docs",
	})
	if err != nil {
		t.Fatalf("NormalizeSpec() error = %v", err)
	}

	if len(spec.PageOptions.Formats) != 1 || spec.PageOptions.Formats[0].Type != "markdown" {
		t.Fatalf("formats = %+v, want markdown default", spec.PageOptions.Formats)
	}
}

func TestFetchSitemapURLs_ParsesURLSet(t *testing.T) {
	t.Parallel()

	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/sitemap.xml" {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		_, _ = w.Write([]byte(`<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url><loc>` + server.URL + `/a</loc></url>
  <url><loc>` + server.URL + `/b</loc></url>
</urlset>`))
	}))
	defer server.Close()

	urls, err := FetchSitemapURLs(t.Context(), server.URL+"/start")
	if err != nil {
		t.Fatalf("FetchSitemapURLs() error = %v", err)
	}
	if len(urls) != 2 {
		t.Fatalf("len(urls) = %d, want 2", len(urls))
	}
}

func TestMemoryStore_TracksErrorsAndRobotsBlocked(t *testing.T) {
	t.Parallel()

	store := NewMemoryStore(time.Hour)
	run := &Run{ID: "run-1", URL: "https://example.com"}
	if err := store.CreateRun(t.Context(), run); err != nil {
		t.Fatalf("CreateRun() error = %v", err)
	}

	if err := store.AppendError(t.Context(), run.ID, &PageError{URL: "https://example.com/fail", Error: "boom"}); err != nil {
		t.Fatalf("AppendError() error = %v", err)
	}
	if err := store.AddRobotsBlocked(t.Context(), run.ID, []string{"https://example.com/private"}); err != nil {
		t.Fatalf("AddRobotsBlocked() error = %v", err)
	}

	errors, err := store.ListErrors(t.Context(), run.ID)
	if err != nil {
		t.Fatalf("ListErrors() error = %v", err)
	}
	if len(errors) != 1 {
		t.Fatalf("len(errors) = %d, want 1", len(errors))
	}

	blocked, err := store.ListRobotsBlocked(t.Context(), run.ID)
	if err != nil {
		t.Fatalf("ListRobotsBlocked() error = %v", err)
	}
	if len(blocked) != 1 {
		t.Fatalf("len(blocked) = %d, want 1", len(blocked))
	}
}
