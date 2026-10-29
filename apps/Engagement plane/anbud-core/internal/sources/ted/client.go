// Package ted reads published notices from TED, the EU's journal of public procurement, for the
// day-window sync (internal/doffinsync). TED serves published notices anonymously (docs/SOURCES.md):
// no key is sent. The client speaks the sync engine's types, the Doffin client's, so the same engine,
// ledger and store run for both sources:
//
//   - Search takes a one-day window and answers the publication numbers of the notices TED published
//     that day for the configured buyer countries, as the engine's search response.
//   - Download fetches one notice's eForms XML by its publication number.
//
// Errors are the engine's: a 404 is doffin.ErrNotFound, a 429 a *doffin.RateLimitedError, any other
// failure status a *doffin.HTTPError, and a response the client cannot read a *doffin.DecodeError.
// TED's download endpoint rate-limits an unpaced client (a burst of 20 downloads drew five 429s on
// 2026-10-28; one every three seconds drew none), so both operations are paced.
package ted

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
	"time"
	"unicode"

	"golang.org/x/time/rate"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

const (
	// DefaultSearchURL is TED's search API; DefaultNoticeURL serves a notice's XML.
	DefaultSearchURL = "https://api.ted.europa.eu/v3/notices/search"
	DefaultNoticeURL = "https://ted.europa.eu"
	// DefaultRPM paces each operation: one call every three seconds.
	DefaultRPM = 20
	// MaxRPM bounds a configured pace.
	MaxRPM = 60
	// maxSearchBytes caps a search response; a page of 100 publication numbers is a few kilobytes.
	maxSearchBytes = 4 << 20
	// fallbackRetryAfter is the wait a 429 without a usable Retry-After asks for.
	fallbackRetryAfter = time.Minute
	maxMessage         = 200
)

// DefaultCountries are the buyer countries a sync reads when none are configured: the Nordic EEA
// states. Norway's above-threshold notices are on Doffin too, under the same eForms notice
// identifier: both copies are kept as evidence, and the readers show the notice once
// (internal/demo, twins.go).
var DefaultCountries = []string{"NOR", "SWE", "DNK", "FIN", "ISL"}

var countryPattern = regexp.MustCompile(`^[A-Z]{3}$`)

// ErrInvalidConfig refuses a configuration the client will not run with.
var ErrInvalidConfig = errors.New("ted: invalid configuration")

// Config configures a Client. Zero values take the defaults.
type Config struct {
	SearchURL   string
	NoticeURL   string
	Countries   []string // ISO 3166-1 alpha-3 buyer countries, e.g. SWE
	SearchRPM   int
	DownloadRPM int
	Timeout     time.Duration
	HTTPClient  *http.Client
	Now         func() time.Time
}

// Client is a paced, anonymous TED reader.
type Client struct {
	searchURL *url.URL
	noticeURL *url.URL
	countries []string
	http      *http.Client
	search    *rate.Limiter
	download  *rate.Limiter
	now       func() time.Time
}

// New checks cfg and builds a client.
func New(cfg Config) (*Client, error) {
	searchURL, err := absoluteHTTPS(orDefault(cfg.SearchURL, DefaultSearchURL))
	if err != nil {
		return nil, err
	}
	noticeURL, err := absoluteHTTPS(orDefault(cfg.NoticeURL, DefaultNoticeURL))
	if err != nil {
		return nil, err
	}
	countries := cfg.Countries
	if len(countries) == 0 {
		countries = DefaultCountries
	}
	for _, c := range countries {
		if !countryPattern.MatchString(c) {
			return nil, fmt.Errorf("%w: a buyer country must be three capital letters (ISO 3166-1 alpha-3)", ErrInvalidConfig)
		}
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
		searchURL: searchURL,
		noticeURL: noticeURL,
		countries: append([]string(nil), countries...),
		http:      client,
		search:    rate.NewLimiter(rate.Limit(float64(searchRPM)/60), 1),
		download:  rate.NewLimiter(rate.Limit(float64(downloadRPM)/60), 1),
		now:       now,
	}, nil
}

type searchRequest struct {
	Query          string   `json:"query"`
	Fields         []string `json:"fields"`
	Limit          int      `json:"limit"`
	Page           int      `json:"page"`
	Scope          string   `json:"scope"`
	PaginationMode string   `json:"paginationMode"`
}

type searchResponse struct {
	Notices []struct {
		PublicationNumber string `json:"publication-number"`
		PublicationDate   string `json:"publication-date"`
	} `json:"notices"`
	TotalNoticeCount *int64 `json:"totalNoticeCount"`
}

// Query is the expert query of one publication day for the client's buyer countries.
func (c *Client) Query(day time.Time) string {
	return fmt.Sprintf("publication-date=%s AND buyer-country IN (%s)", day.UTC().Format("20060102"), strings.Join(c.countries, " "))
}

