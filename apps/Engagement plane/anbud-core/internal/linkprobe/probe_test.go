package linkprobe

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// Each response of the kinds a document link can give is classified as what it is for the user:
// a document to take, a login wall to stop at, a page with nothing to take, or a failure.
func TestClassifyTheResponsesThatMatter(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name string
		res  Result
		want Class
	}{
		{"a PDF", Result{Status: 200, ContentType: "application/pdf", Head: []byte("%PDF-1.7 ...")}, Document},
		{"a ZIP", Result{Status: 200, ContentType: "application/octet-stream", Head: []byte("PK\x03\x04...")}, Document},
		{"a login form", Result{Status: 200, ContentType: "text/html", Head: []byte(`<html><form><input type="password" name="p"></form>`)}, LoginRequired},
		{"a page with no form", Result{Status: 200, ContentType: "text/html", Head: []byte("<html><body>Velkommen</body></html>")}, HTMLPage},
		{"redirected to a login path", Result{Status: 200, FinalPath: "/login", ContentType: "text/html", Head: []byte("<html>")}, LoginRequired},
		{"unauthorised", Result{Status: 401}, LoginRequired},
		{"forbidden", Result{Status: 403}, Forbidden},
		{"gone", Result{Status: 404}, Gone},
		{"rate limited", Result{Status: 429}, RateLimited},
		{"server error", Result{Status: 503}, ServerError},
		{"no response", Result{Err: errors.New("dial failed")}, Unreachable},
		{"bytes that are none of these", Result{Status: 200, ContentType: "application/octet-stream", Head: []byte("\x00\x01\x02")}, Unclear},
	}
	for _, c := range cases {
		if got := Classify(c.res); got != c.want {
			t.Errorf("%s: Classify = %v, want %v", c.name, got, c.want)
		}
	}
}

// The request carries no cookie, no credential and no body, asks for the first kilobytes only,
// and says who is asking. The response it gets back is read in part and closed.
func TestFetchAsksForTheHeadOfAnOpenDocumentWithoutAnything(t *testing.T) {
	t.Parallel()

	var got http.Header
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got = r.Header.Clone()
		w.Header().Set("Content-Type", "application/pdf")
		_, _ = w.Write([]byte("%PDF-1.4 the start of a document"))
	}))
	defer srv.Close()

	res := Fetch(context.Background(), NewClient(), srv.URL+"/doc.pdf")
	if res.Err != nil || res.Status != 200 {
		t.Fatalf("Fetch: status %d, err %v", res.Status, res.Err)
	}
	if Classify(res) != Document || !strings.HasPrefix(string(res.Head), "%PDF-") {
		t.Errorf("classified %v with head %q", Classify(res), res.Head)
	}
	if len(res.Head) > HeadBytes {
		t.Errorf("read %d bytes, want at most %d", len(res.Head), HeadBytes)
	}
	if got.Get("Cookie") != "" || got.Get("Authorization") != "" {
		t.Error("the probe sent a cookie or an authorisation header")
	}
	if !strings.HasPrefix(got.Get("User-Agent"), "anbud-core-link-probe") {
		t.Errorf("User-Agent = %q, want the probe named", got.Get("User-Agent"))
	}
	if got.Get("Range") == "" {
		t.Error("the probe asked for the whole document, not its head")
	}
}

// A link that sends the probe to a login page is a login wall, whatever status it ends on, and
// the probe stops there: it does not follow redirects past the third.
func TestFetchReportsALoginRedirect(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/login") {
			w.Header().Set("Content-Type", "text/html")
			_, _ = w.Write([]byte(`<html><form><input type="password"></form>`))
			return
		}
		http.Redirect(w, r, "/login?next=/doc", http.StatusFound)
	}))
	defer srv.Close()

	res := Fetch(context.Background(), NewClient(), srv.URL+"/doc")
	if Classify(res) != LoginRequired {
		t.Errorf("a redirect to a login page classified as %v, want %v", Classify(res), LoginRequired)
	}
}

// Links are grouped by the family of host they point at, for the summary: the known login
// platforms are told apart, because they are not requested at all.
func TestFamilyGroupsHostsWithoutNamingThem(t *testing.T) {
	t.Parallel()

	cases := map[string]string{
		"https://www.mercell.com/tender/1":    "login-platform",
		"https://eu-supply.com/x":             "login-platform",
		"https://app.tendsign.com/y":          "login-platform",
		"https://doffin.no/attachments/z.pdf": "doffin",
		"https://api.doffin.no/x":             "doffin",
		"https://www.some-kommune.no/anbud":   "norway-other",
		"https://example.org/doc":             "other",
		"not a url":                           "other",
	}
	for link, want := range cases {
		if got := Family(link); got != want {
			t.Errorf("Family(%q) = %q, want %q", link, got, want)
		}
	}
}

// A request that gets no answer in time is unreachable, not a failure of the probe.
func TestFetchTimesOut(t *testing.T) {
	t.Parallel()

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		time.Sleep(300 * time.Millisecond)
	}))
	defer srv.Close()

	client := NewClient()
	client.Timeout = 50 * time.Millisecond
	res := Fetch(context.Background(), client, srv.URL)
	if Classify(res) != Unreachable {
		t.Errorf("a timed-out request classified as %v, want %v", Classify(res), Unreachable)
	}
}
