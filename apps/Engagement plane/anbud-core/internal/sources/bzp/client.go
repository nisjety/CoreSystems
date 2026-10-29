// Package bzp reads public procurement notices from BZP, the Polish public procurement bulletin, for the
// day-window sync (internal/doffinsync). BZP publishes each notice as a record of its board search, which
// answers anonymously (docs/SOURCES.md). The client speaks the sync engine's types, as the other clients do:
//
//   - Search answers the BZP notices of one day, by their publication date, as the engine's search response.
//     The board answers ten records a page and states no total, and its pages do not always come in the same
//     order, so the client reads a day's pages, keeps the notices it has not seen, and reads the day again
//     while a pass still finds new ones (up to maxPasses). It keeps that day's listing and answers the
//     engine's numbered pages from it, so the total the engine sees is the same on every sweep of the day.
//   - Download fetches one notice by its notice number, with the board's noticeNumber filter, and keeps the
//     record's JSON. Parse (parse.go) reads that JSON into the eForms model the readers share.
//
// Notices the sync does not read (a TED notice of a Polish buyer that the board also lists, or an update or
// a performance notice) are left out of the listing. They are the board's own, and not BZP's to store here.
package bzp

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
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"

	"golang.org/x/time/rate"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	// DefaultRecordsURL is the board's search, which the sync reads by day and, for one notice, by its number.
	DefaultRecordsURL = "https://ezamowienia.gov.pl/mo-board/api/v1/Board/Search"
	// DefaultRPM paces the day's listing: one call every three seconds.
	DefaultRPM = 20
	// DefaultDownloadRPM paces the downloads of single notices.
	DefaultDownloadRPM = 20
	// MaxRPM bounds a configured pace.
	MaxRPM = 60
	// maxResponseBytes caps one page, which is ten records with their JSON.
	maxResponseBytes = 16 << 20
	// pageSize is the most records the board answers a page with; a larger PageSize is cut to ten.
	pageSize = 10
	// maxPages bounds the pages of one pass. The board refuses a window past 10 000 records.
	maxPages = 1000
	// maxPasses bounds the passes over one day. A pass that finds no notice the last one did not ends the walk.
	maxPasses = 3
	// fallbackRetryAfter is the wait a 429 without a usable Retry-After asks for.
	fallbackRetryAfter = time.Minute
	maxMessage         = 200
	// maxAttempts bounds the tries of one request that fails in transit or on the server's side.
	maxAttempts = 3
)

// ErrInvalidConfig refuses a configuration the client will not run with.
var ErrInvalidConfig = errors.New("bzp: invalid configuration")

// errUnreachable is a request that got no answer from the board.
var errUnreachable = errors.New("bzp: the board could not be reached")

// transient reports whether a failed request is worth sending again: no answer, or a server error.
func transient(err error) bool {
	var httpErr *doffin.HTTPError
	return errors.Is(err, errUnreachable) || (errors.As(err, &httpErr) && httpErr.Status >= 500)
}

// sleepCtx waits d, or returns the context's error when it ends first.
func sleepCtx(ctx context.Context, d time.Duration) error {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

// Config configures a Client. Zero values take the defaults.
type Config struct {
	RecordsURL  string
	SearchRPM   int
	DownloadRPM int
	Timeout     time.Duration
	HTTPClient  *http.Client
	Now         func() time.Time
}

// Client is a paced, anonymous BZP reader. It holds the listing of one day at a time.
type Client struct {
	records  *url.URL
	http     *http.Client
	search   *rate.Limiter
	download *rate.Limiter
	now      func() time.Time
	backoff  time.Duration // the wait before the first retry; the n-th retry waits n times it

	mu      sync.Mutex
	listed  string   // the day whose listing is held, YYYY-MM-DD
	listing []listed // that day's notices the sync reads, in the order the board gave them
}

// listed is one notice of a day's listing: what the sync reads from the board's record.
type listed struct {
	ID              string `json:"id"`
	NoticeNumber    string `json:"noticeNumber"`
	PublicationDate string `json:"publicationDate"`
	NoticeType      string `json:"noticeType"`
}

// record is the part of a board record the sync reads to choose it.
type record struct {
	NoticeNumber    string `json:"noticeNumber"`
	BZPNumber       string `json:"bzpNumber"`
	NoticeType      string `json:"noticeType"`
	PublicationDate string `json:"publicationDate"`
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
		backoff:  5 * time.Second,
	}, nil
}

