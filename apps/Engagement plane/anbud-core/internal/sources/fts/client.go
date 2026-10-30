// Package fts reads public procurement notices from Find a Tender, the UK's public procurement service,
// for the day-window sync (internal/doffinsync). The service publishes each notice as an OCDS release
// package (the Open Contracting Data Standard) and serves them anonymously under the Open Government
// Licence v3.0 (docs/SOURCES.md). The client speaks the sync engine's types, as the other clients do:
//
//   - Search answers the notices of one day, by the day the service last updated them (updatedFrom and
//     updatedTo), as the engine's search response. The service pages by cursor and states no total, so the
//     client walks a day's pages once, keeps that day's listing, and answers the engine's numbered pages
//     from it. The total the engine sees is therefore the same on every sweep of one day.
//   - Download fetches one notice's release package by its notice number and keeps its JSON. Parse
//     (parse.go) reads that JSON into the eForms model the readers share.
package fts

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"

	"golang.org/x/time/rate"

	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	// DefaultRecordsURL is the release package endpoint. A notice is read at DefaultRecordsURL/{number}.
	DefaultRecordsURL = "https://www.find-tender.service.gov.uk/api/1.0/ocdsReleasePackages"
	// DefaultRPM paces the listing: one call every three seconds.
	DefaultRPM = 20
	// DefaultDownloadRPM paces the downloads: one every ten seconds. The service refused a run of ten
	// downloads in thirty seconds on 2026-10-29 (429, Retry-After 120), so the pace is set below that.
	DefaultDownloadRPM = 6
	// MaxRPM bounds a configured pace.
	MaxRPM = 60
	// maxResponseBytes caps one response: a page of listings or one release package.
	maxResponseBytes = 16 << 20
	// listingLimit is the page size the service is asked for when a day is walked.
	listingLimit = 100
	// maxWalkPages bounds the pages of one day. A day of the service holds far fewer releases than this.
	maxWalkPages = 500
	// fallbackRetryAfter is the wait a 429 without a usable Retry-After asks for.
	fallbackRetryAfter = time.Minute
	maxMessage         = 200
)

// ErrInvalidConfig refuses a configuration the client will not run with.
var ErrInvalidConfig = errors.New("fts: invalid configuration")

// Config configures a Client. Zero values take the defaults.
type Config struct {
	RecordsURL  string
	SearchRPM   int
	DownloadRPM int
	Timeout     time.Duration
	HTTPClient  *http.Client
	Now         func() time.Time
}

// Client is a paced, anonymous Find a Tender reader. It holds the listing of one day at a time.
type Client struct {
	records  *url.URL
	http     *http.Client
	search   *rate.Limiter
	download *rate.Limiter
	now      func() time.Time

	mu      sync.Mutex
	listed  string    // the day whose listing is held, YYYY-MM-DD
	listing []listing // that day's releases, in the order the service gave them
}

// listing is one release of a day's listing: what the sync reads from the service's listing.
type listing struct {
	ID   string   `json:"id"`
	OCID string   `json:"ocid"`
	Date string   `json:"date"`
	Tag  []string `json:"tag"`
}

// New checks cfg and builds a client.
func New(cfg Config) (*Client, error) {
	records, err := absoluteHTTPS(orDefault(cfg.RecordsURL, DefaultRecordsURL))
	if err != nil {
		return nil, err
	}
	searchRPM, downloadRPM := orDefaultInt(cfg.SearchRPM, DefaultRPM), orDefaultInt(cfg.DownloadRPM, DefaultDownloadRPM)
	if searchRPM < 1 || searchRPM > MaxRPM || downloadRPM < 1 || downloadRPM > MaxRPM {
		return nil, fmt.Errorf("%w: the pace must be between 1 and %d calls a minute", ErrInvalidConfig, MaxRPM)
	}
	client := cfg.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: orDefaultDuration(cfg.Timeout, 30*time.Second)}
	}
	now := cfg.Now
	if now == nil {
		now = time.Now
	}
	return &Client{
		records:  records,
		http:     client,
		search:   rate.NewLimiter(rate.Limit(float64(searchRPM)/60), 1),
		download: rate.NewLimiter(rate.Limit(float64(downloadRPM)/60), 1),
		now:      now,
	}, nil
}

// listingResponse is one page of the service's listing: its releases and the link to the next page.
type listingResponse struct {
	Releases []listing `json:"releases"`
	Links    struct {
		Next string `json:"next"`
	} `json:"links"`
}

