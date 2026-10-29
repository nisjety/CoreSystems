package linkprobe

import (
	"bytes"
	"context"
	"net/http"
	"net/http/httptest"
	"net/url"
	"slices"
	"strings"
	"testing"
)

// The links of a page that point at a document, on the page's own host, are the one hop the probe
// makes: a link to another host is counted and not followed, and a link to a page is not a document.
func TestDocumentLinksOfAPageStayOnItsHostAndAreDocuments(t *testing.T) {
	t.Parallel()

	base, err := url.Parse("https://buyer.example.invalid/anbud/42")
	if err != nil {
		t.Fatal(err)
	}
	page := []byte(`<html>
	  <a href="/files/annonse.pdf">annonse</a>
	  <a href="https://buyer.example.invalid/dok/grunnlag.zip">grunnlag</a>
	  <a href="https://other.example.invalid/y.pdf">other host</a>
	  <a href="/om-oss">a page, not a document</a>
	  <a href="mailto:someone@example.invalid">mail</a>
	  <a href="javascript:void(0)">script</a>
	  <a href="/download?id=7">download</a>
	  <a href="/files/annonse.pdf">again</a>
	</html>`)
	same, otherHost := DocumentLinks(page, base)
	want := []string{
		"https://buyer.example.invalid/files/annonse.pdf",
		"https://buyer.example.invalid/dok/grunnlag.zip",
		"https://buyer.example.invalid/download?id=7",
	}
	if !slices.Equal(same, want) {
		t.Errorf("same-host document links = %v, want %v", same, want)
	}
	if otherHost != 1 {
		t.Errorf("links on other hosts = %d, want 1", otherHost)
	}
}

// The probe reads at most PageBytes of a page: enough to find its links, never a whole large file.
// The final URL is the one the page came from, after redirects, so that relative links resolve.
func TestFetchPageReadsOnlyItsFirstBytesAndKeepsTheFinalURL(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/start" {
			http.Redirect(w, r, "/anbud/42", http.StatusFound)
			return
		}
		w.Header().Set("Content-Type", "text/html")
		_, _ = w.Write(bytes.Repeat([]byte("x"), PageBytes+100_000))
	}))
	defer srv.Close()

	body, final, err := FetchPage(context.Background(), NewClient(), srv.URL+"/start")
	if err != nil {
		t.Fatalf("FetchPage: %v", err)
	}
	if len(body) != PageBytes {
		t.Errorf("read %d bytes of the page, want exactly %d", len(body), PageBytes)
	}
	if final == nil || final.Path != "/anbud/42" {
		t.Errorf("final URL path = %v, want /anbud/42", final)
	}
	if strings.Contains(string(body), "\x00") {
		t.Error("body was altered")
	}
}

// A page the probe cannot read gives an error and no links, not a guess.
func TestFetchPageReportsAnUnreadablePage(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Error(w, "down", http.StatusInternalServerError)
	}))
	defer srv.Close()

	_, _, err := FetchPage(context.Background(), NewClient(), srv.URL)
	if err == nil {
		t.Error("a server error gave a page")
	}
}