// Search answers one page of one day. The window must be a single day, as the engine asks.
func (c *Client) Search(ctx context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	page := 1
	if p.Page != nil {
		page = *p.Page
	}
	switch {
	case p.IssueDateFrom.IsZero() || !sameDay(p.IssueDateFrom, p.IssueDateTo):
		return doffin.SearchResponse{}, fmt.Errorf("%w: BZP is searched one day at a time", doffin.ErrInvalidParams)
	case page < 1 || p.NumHitsPerPage < 1 || p.NumHitsPerPage > doffin.MaxHitsPerPage:
		return doffin.SearchResponse{}, fmt.Errorf("%w: the page and page size are out of range", doffin.ErrInvalidParams)
	}
	day := p.IssueDateFrom.UTC().Format("2006-01-02")

	c.mu.Lock()
	defer c.mu.Unlock()
	if c.listed != day {
		notices, err := c.walk(ctx, day)
		if err != nil {
			return doffin.SearchResponse{}, err
		}
		c.listed, c.listing = day, notices
	}
	total := len(c.listing)
	start := min((page-1)*p.NumHitsPerPage, total)
	end := min(start+p.NumHitsPerPage, total)
	slice := c.listing[start:end]

	raw, err := json.Marshal(struct {
		Day     string   `json:"day"`
		Page    int      `json:"page"`
		Records []listed `json:"records"`
	}{Day: day, Page: page, Records: slice})
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
	for _, n := range slice {
		out.Hits = append(out.Hits, doffin.Hit{ID: n.ID, PublicationDate: dayOf(n.PublicationDate)})
	}
	return out, nil
}

// walk reads a day's pages until a pass finds no notice the last one did not, and returns the notices the
// sync reads, once each, in the order they were first seen.
func (c *Client) walk(ctx context.Context, day string) ([]listed, error) {
	seen := map[string]bool{}
	var all []listed
	for pass := 1; pass <= maxPasses; pass++ {
		added := 0
		for page := 1; page <= maxPages; page++ {
			recs, err := c.page(ctx, day, page)
			if err != nil {
				return nil, err
			}
			if len(recs) == 0 {
				break
			}
			for _, raw := range recs {
				var r record
				if json.Unmarshal(raw, &r) != nil || !readable(r) || seen[r.NoticeNumber] {
					continue
				}
				id, _ := idOf(r.NoticeNumber)
				seen[r.NoticeNumber] = true
				all = append(all, listed{ID: id, NoticeNumber: r.NoticeNumber, PublicationDate: r.PublicationDate, NoticeType: r.NoticeType})
				added++
			}
		}
		if added == 0 {
			break
		}
	}
	return all, nil
}

// page reads one page of a day's board records, each as the board gave it. An empty page is the end.
func (c *Client) page(ctx context.Context, day string, page int) ([]json.RawMessage, error) {
	q := url.Values{}
	q.Set("publicationDateFrom", day)
	q.Set("publicationDateTo", day)
	q.Set("PageSize", strconv.Itoa(pageSize))
	q.Set("PageNumber", strconv.Itoa(page))
	return c.list(ctx, q, c.search, "search")
}

// list is one GET of the board with query, paced by limiter, read as a JSON array of records. A transport
// failure or a server error is tried again, a few times with a growing wait: the board answered some
// requests after tens of seconds and others not at all, a minute apart (2026-10-29).
func (c *Client) list(ctx context.Context, query url.Values, limiter *rate.Limiter, op string) ([]json.RawMessage, error) {
	if err := limiter.Wait(ctx); err != nil {
		return nil, err
	}
	endpoint := *c.records
	endpoint.RawQuery = query.Encode()
	var body []byte
	for attempt := 1; ; attempt++ {
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
		if err != nil {
			return nil, fmt.Errorf("%w: %v", doffin.ErrInvalidParams, err)
		}
		req.Header.Set("Accept", "application/json")
		body, _, err = c.do(req, op, maxResponseBytes)
		if err == nil || !transient(err) || attempt == maxAttempts {
			if err != nil {
				return nil, err
			}
			break
		}
		if err := sleepCtx(ctx, time.Duration(attempt)*c.backoff); err != nil {
			return nil, err
		}
	}
	var recs []json.RawMessage
	if err := json.Unmarshal(body, &recs); err != nil {
		return nil, &doffin.DecodeError{Operation: op, Err: errors.New("the response is not the board's record list")}
	}
	return recs, nil
}