// Search answers one page of one day. The window must be a single day, as the engine asks.
func (c *Client) Search(ctx context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	page := 1
	if p.Page != nil {
		page = *p.Page
	}
	switch {
	case p.IssueDateFrom.IsZero() || !sameDay(p.IssueDateFrom, p.IssueDateTo):
		return doffin.SearchResponse{}, fmt.Errorf("%w: Find a Tender is searched one day at a time", doffin.ErrInvalidParams)
	case page < 1 || p.NumHitsPerPage < 1 || p.NumHitsPerPage > doffin.MaxHitsPerPage:
		return doffin.SearchResponse{}, fmt.Errorf("%w: the page and page size are out of range", doffin.ErrInvalidParams)
	}
	day := p.IssueDateFrom.UTC().Format("2006-01-02")

	c.mu.Lock()
	defer c.mu.Unlock()
	if c.listed != day {
		releases, err := c.walk(ctx, p.IssueDateFrom.UTC())
		if err != nil {
			return doffin.SearchResponse{}, err
		}
		c.listed, c.listing = day, releases
	}
	total := len(c.listing)
	start := min((page-1)*p.NumHitsPerPage, total)
	end := min(start+p.NumHitsPerPage, total)
	slice := c.listing[start:end]

	raw, err := json.Marshal(struct {
		Day      string    `json:"day"`
		Page     int       `json:"page"`
		Releases []listing `json:"releases"`
	}{Day: day, Page: page, Releases: slice})
	if err != nil {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: err}
	}
	out := doffin.SearchResponse{
		NumHitsTotal:      doffin.Count(total),
		NumHitsAccessible: doffin.Count(total),
		Raw:               json.RawMessage(raw),
		ContentType:       "application/json",
		FetchedAt:         c.now().UTC(),
	}
	for _, r := range slice {
		out.Hits = append(out.Hits, doffin.Hit{ID: r.ID, PublicationDate: dayOf(r.Date)})
	}
	return out, nil
}

// walk reads every page of the listing of the day that starts at from, in the order the service gives.
func (c *Client) walk(ctx context.Context, from time.Time) ([]listing, error) {
	next := c.listingURL(from)
	var all []listing
	for n := 1; next != ""; n++ {
		if n > maxWalkPages {
			return nil, &doffin.DecodeError{Operation: "search", Err: errors.New("the day holds more pages than the sync reads")}
		}
		if err := c.search.Wait(ctx); err != nil {
			return nil, err
		}
		req, err := c.request(ctx, next)
		if err != nil {
			return nil, err
		}
		raw, _, err := c.do(req, "search", maxResponseBytes)
		if err != nil {
			return nil, err
		}
		var page listingResponse
		if err := json.Unmarshal(raw, &page); err != nil {
			return nil, &doffin.DecodeError{Operation: "search", Err: errors.New("the response is not Find a Tender's listing JSON")}
		}
		if len(page.Releases) == 0 {
			break
		}
		all = append(all, page.Releases...)
		next = ""
		if page.Links.Next != "" {
			if next, err = c.sameService(page.Links.Next); err != nil {
				return nil, err
			}
		}
	}
	return all, nil
}

// listingURL is the first page of the listing of the day that starts at from.
func (c *Client) listingURL(from time.Time) string {
	q := url.Values{}
	q.Set("updatedFrom", from.Format("2006-01-02T15:04:05"))
	q.Set("updatedTo", from.AddDate(0, 0, 1).Format("2006-01-02T15:04:05"))
	q.Set("limit", strconv.Itoa(listingLimit))
	endpoint := *c.records
	endpoint.RawQuery = q.Encode()
	return endpoint.String()
}

// sameService refuses a link to another host: the service's next link is followed only on the service.
func (c *Client) sameService(link string) (string, error) {
	u, err := url.Parse(link)
	if err != nil || u.Scheme != c.records.Scheme || u.Host != c.records.Host {
		return "", &doffin.DecodeError{Operation: "search", Err: errors.New("the listing links to another host")}
	}
	return u.String(), nil
}

// Download fetches the release package of one notice by its notice number.
func (c *Client) Download(ctx context.Context, id string) (doffin.Notice, error) {
	if !noticeid.IsFTSID(id) {
		return doffin.Notice{}, fmt.Errorf("%w: not a Find a Tender notice number", doffin.ErrInvalidParams)
	}
	if err := c.download.Wait(ctx); err != nil {
		return doffin.Notice{}, err
	}
	req, err := c.request(ctx, c.records.JoinPath(id).String())
	if err != nil {
		return doffin.Notice{}, err
	}
	body, _, err := c.do(req, "download", maxResponseBytes)
	if err != nil {
		return doffin.Notice{}, err
	}
	if !json.Valid(body) {
		return doffin.Notice{}, &doffin.DecodeError{Operation: "download", Err: errors.New("the response is not JSON")}
	}
	if !holdsNotice(body, id) {
		return doffin.Notice{}, &doffin.DecodeError{Operation: "download", Err: errors.New("the release package does not hold the notice asked for")}
	}
	sum := sha256.Sum256(body)
	return doffin.Notice{ID: id, ContentType: "application/json", Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: c.now().UTC()}, nil
}

