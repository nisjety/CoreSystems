package main

import (
	"bytes"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/linkprobe"
)

// notice is a minimal eForms document whose external references are the given links.
func notice(links ...string) string {
	var b strings.Builder
	b.WriteString(`<?xml version="1.0"?><ContractNotice xmlns="urn:oasis:names:specification:ubl:schema:xsd:ContractNotice-2" xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2" xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2">`)
	b.WriteString(`<cbc:CustomizationID>eforms-sdk-1.14#extended#urn:fdc:anskaffelser.no:2023:eforms:eu</cbc:CustomizationID><cbc:ID>00000000-0000-4000-8000-0000000000cc</cbc:ID>`)
	for _, l := range links {
		fmt.Fprintf(&b, `<cac:Attachment><cac:ExternalReference><cbc:URI>%s</cbc:URI></cac:ExternalReference></cac:Attachment>`, l)
	}
	b.WriteString(`</ContractNotice>`)
	return b.String()
}

// corpus writes the notices to a temporary directory and returns it.
func corpus(t *testing.T, notices ...string) string {
	t.Helper()
	dir := t.TempDir()
	for i, n := range notices {
		if err := os.WriteFile(filepath.Join(dir, fmt.Sprintf("n%d.xml", i)), []byte(n), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return dir
}

// server answers a document, a login page and nothing else, so no test needs the internet.
func server(t *testing.T) *httptest.Server {
	t.Helper()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/a.pdf":
			w.Header().Set("Content-Type", "application/pdf")
			_, _ = w.Write([]byte("%PDF-1.4 a document"))
		case "/login":
			w.Header().Set("Content-Type", "text/html")
			_, _ = w.Write([]byte(`<html><form><input type="password" name="p"></form>`))
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(srv.Close)
	return srv
}

func noPause(time.Duration) {}

// The run asks each distinct link once, classifies the answers, and prints counts only: the links,
// the hosts and the notices never appear in its output.
func TestRunPrintsCountsOnlyAndSkipsLoginPlatforms(t *testing.T) {
	t.Parallel()

	srv := server(t)
	dir := corpus(t,
		notice(srv.URL+"/a.pdf", srv.URL+"/login"),
		notice(srv.URL+"/a.pdf", "https://www.mercell.com/tender/42"),
	)
	var stdout, stderr bytes.Buffer
	code := run([]string{"-dir", dir, "-gap", "0s"}, &stdout, &stderr, linkprobe.NewClient(), noPause)
	if code != 0 {
		t.Fatalf("exit %d, stderr %q", code, stderr.String())
	}
	out := stdout.String()
	for _, want := range []string{
		"notices read: 2",
		"distinct links: 3",
		"not requested (login platform): 1",
		"requested: 2",
		"document 1",
		"login-required 1",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("output lacks %q:\n%s", want, out)
		}
	}
	for _, leak := range []string{"http", "127.0.0.1", "mercell", "/a.pdf", "/login", "tender"} {
		if strings.Contains(out, leak) {
			t.Errorf("output carries %q, which names a link or host", leak)
		}
	}
}

// A run asks no more links than -max, and says so.
func TestRunCapsTheRequests(t *testing.T) {
	t.Parallel()

	srv := server(t)
	dir := corpus(t, notice(srv.URL+"/a.pdf", srv.URL+"/login", srv.URL+"/missing"))
	var stdout bytes.Buffer
	if code := run([]string{"-dir", dir, "-max", "1", "-gap", "0s"}, &stdout, &bytes.Buffer{}, linkprobe.NewClient(), noPause); code != 0 {
		t.Fatalf("exit %d", code)
	}
	if !strings.Contains(stdout.String(), "requested: 1, capped at max: yes") {
		t.Errorf("output does not show the cap:\n%s", stdout.String())
	}
}

// A directory with no notices in it is an error, not an empty report.
func TestRunRefusesAnEmptyDirectory(t *testing.T) {
	t.Parallel()

	var stderr bytes.Buffer
	if code := run([]string{"-dir", t.TempDir()}, &bytes.Buffer{}, &stderr, linkprobe.NewClient(), noPause); code != 1 {
		t.Errorf("exit %d, want 1 for a directory with no notices", code)
	}
}

// With -hop-max, each page the first hop found is read once, and the document links it holds on its
// own host are requested once each. Links on other hosts are counted and never followed.
func TestRunHopsFromHtmlPagesToTheirDocuments(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/page":
			w.Header().Set("Content-Type", "text/html")
			_, _ = w.Write([]byte(`<html><a href="/a.pdf">annonse</a><a href="https://other.example.invalid/x.pdf">andre</a></html>`))
		case "/a.pdf":
			w.Header().Set("Content-Type", "application/pdf")
			_, _ = w.Write([]byte("%PDF-1.4 a document"))
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(srv.Close)
	dir := corpus(t, notice(srv.URL+"/page"))
	var stdout bytes.Buffer
	if code := run([]string{"-dir", dir, "-gap", "0s", "-hop-max", "5"}, &stdout, &bytes.Buffer{}, linkprobe.NewClient(), noPause); code != 0 {
		t.Fatalf("exit %d", code)
	}
	out := stdout.String()
	for _, want := range []string{
		"second hop: pages read 1, unreadable 0",
		"document links on them 1 (same host), on other hosts 1 (not followed)",
		"second hop: requested 1, capped at max per page: no",
		"second hop results: document 1",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("output lacks %q:\n%s", want, out)
		}
	}
	for _, leak := range []string{"http", "127.0.0.1", "example.invalid", "/a.pdf", "/page"} {
		if strings.Contains(out, leak) {
			t.Errorf("output carries %q, which names a link or host", leak)
		}
	}
}

// Without -hop-max the probe makes no second request: the first hop is all that was agreed.
func TestRunWithoutHopMakesNoSecondRequest(t *testing.T) {
	t.Parallel()

	var pdfRequests int
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/page":
			w.Header().Set("Content-Type", "text/html")
			_, _ = w.Write([]byte(`<html><a href="/a.pdf">annonse</a></html>`))
		case "/a.pdf":
			pdfRequests++
			_, _ = w.Write([]byte("%PDF-1.4"))
		default:
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(srv.Close)
	dir := corpus(t, notice(srv.URL+"/page"))
	var stdout bytes.Buffer
	if code := run([]string{"-dir", dir, "-gap", "0s"}, &stdout, &bytes.Buffer{}, linkprobe.NewClient(), noPause); code != 0 {
		t.Fatalf("exit %d", code)
	}
	if pdfRequests != 0 {
		t.Errorf("the document behind the page was requested %d times without -hop-max", pdfRequests)
	}
	if strings.Contains(stdout.String(), "second hop") {
		t.Errorf("a second-hop line is printed without -hop-max:\n%s", stdout.String())
	}
}