// Download fetches one notice by its notice number and keeps the record's JSON.
func (c *Client) Download(ctx context.Context, id string) (doffin.Notice, error) {
	number, ok := noticeNumberOf(id)
	if !ok {
		return doffin.Notice{}, fmt.Errorf("%w: not a BZP notice id", doffin.ErrInvalidParams)
	}
	q := url.Values{}
	q.Set("noticeNumber", number)
	q.Set("PageSize", strconv.Itoa(pageSize))
	q.Set("PageNumber", "1")
	recs, err := c.list(ctx, q, c.download, "download")
	if err != nil {
		return doffin.Notice{}, err
	}
	for _, raw := range recs {
		var r record
		if json.Unmarshal(raw, &r) == nil && r.NoticeNumber == number {
			sum := sha256.Sum256(raw)
			return doffin.Notice{ID: id, ContentType: "application/json", Body: raw, SHA256: hex.EncodeToString(sum[:]), FetchedAt: c.now().UTC()}, nil
		}
	}
	return doffin.Notice{}, doffin.ErrNotFound
}

// do sends req and reads at most limit bytes of a success.
func (c *Client) do(req *http.Request, op string, limit int64) ([]byte, http.Header, error) {
	resp, err := c.http.Do(req)
	if err != nil {
		if ctxErr := req.Context().Err(); ctxErr != nil {
			return nil, nil, ctxErr
		}
		// No response: a transport error, which the engine counts toward its consecutive-error stop.
		return nil, nil, fmt.Errorf("%w: %s", errUnreachable, op)
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

// PublicationDates reads the publication day of each notice in a stored listing, keyed by public id's
// source identifier. The first answer for an identifier wins: a store reads the newest page first.
func PublicationDates(body []byte) map[string]string {
	var decoded struct {
		Records []listed `json:"records"`
	}
	dates := map[string]string{}
	if json.Unmarshal(body, &decoded) != nil {
		return dates
	}
	for _, n := range decoded.Records {
		if _, seen := dates[n.ID]; seen {
			continue
		}
		if day := dayOf(n.PublicationDate); day != "" {
			dates[n.ID] = day
		}
	}
	return dates
}

var noticeNumberPattern = regexp.MustCompile(`^([0-9]{4})/BZP ([0-9]{8})/([0-9]{2})(/P)?$`)
var idPattern = regexp.MustCompile(`^([0-9]{4})-([0-9]{8})-([0-9]{2})(-P)?$`)

// idOf is the source identifier of a notice number: 2026/BZP 00505976/01 is 2026-00505976-01, and a planning
// notice's 2025/BZP 00610043/06/P is 2025-00610043-06-P.
func idOf(number string) (string, bool) {
	m := noticeNumberPattern.FindStringSubmatch(number)
	if m == nil {
		return "", false
	}
	id := m[1] + "-" + m[2] + "-" + m[3]
	if m[4] != "" {
		id += "-P"
	}
	return id, true
}

// noticeNumberOf is the notice number of a source identifier: 2026-00505976-01 is 2026/BZP 00505976/01, and
// 2025-00610043-06-P is 2025/BZP 00610043/06/P.
func noticeNumberOf(id string) (string, bool) {
	m := idPattern.FindStringSubmatch(id)
	if m == nil {
		return "", false
	}
	number := m[1] + "/BZP " + m[2] + "/" + m[3]
	if m[4] != "" {
		number += "/P"
	}
	return number, true
}

// readable reports whether the sync reads a board record: a BZP notice of a kind Parse reads, with a notice
// number the identifier can carry.
func readable(r record) bool {
	if !strings.Contains(r.BZPNumber, "/BZP ") {
		return false
	}
	if _, _, ok := kindOf(r.NoticeType); !ok {
		return false
	}
	_, ok := idOf(r.NoticeNumber)
	return ok
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

// dayOf is the UTC calendar day of a board publication instant ("2026-10-28T00:23:39.0895785Z"), or "" when
// it has none.
func dayOf(s string) string {
	t, err := time.Parse(time.RFC3339Nano, s)
	if err != nil {
		return ""
	}
	return t.UTC().Format("2006-01-02")
}

func absoluteHTTPS(raw string) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || (u.Scheme != "https" && !(u.Scheme == "http" && isLoopback(u.Hostname()))) {
		return nil, fmt.Errorf("%w: a BZP URL must be absolute https (http only on loopback, for tests)", ErrInvalidConfig)
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
