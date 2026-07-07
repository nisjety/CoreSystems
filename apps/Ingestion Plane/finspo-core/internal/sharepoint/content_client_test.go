package sharepoint

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// staticTokenProvider is defined in delta_client_test.go (same package).

func TestContentClient_DownloadFollows302(t *testing.T) {
	// CDN host that serves the preauthenticated bytes. (In production Graph
	// redirects /content to a different host, so Go strips Authorization; both
	// httptest servers share 127.0.0.1 here so we don't assert on that.)
	cdn := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte("file-bytes"))
	}))
	defer cdn.Close()

	// Graph host that 302-redirects /content to the CDN.
	graph := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasSuffix(r.URL.Path, "/content") {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		if r.Header.Get("Authorization") != "Bearer tok" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		http.Redirect(w, r, cdn.URL+"/download", http.StatusFound)
	}))
	defer graph.Close()

	c := NewContentClient(ContentClientConfig{
		BaseURL:       graph.URL,
		TokenProvider: staticTokenProvider{token: "tok"},
	})
	data, err := c.DownloadContent(context.Background(), "org", "drive-1", "item-1")
	if err != nil {
		t.Fatalf("download: %v", err)
	}
	if string(data) != "file-bytes" {
		t.Fatalf("got %q", data)
	}
}

func TestContentClient_RejectsOversized(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(strings.Repeat("A", 100)))
	}))
	defer srv.Close()

	c := NewContentClient(ContentClientConfig{
		BaseURL:       srv.URL,
		TokenProvider: staticTokenProvider{token: "tok"},
		MaxBytes:      10,
	})
	_, err := c.DownloadContent(context.Background(), "org", "d", "i")
	if err == nil || !strings.Contains(err.Error(), "exceeds max") {
		t.Fatalf("expected oversized error, got %v", err)
	}
}

func TestContentClient_Non200IsError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusForbidden)
	}))
	defer srv.Close()

	c := NewContentClient(ContentClientConfig{
		BaseURL:       srv.URL,
		TokenProvider: staticTokenProvider{token: "tok"},
	})
	if _, err := c.DownloadContent(context.Background(), "org", "d", "i"); err == nil {
		t.Fatal("expected error on 403")
	}
}

func TestContentClient_MissingTokenProvider(t *testing.T) {
	c := NewContentClient(ContentClientConfig{BaseURL: "http://x"})
	if _, err := c.DownloadContent(context.Background(), "org", "d", "i"); err != ErrNotConfigured {
		t.Fatalf("expected ErrNotConfigured, got %v", err)
	}
}

func TestContentClient_RequiresIDs(t *testing.T) {
	c := NewContentClient(ContentClientConfig{
		BaseURL:       "http://x",
		TokenProvider: staticTokenProvider{token: "tok"},
	})
	if _, err := c.DownloadContent(context.Background(), "org", "", "i"); err == nil {
		t.Fatal("expected error for empty driveID")
	}
}
