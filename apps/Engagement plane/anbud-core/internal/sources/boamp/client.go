// Package boamp reads public procurement notices from BOAMP, the French official bulletin of public
// tenders, for the day-window sync (internal/doffinsync). BOAMP publishes its notices on an open-data
// portal (Opendatasoft Explore v2.1) under the Licence Ouverte 2.0, which needs no key and is read
// anonymously (docs/SOURCES.md). The client speaks the sync engine's types, as the Doffin and TED
// clients do, so the same engine, ledger and store run for all three:
//
//   - Search answers the records published on one day, as the engine's search response. The records of
//     a day are ordered by identifier and paged by offset. The search keeps only each record's
//     identifier, day and family as evidence: the notice itself is the download's.
//   - Download fetches one record by its identifier and keeps its notice as eForms XML (eforms.go). A
//     record that is not an eForms notice (a French national form, which BOAMP also publishes) is kept
//     as its JSON, which the eForms parser refuses, as it refuses any unsupported notice.
//
// Errors are the engine's: a 404 or a record that is not there is doffin.ErrNotFound, a 429 a
// *doffin.RateLimitedError, any other failure status a *doffin.HTTPError, and a response the client
// cannot read a *doffin.DecodeError. Both operations are paced.
package boamp

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
	"time"
	"unicode"

	"golang.org/x/time/rate"

	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	// DefaultRecordsURL is the records endpoint of BOAMP's open-data dataset. Search and download both
	// use it: a download is a search for one identifier.
	DefaultRecordsURL = "https://boamp-datadila.opendatasoft.com/api/explore/v2.1/catalog/datasets/boamp/records"
	// DefaultRPM paces each operation: one call every three seconds.
	DefaultRPM = 20
	// MaxRPM bounds a configured pace.
	MaxRPM = 60
	// maxResponseBytes caps one response. A page of 100 search records is a few kilobytes, a download
	// one record with its eForms JSON.
	maxResponseBytes = 16 << 20
	// maxWindow is the most records one query answers: the portal refuses offset plus limit beyond it.
	maxWindow = 10000
	// fallbackRetryAfter is the wait a 429 without a usable Retry-After asks for.
	fallbackRetryAfter = time.Minute
	maxMessage         = 200
)

// ErrInvalidConfig refuses a configuration the client will not run with.
var ErrInvalidConfig = errors.New("boamp: invalid configuration")

// Config configures a Client. Zero values take the defaults.
type Config struct {
	RecordsURL  string
	SearchRPM   int
	DownloadRPM int
	Timeout     time.Duration
	HTTPClient  *http.Client
	Now         func() time.Time
}

// Client is a paced, anonymous BOAMP reader.
type Client struct {
	records  *url.URL
	http     *http.Client
	search   *rate.Limiter
	download *rate.Limiter
	now      func() time.Time
}

// New checks cfg and builds a client.
func New(cfg Config) (*Client, error) {
	records, err := absoluteHTTPS(orDefault(cfg.RecordsURL, DefaultRecordsURL))
	if err != nil {
		return nil, err
	}
	searchRPM, downloadRPM := orDefaultInt(cfg.SearchRPM, DefaultRPM), orDefaultInt(cfg.DownloadRPM, DefaultRPM)
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

// recordsResponse is the portal's answer to a records query. The donnees of a record is the notice as
// a JSON string.
type recordsResponse struct {
	TotalCount *int64   `json:"total_count"`
	Results    []record `json:"results"`
}

type record struct {
	Idweb        string `json:"idweb"`
	Dateparution string `json:"dateparution"`
	Donnees      string `json:"donnees"`
}

// Search answers one page of one publication day. The window must be a single day, as the engine asks.
func (c *Client) Search(ctx context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	page := 1
	if p.Page != nil {
		page = *p.Page
	}
	switch {
	case p.IssueDateFrom.IsZero() || !sameDay(p.IssueDateFrom, p.IssueDateTo):
		return doffin.SearchResponse{}, fmt.Errorf("%w: BOAMP is searched one publication day at a time", doffin.ErrInvalidParams)
	case page < 1 || p.NumHitsPerPage < 1 || p.NumHitsPerPage > doffin.MaxHitsPerPage:
		return doffin.SearchResponse{}, fmt.Errorf("%w: the page and page size are out of range", doffin.ErrInvalidParams)
	}
	offset := (page - 1) * p.NumHitsPerPage
	if offset+p.NumHitsPerPage > maxWindow {
		return doffin.SearchResponse{}, fmt.Errorf("%w: BOAMP answers the first %d records of a query", doffin.ErrInvalidParams, maxWindow)
	}

	query := url.Values{}
	query.Set("where", fmt.Sprintf("dateparution = date'%s'", p.IssueDateFrom.UTC().Format("2006-01-02")))
	query.Set("order_by", "idweb")
	query.Set("select", "idweb,dateparution,famille")
	query.Set("limit", strconv.Itoa(p.NumHitsPerPage))
	query.Set("offset", strconv.Itoa(offset))
	req, err := c.request(ctx, query, c.search)
	if err != nil {
		return doffin.SearchResponse{}, err
	}
	raw, header, err := c.do(req, "search", maxResponseBytes)
	if err != nil {
		return doffin.SearchResponse{}, err
	}
	var decoded recordsResponse
	if err := json.Unmarshal(raw, &decoded); err != nil {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: errors.New("the response is not BOAMP's records JSON")}
	}
	if decoded.TotalCount == nil || *decoded.TotalCount < 0 {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: errors.New("the response has no record count")}
	}
	out := doffin.SearchResponse{
		NumHitsTotal:      doffin.Count(*decoded.TotalCount),
		NumHitsAccessible: doffin.Count(*decoded.TotalCount),
		Raw:               json.RawMessage(raw),
		ContentType:       header.Get("Content-Type"),
		FetchedAt:         c.now().UTC(),
	}
	for _, r := range decoded.Results {
		out.Hits = append(out.Hits, doffin.Hit{ID: r.Idweb, PublicationDate: dayOf(r.Dateparution)})
	}
	return out, nil
}