// holdsNotice reports whether a release package has a release with the notice number id.
func holdsNotice(body []byte, id string) bool {
	var pkg struct {
		Releases []struct {
			ID string `json:"id"`
		} `json:"releases"`
	}
	if json.Unmarshal(body, &pkg) != nil {
		return false
	}
	for _, r := range pkg.Releases {
		if r.ID == id {
			return true
		}
	}
	return false
}

// request is a GET of u with the JSON accept header.
func (c *Client) request(ctx context.Context, u string) (*http.Request, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u, nil)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", doffin.ErrInvalidParams, err)
	}
	req.Header.Set("Accept", "application/json")
	return req, nil
}

// do sends req and reads at most limit bytes of a success.
func (c *Client) do(req *http.Request, op string, limit int64) ([]byte, http.Header, error) {
	resp, err := c.http.Do(req)
	if err != nil {
		if ctxErr := req.Context().Err(); ctxErr != nil {
			return nil, nil, ctxErr
		}
		// No response: a transport error, which the engine counts toward its consecutive-error stop.
		return nil, nil, fmt.Errorf("fts: %s: Find a Tender could not be reached", op)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, limit+1))
	if err != nil {
		return nil, nil, &doffin.DecodeError{Operation: op, Err: errors.New("the response could not be read")}
	}
	switch {
	case resp.StatusCode == http.StatusNotFound:
		return nil, nil, doffin.ErrNotFound
	case resp.StatusCode == http.StatusTooManyRequests:
		return nil, nil, &doffin.RateLimitedError{Operation: op, RetryAfter: retryAfter(resp.Header.Get("Retry-After"), c.now())}
	case resp.StatusCode < 200 || resp.StatusCode > 299:
		return nil, nil, &doffin.HTTPError{Operation: op, Status: resp.StatusCode, Message: sanitize(string(body))}
	case int64(len(body)) > limit:
		return nil, nil, doffin.ErrResponseTooLarge
	case len(bytes.TrimSpace(body)) == 0:
		return nil, nil, doffin.ErrEmptyResponse
	}
	return body, resp.Header, nil
}

// PublicationDates reads the day each notice was last updated on, from a stored listing, keyed by notice
// number. The first answer for a number wins: a store reads the newest page first.
func PublicationDates(body []byte) map[string]string {
	var decoded struct {
		Releases []listing `json:"releases"`
	}
	dates := map[string]string{}
	if json.Unmarshal(body, &decoded) != nil {
		return dates
	}
	for _, r := range decoded.Releases {
		if _, seen := dates[r.ID]; seen {
			continue
		}
		if day := dayOf(r.Date); day != "" {
			dates[r.ID] = day
		}
	}
	return dates
}

// retryAfter reads delta-seconds or an HTTP date, bounded to an hour; anything else is a minute.
func retryAfter(v string, now time.Time) time.Duration {
	v = strings.TrimSpace(v)
	if s, err := strconv.Atoi(v); err == nil && s > 0 {
		return min(time.Duration(s)*time.Second, time.Hour)
	}
	if t, err := http.ParseTime(v); err == nil && t.After(now) {
		return min(t.Sub(now), time.Hour)
	}
	return fallbackRetryAfter
}

// sanitize keeps a short, printable message from an error body.
func sanitize(s string) string {
	s = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return ' '
		}
		return r
	}, s)
	s = strings.Join(strings.Fields(s), " ")
	if len(s) > maxMessage {
		s = s[:maxMessage]
	}
	return s
}

func sameDay(a, b time.Time) bool {
	a, b = a.UTC(), b.UTC()
	return a.Year() == b.Year() && a.YearDay() == b.YearDay()
}

// dayOf is the UTC calendar day of an RFC 3339 instant ("2026-10-27T22:02:03Z"), or "" when it has none.
func dayOf(s string) string {
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return ""
	}
	return t.UTC().Format("2006-01-02")
}

func absoluteHTTPS(raw string) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || (u.Scheme != "https" && !(u.Scheme == "http" && isLoopback(u.Hostname()))) {
		return nil, fmt.Errorf("%w: a Find a Tender URL must be absolute https (http only on loopback, for tests)", ErrInvalidConfig)
	}
	return u, nil
}

func isLoopback(host string) bool { return host == "127.0.0.1" || host == "localhost" || host == "::1" }

func orDefault(s, d string) string {
	if s == "" {
		return d
	}
	return s
}

func orDefaultInt(n, d int) int {
	if n == 0 {
		return d
	}
	return n
}

func orDefaultDuration(t, d time.Duration) time.Duration {
	if t == 0 {
		return d
	}
	return t
}
