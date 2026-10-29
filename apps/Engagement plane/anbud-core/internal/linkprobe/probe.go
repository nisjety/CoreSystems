// Package linkprobe measures whether the document links a notice publishes are open to anyone. It
// asks each link once, with no cookie, no credential and no login, and classifies the answer. It
// downloads no more than the first HeadBytes of a response, follows at most three redirects, and
// never requests the known login platforms at all: a login wall is not something to get past.
package linkprobe

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// UserAgent names the probe to the server it asks.
const UserAgent = "anbud-core-link-probe/0.1 (no login, no cookies; the project owner can be asked)"

// HeadBytes is how much of a response the probe reads: enough to tell a document from a page.
const HeadBytes = 2048

// Class is what a link's answer means for a user who wants the document.
type Class int

const (
	Unclear       Class = iota // none of the cases below
	Document                   // a document is there to take
	LoginRequired              // the link stops at a login
	HTMLPage                   // a page with no document on it
	Forbidden                  // refused
	Gone                       // not found or removed
	RateLimited                // asked too often
	ServerError                // the server failed
	Unreachable                // no answer, or no answer in time
)

func (c Class) String() string {
	switch c {
	case Document:
		return "document"
	case LoginRequired:
		return "login-required"
	case HTMLPage:
		return "html-page"
	case Forbidden:
		return "forbidden"
	case Gone:
		return "gone"
	case RateLimited:
		return "rate-limited"
	case ServerError:
		return "server-error"
	case Unreachable:
		return "unreachable"
	}
	return "unclear"
}

// Result is what came back for one link. Err is set when there was no answer at all. FinalPath is
// the path the answer came from, after any redirects; it is read to spot a login page and is never
// printed.
type Result struct {
	Status      int
	FinalPath   string
	ContentType string
	Head        []byte
	Err         error
}

var (
	loginPath     = regexp.MustCompile(`(?i)/(login|signin|sign-in|logg-inn|logginn|authenticate|auth)(/|$|\?)`)
	passwordField = regexp.MustCompile(`(?i)type\s*=\s*["']?password`)
)

// Classify turns an answer into a class. The status decides first; for a 200 the path, the bytes
// and then the page are read, in that order, so that a login page with a document type is still a
// login page.
func Classify(r Result) Class {
	if r.Err != nil {
		return Unreachable
	}
	switch {
	case r.Status == 401:
		return LoginRequired
	case r.Status == 403:
		return Forbidden
	case r.Status == 404 || r.Status == 410:
		return Gone
	case r.Status == 429:
		return RateLimited
	case r.Status >= 500:
		return ServerError
	case r.Status != 200 && r.Status != 206:
		return Unclear
	}
	if loginPath.MatchString(r.FinalPath) {
		return LoginRequired
	}
	if isDocument(r.Head) {
		return Document
	}
	if passwordField.Match(r.Head) {
		return LoginRequired
	}
	if strings.Contains(strings.ToLower(r.ContentType), "text/html") || bytes.HasPrefix(bytes.TrimSpace(r.Head), []byte("<")) {
		return HTMLPage
	}
	return Unclear
}

// isDocument recognises the first bytes of the common document containers: PDF, ZIP (Office
// documents, archives) and the old OLE compound format (.doc, .xls).
func isDocument(head []byte) bool {
	for _, magic := range [][]byte{[]byte("%PDF-"), []byte("PK\x03\x04"), {0xD0, 0xCF, 0x11, 0xE0}} {
		if bytes.HasPrefix(head, magic) {
			return true
		}
	}
	return false
}

// NewClient returns the client the probe uses: a time limit per request, and at most three
// redirects. It keeps no cookies.
func NewClient() *http.Client {
	return &http.Client{
		Timeout: 15 * time.Second,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 3 {
				return http.ErrUseLastResponse
			}
			return nil
		},
	}
}

// Fetch asks the link once, for its first HeadBytes. The request carries the probe's user agent and
// nothing else that identifies anyone.
func Fetch(ctx context.Context, client *http.Client, link string) Result {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, link, nil)
	if err != nil {
		return Result{Err: err}
	}
	req.Header.Set("User-Agent", UserAgent)
	req.Header.Set("Accept", "*/*")
	req.Header.Set("Range", "bytes=0-"+strconv.Itoa(HeadBytes-1))
	resp, err := client.Do(req)
	if err != nil {
		return Result{Err: err}
	}
	defer resp.Body.Close()
	head, _ := io.ReadAll(io.LimitReader(resp.Body, HeadBytes))
	path := ""
	if resp.Request != nil && resp.Request.URL != nil {
		path = resp.Request.URL.Path
	}
	return Result{Status: resp.StatusCode, FinalPath: path, ContentType: resp.Header.Get("Content-Type"), Head: head}
}

// loginPlatforms are the portals that sell or gate tender documents behind a login. They are not
// requested: a probe that signs in, or that asks a platform's pages for documents, is a question
// for a lawyer, and the answer is known in advance.
var loginPlatforms = []string{"mercell.com", "mercell.no", "eu-supply.com", "tendsign.com", "ivalua.com", "achilles.com"}

// Family groups a link by the kind of host it points at, for the summary: a login platform, the
// official register, another Norwegian host, or anything else. It names no host.
func Family(link string) string {
	u, err := url.Parse(link)
	if err != nil || u.Host == "" {
		return "other"
	}
	host := strings.ToLower(u.Hostname())
	for _, p := range loginPlatforms {
		if host == p || strings.HasSuffix(host, "."+p) {
			return "login-platform"
		}
	}
	if host == "doffin.no" || strings.HasSuffix(host, ".doffin.no") {
		return "doffin"
	}
	if strings.HasSuffix(host, ".no") {
		return "norway-other"
	}
	return "other"
}