// Download fetches one record by its identifier and keeps its notice (noticeBody).
func (c *Client) Download(ctx context.Context, id string) (doffin.Notice, error) {
	if !noticeid.IsBOAMPID(id) {
		return doffin.Notice{}, fmt.Errorf("%w: not a BOAMP identifier", doffin.ErrInvalidParams)
	}
	query := url.Values{}
	query.Set("where", fmt.Sprintf(`idweb="%s"`, id))
	query.Set("select", "idweb,donnees")
	query.Set("limit", "1")
	req, err := c.request(ctx, query, c.download)
	if err != nil {
		return doffin.Notice{}, err
	}
	raw, _, err := c.do(req, "download", maxResponseBytes)
	if err != nil {
		return doffin.Notice{}, err
	}
	var decoded recordsResponse
	if err := json.Unmarshal(raw, &decoded); err != nil {
		return doffin.Notice{}, &doffin.DecodeError{Operation: "download", Err: errors.New("the response is not BOAMP's records JSON")}
	}
	if len(decoded.Results) == 0 || decoded.Results[0].Idweb != id {
		return doffin.Notice{}, doffin.ErrNotFound
	}
	donnees := decoded.Results[0].Donnees
	if donnees == "" {
		return doffin.Notice{}, fmt.Errorf("%w: download %s", doffin.ErrEmptyResponse, id)
	}
	body, contentType := noticeBody(donnees)
	sum := sha256.Sum256(body)
	return doffin.Notice{ID: id, ContentType: contentType, Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: c.now().UTC()}, nil
}

// request is a GET of the records endpoint with query, paced by limiter.
func (c *Client) request(ctx context.Context, query url.Values, limiter *rate.Limiter) (*http.Request, error) {
	if err := limiter.Wait(ctx); err != nil {
		return nil, err
	}
	endpoint := *c.records
	endpoint.RawQuery = query.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
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
		return nil, nil, fmt.Errorf("boamp: %s: BOAMP could not be reached", op)
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

// PublicationDates reads the publication day of each record in a stored search response, keyed by
// identifier. The first answer for an identifier wins: a store reads the newest page first.
func PublicationDates(body []byte) map[string]string {
	var decoded recordsResponse
	dates := map[string]string{}
	if json.Unmarshal(body, &decoded) != nil {
		return dates
	}
	for _, r := range decoded.Results {
		if _, seen := dates[r.Idweb]; seen {
			continue
		}
		if day := dayOf(r.Dateparution); day != "" {
			dates[r.Idweb] = day
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

// dayOf is the calendar day of a BOAMP publication date ("2026-10-29"), or "" when it has none.
func dayOf(s string) string {
	if _, err := time.Parse("2006-01-02", s); err != nil {
		return ""
	}
	return s
}

func absoluteHTTPS(raw string) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || (u.Scheme != "https" && !(u.Scheme == "http" && isLoopback(u.Hostname()))) {
		return nil, fmt.Errorf("%w: a BOAMP URL must be absolute https (http only on loopback, for tests)", ErrInvalidConfig)
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