// Search answers one page of one publication day. The window must be a single day, as the engine
// asks; the sort is TED's own, and the engine's union of sweeps covers an order that is not stable.
func (c *Client) Search(ctx context.Context, p doffin.SearchParams) (doffin.SearchResponse, error) {
	page := 1
	if p.Page != nil {
		page = *p.Page
	}
	switch {
	case p.IssueDateFrom.IsZero() || !sameDay(p.IssueDateFrom, p.IssueDateTo):
		return doffin.SearchResponse{}, fmt.Errorf("%w: TED is searched one publication day at a time", doffin.ErrInvalidParams)
	case page < 1 || p.NumHitsPerPage < 1 || p.NumHitsPerPage > doffin.MaxHitsPerPage:
		return doffin.SearchResponse{}, fmt.Errorf("%w: the page and page size are out of range", doffin.ErrInvalidParams)
	}
	body, err := json.Marshal(searchRequest{
		Query: c.Query(p.IssueDateFrom), Fields: []string{"publication-number", "publication-date"},
		Limit: p.NumHitsPerPage, Page: page, Scope: "ALL", PaginationMode: "PAGE_NUMBER",
	})
	if err != nil {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: err}
	}
	if err := c.search.Wait(ctx); err != nil {
		return doffin.SearchResponse{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.searchURL.String(), bytes.NewReader(body))
	if err != nil {
		return doffin.SearchResponse{}, fmt.Errorf("%w: %v", doffin.ErrInvalidParams, err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	raw, header, err := c.do(req, "search", maxSearchBytes)
	if err != nil {
		return doffin.SearchResponse{}, err
	}
	var decoded searchResponse
	if err := json.Unmarshal(raw, &decoded); err != nil {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: errors.New("the response is not TED's search JSON")}
	}
	if decoded.TotalNoticeCount == nil || *decoded.TotalNoticeCount < 0 {
		return doffin.SearchResponse{}, &doffin.DecodeError{Operation: "search", Err: errors.New("the response has no notice count")}
	}
	out := doffin.SearchResponse{
		NumHitsTotal:      doffin.Count(*decoded.TotalNoticeCount),
		NumHitsAccessible: doffin.Count(*decoded.TotalNoticeCount),
		Raw:               json.RawMessage(raw),
		ContentType:       header.Get("Content-Type"),
		FetchedAt:         c.now().UTC(),
	}
	for _, n := range decoded.Notices {
		out.Hits = append(out.Hits, doffin.Hit{ID: n.PublicationNumber, PublicationDate: dayOf(n.PublicationDate)})
	}
	return out, nil
}

// Download fetches the eForms XML of one notice by its publication number.
func (c *Client) Download(ctx context.Context, id string) (doffin.Notice, error) {
	if !noticeid.IsTEDID(id) {
		return doffin.Notice{}, fmt.Errorf("%w: not a TED publication number", doffin.ErrInvalidParams)
	}
	if err := c.download.Wait(ctx); err != nil {
		return doffin.Notice{}, err
	}
	endpoint := c.noticeURL.JoinPath("en", "notice", id, "xml")
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return doffin.Notice{}, fmt.Errorf("%w: %v", doffin.ErrInvalidParams, err)
	}
	req.Header.Set("Accept", "application/xml")
	body, header, err := c.do(req, "download", eforms.MaxNoticeBytes)
	if err != nil {
		return doffin.Notice{}, err
	}
	// TED answers some failures with an HTML page and a success status. A body that is not XML is
	// not a notice: refuse it here rather than store a page of HTML as evidence.
	if !looksLikeXML(body) {
		return doffin.Notice{}, &doffin.HTTPError{Operation: "download", Status: http.StatusBadGateway, Message: "the response is not XML"}
	}
	sum := sha256.Sum256(body)
	return doffin.Notice{ID: id, ContentType: header.Get("Content-Type"), Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: c.now().UTC()}, nil
}

// do sends req and reads at most limit bytes of a success.
func (c *Client) do(req *http.Request, op string, limit int64) ([]byte, http.Header, error) {
	resp, err := c.http.Do(req)
	if err != nil {
		if ctxErr := req.Context().Err(); ctxErr != nil {
			return nil, nil, ctxErr
		}
		// No response: a transport error, which the engine counts toward its consecutive-error stop.
		return nil, nil, fmt.Errorf("ted: %s: TED could not be reached", op)
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

func looksLikeXML(body []byte) bool {
	trimmed := bytes.TrimSpace(bytes.TrimPrefix(body, []byte{0xEF, 0xBB, 0xBF}))
	return bytes.HasPrefix(trimmed, []byte("<?xml")) || (bytes.HasPrefix(trimmed, []byte("<")) && !bytes.HasPrefix(bytes.ToLower(trimmed), []byte("<html")) && !bytes.HasPrefix(bytes.ToLower(trimmed), []byte("<!doctype html")))
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

func absoluteHTTPS(raw string) (*url.URL, error) {
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || (u.Scheme != "https" && !(u.Scheme == "http" && isLoopback(u.Hostname()))) {
		return nil, fmt.Errorf("%w: a TED URL must be absolute https (http only on loopback, for tests)", ErrInvalidConfig)
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

// dayOf is the calendar day of a TED publication date ("2026-10-27+01:00"), or "" when it has none.
func dayOf(s string) string {
	if len(s) < 10 {
		return ""
	}
	if _, err := time.Parse("2006-01-02", s[:10]); err != nil {
		return ""
	}
	return s[:10]
}

// PublicationDates reads the publication day of each notice in a stored search response, keyed by
// publication number. The first answer for a number wins: a store reads the newest page first.
func PublicationDates(body []byte) map[string]string {
	var decoded searchResponse
	dates := map[string]string{}
	if json.Unmarshal(body, &decoded) != nil {
		return dates
	}
	for _, n := range decoded.Notices {
		if _, seen := dates[n.PublicationNumber]; seen {
			continue
		}
		if day := dayOf(n.PublicationDate); day != "" {
			dates[n.PublicationNumber] = day
		}
	}
	return dates
}
