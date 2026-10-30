// Package doffin is a client for the Doffin Public API (search and download of
// published notices). It is read-only by construction: the Notices API, which
// publishes legally binding notices, is deliberately out of scope.
//
// One subscription key is a single global budget (30 search and 10 download
// requests per minute), so the client paces itself with a token bucket per
// operation, honours Retry-After, and shares a back-off gate between callers.
// The key travels only in a request header, is never logged, and redirects are
// never followed so it cannot be forwarded to another host.
package doffin

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"

	"golang.org/x/time/rate"

	"github.com/triodelab/anbud/internal/secret"
)

const (
	// DefaultBaseURL is the production Public API.
	DefaultBaseURL = "https://api.doffin.no/public/v2"
	// DefaultHost is the host of DefaultBaseURL. It is the only non-loopback host
	// the subscription keys may be sent to without being named explicitly.
	DefaultHost = "api.doffin.no"
	// KeyHeader carries the API Management subscription key.
	KeyHeader = "Ocp-Apim-Subscription-Key"

	// MaxSearchRPM and MaxDownloadRPM are the documented per-key limits.
	MaxSearchRPM   = 30
	MaxDownloadRPM = 10
	// DefaultSearchRPM and DefaultDownloadRPM keep ~20% headroom under them.
	DefaultSearchRPM   = 24
	DefaultDownloadRPM = 8
	// MaxHitsPerPage is the documented page-size ceiling.
	MaxHitsPerPage = 100

	defaultTimeout          = 30 * time.Second
	defaultMaxRetryAfter    = 2 * time.Minute
	defaultMaxRetries       = 2
	defaultMaxSearchBytes   = 16 << 20
	defaultMaxDownloadBytes = 32 << 20
	fallbackRetryAfter      = 60 * time.Second // the limit window is one minute
	maxRetryAfterHeader     = 24 * time.Hour
	maxErrorMessageRunes    = 200
	defaultUserAgent        = "anbud-core/0.1 (Verevon public-notice ingestion)"
	redactedMarker          = "[redacted]"
)

var (
	// ErrNotConfigured means no subscription key was supplied.
	ErrNotConfigured = errors.New("doffin: API key is not configured")
	// ErrInvalidConfig means the configuration is unusable.
	ErrInvalidConfig = errors.New("doffin: invalid configuration")
	// ErrInvalidParams means a request was rejected locally, before any call.
	ErrInvalidParams = errors.New("doffin: invalid request parameters")
	// ErrUnauthorized means the API rejected the subscription key (HTTP 401/403).
	ErrUnauthorized = errors.New("doffin: subscription key rejected")
	// ErrNotFound means the notice does not exist (HTTP 404).
	ErrNotFound = errors.New("doffin: notice not found")
	// ErrResponseTooLarge means a response exceeded the configured size cap.
	ErrResponseTooLarge = errors.New("doffin: response exceeds the size cap")
	// ErrEmptyResponse means a successful call returned no body.
	ErrEmptyResponse = errors.New("doffin: empty response body")
)

// HTTPError is a non-success response from the API.
type HTTPError struct {
	Operation string
	Status    int
	Message   string // sanitised: key redacted, control characters removed, truncated
}

func (e *HTTPError) Error() string {
	if e.Message == "" {
		return fmt.Sprintf("doffin: %s returned HTTP %d", e.Operation, e.Status)
	}
	return fmt.Sprintf("doffin: %s returned HTTP %d: %s", e.Operation, e.Status, e.Message)
}

// Is lets errors.Is match the sentinel errors by status.
func (e *HTTPError) Is(target error) bool {
	switch target {
	case ErrUnauthorized:
		return e.Status == http.StatusUnauthorized || e.Status == http.StatusForbidden
	case ErrNotFound:
		return e.Status == http.StatusNotFound
	}
	return false
}

// RateLimitedError reports that the API asked us to back off for longer than
// the client is willing to wait inside a call. The caller should reschedule.
type RateLimitedError struct {
	Operation  string
	RetryAfter time.Duration
}

func (e *RateLimitedError) Error() string {
	return fmt.Sprintf("doffin: %s is rate limited; retry after %s", e.Operation, e.RetryAfter)
}

// DecodeError reports a response that arrived intact but does not fit the
// expected shape. The untouched body still comes back in SearchResponse.Raw, so
// the caller keeps the evidence and the shape can be fixed without another call.
type DecodeError struct {
	Operation string
	Err       error
}

func (e *DecodeError) Error() string {
	return fmt.Sprintf("doffin: decode %s response: %v", e.Operation, e.Err)
}

// Unwrap exposes the underlying decoding error.
func (e *DecodeError) Unwrap() error { return e.Err }

// Config configures a Client.
type Config struct {
	BaseURL string
	APIKey  secret.Value

	// SearchRPM and DownloadRPM are requests per minute; 0 selects the default
	// and anything above the documented per-key limit is rejected.
	SearchRPM   int
	DownloadRPM int

	// HTTPClient is optional. Redirect following is always disabled on a copy.
	HTTPClient *http.Client
	// Timeout applies when HTTPClient is nil; default 30s.
	Timeout   time.Duration
	UserAgent string

	// MaxRetryAfter is the longest 429 back-off absorbed inside a call; longer
	// ones return *RateLimitedError. Default 2m.
	MaxRetryAfter time.Duration
	// MaxRetries is the number of retries after a 429. 0 selects the default (2);
	// a negative value disables retrying.
	MaxRetries int

	MaxSearchBytes   int64 // default 16 MiB
	MaxDownloadBytes int64 // default 32 MiB
}

// Budgets are the effective request budgets, per minute.
type Budgets struct {
	SearchRPM   int
	DownloadRPM int
}

type operation string

const (
	opSearch   operation = "search"
	opDownload operation = "download"
)

// Client talks to the Doffin Public API. It is safe for concurrent use.
type Client struct {
	key           secret.Value
	base          *url.URL
	http          *http.Client
	userAgent     string
	budgets       Budgets
	maxRetryAfter time.Duration
	maxRetries    int
	maxBytes      map[operation]int64
	limiters      map[operation]*rate.Limiter

	mu           sync.Mutex
	blockedUntil map[operation]time.Time

	now   func() time.Time
	sleep func(context.Context, time.Duration) error
}

// New validates cfg and builds a Client. It returns ErrNotConfigured when no
// key is present so callers can tell "not set up yet" from "set up wrongly".
func New(cfg Config) (*Client, error) {
	key := strings.TrimSpace(cfg.APIKey.Reveal())
	if key == "" {
		return nil, ErrNotConfigured
	}
	if !validKey(key) {
		return nil, fmt.Errorf("%w: the API key must be 16-256 printable ASCII characters without spaces", ErrInvalidConfig)
	}

	base, err := parseBaseURL(cfg.BaseURL)
	if err != nil {
		return nil, err
	}
	searchRPM, err := budget("SearchRPM", cfg.SearchRPM, DefaultSearchRPM, MaxSearchRPM)
	if err != nil {
		return nil, err
	}
	downloadRPM, err := budget("DownloadRPM", cfg.DownloadRPM, DefaultDownloadRPM, MaxDownloadRPM)
	if err != nil {
		return nil, err
	}

	timeout := cfg.Timeout
	if timeout <= 0 {
		timeout = defaultTimeout
	}
	hc := &http.Client{Timeout: timeout}
	if cfg.HTTPClient != nil {
		copied := *cfg.HTTPClient
		hc = &copied
	}
	// The key is a custom header, which net/http would forward to a redirect
	// target. There is no legitimate redirect, so refuse them all.
	hc.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }

	maxRetryAfter := cfg.MaxRetryAfter
	if maxRetryAfter <= 0 {
		maxRetryAfter = defaultMaxRetryAfter
	}
	maxRetries := cfg.MaxRetries
	switch {
	case maxRetries == 0:
		maxRetries = defaultMaxRetries
	case maxRetries < 0:
		maxRetries = 0
	}
	userAgent := strings.TrimSpace(cfg.UserAgent)
	if userAgent == "" {
		userAgent = defaultUserAgent
	}
	maxSearch := cfg.MaxSearchBytes
	if maxSearch <= 0 {
		maxSearch = defaultMaxSearchBytes
	}
	maxDownload := cfg.MaxDownloadBytes
	if maxDownload <= 0 {
		maxDownload = defaultMaxDownloadBytes
	}

	return &Client{
		key:           secret.Value(key),
		base:          base,
		http:          hc,
		userAgent:     userAgent,
		budgets:       Budgets{SearchRPM: searchRPM, DownloadRPM: downloadRPM},
		maxRetryAfter: maxRetryAfter,
		maxRetries:    maxRetries,
		maxBytes:      map[operation]int64{opSearch: maxSearch, opDownload: maxDownload},
		limiters: map[operation]*rate.Limiter{
			// A burst of one spaces calls evenly, which is safe under both a
			// fixed-window and a sliding-window limiter.
			opSearch:   rate.NewLimiter(perMinute(searchRPM), 1),
			opDownload: rate.NewLimiter(perMinute(downloadRPM), 1),
		},
		blockedUntil: make(map[operation]time.Time),
		now:          time.Now,
		sleep:        sleepContext,
	}, nil
}

// Budgets returns the effective request budgets.
func (c *Client) Budgets() Budgets { return c.budgets }

// String never includes the key.
func (c *Client) String() string {
	return fmt.Sprintf("doffin.Client{base: %s, key: %s, search: %d/min, download: %d/min}",
		c.base, c.key, c.budgets.SearchRPM, c.budgets.DownloadRPM)
}

// GoString never includes the key.
func (c *Client) GoString() string { return c.String() }

// Search runs GET /search.
func (c *Client) Search(ctx context.Context, params SearchParams) (SearchResponse, error) {
	values, err := params.query()
	if err != nil {
		return SearchResponse{}, err
	}
	endpoint := c.base.JoinPath("search")
	endpoint.RawQuery = url.Values(values).Encode()

	body, header, err := c.get(ctx, opSearch, endpoint)
	if err != nil {
		return SearchResponse{}, err
	}
	evidence := SearchResponse{
		Raw:         json.RawMessage(body),
		ContentType: header.Get("Content-Type"),
		FetchedAt:   c.now().UTC(),
	}
	var resp SearchResponse
	if err := json.Unmarshal(body, &resp); err != nil {
		// Hand the raw body back with the error: it is the evidence needed to
		// fix the shape, and a half-decoded page is never returned.
		return evidence, &DecodeError{Operation: string(opSearch), Err: err}
	}
	resp.Raw = evidence.Raw
	resp.ContentType = evidence.ContentType
	resp.FetchedAt = evidence.FetchedAt
	return resp, nil
}

// Download runs GET /download/{doffinId} and returns the notice exactly as
// received. The ID is validated first because it usually comes from another
// system's data and is placed in a URL path.
func (c *Client) Download(ctx context.Context, doffinID string) (Notice, error) {
	if !noticeIDPattern.MatchString(doffinID) {
		return Notice{}, fmt.Errorf("%w: a Doffin notice id looks like 2025-104837", ErrInvalidParams)
	}
	body, header, err := c.get(ctx, opDownload, c.base.JoinPath("download", doffinID))
	if err != nil {
		return Notice{}, err
	}
	if len(body) == 0 {
		return Notice{}, fmt.Errorf("%w: download %s", ErrEmptyResponse, doffinID)
	}
	sum := sha256.Sum256(body)
	return Notice{
		ID:          doffinID,
		ContentType: header.Get("Content-Type"),
		Body:        body,
		SHA256:      hex.EncodeToString(sum[:]),
		FetchedAt:   c.now().UTC(),
	}, nil
}

// get performs a paced GET, absorbing short 429 back-offs.
func (c *Client) get(ctx context.Context, op operation, endpoint *url.URL) ([]byte, http.Header, error) {
	for attempt := 0; ; attempt++ {
		if err := c.waitForBudget(ctx, op); err != nil {
			return nil, nil, err
		}
		resp, body, err := c.roundTrip(ctx, op, endpoint)
		if err != nil {
			return nil, nil, err
		}

		switch {
		case resp.StatusCode == http.StatusTooManyRequests:
			wait := parseRetryAfter(resp.Header.Get("Retry-After"), c.now(), fallbackRetryAfter)
			// Close the gate for every caller, not just this one.
			c.closeGate(op, wait)
			if wait > c.maxRetryAfter || attempt >= c.maxRetries {
				return nil, nil, &RateLimitedError{Operation: string(op), RetryAfter: wait}
			}
		case resp.StatusCode >= 200 && resp.StatusCode < 300:
			return body, resp.Header, nil
		default:
			return nil, nil, &HTTPError{Operation: string(op), Status: resp.StatusCode, Message: c.sanitize(extractMessage(body))}
		}
	}
}

func (c *Client) roundTrip(ctx context.Context, op operation, endpoint *url.URL) (*http.Response, []byte, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return nil, nil, fmt.Errorf("doffin: build %s request: %w", op, err)
	}
	req.Header.Set(KeyHeader, c.key.Reveal())
	req.Header.Set("User-Agent", c.userAgent)
	if op == opSearch {
		req.Header.Set("Accept", "application/json")
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, nil, fmt.Errorf("doffin: %s request failed: %w", op, err)
	}
	defer resp.Body.Close()

	limit := c.maxBytes[op]
	body, err := io.ReadAll(io.LimitReader(resp.Body, limit+1))
	if err != nil {
		return nil, nil, fmt.Errorf("doffin: read %s response: %w", op, err)
	}
	if int64(len(body)) > limit {
		return nil, nil, fmt.Errorf("%w (%s, limit %d bytes)", ErrResponseTooLarge, op, limit)
	}
	return resp, body, nil
}

// waitForBudget blocks until a 429 gate has opened and the token bucket
// allows a request, or until ctx ends.
//
// A gate that will stay closed for longer than MaxRetryAfter is reported as a
// *RateLimitedError instead of being slept through, so a single hostile or
// buggy Retry-After cannot park every caller for hours. The gate is checked
// again after the token wait: a 429 may have landed while this caller queued,
// and spending the token on a request the API has just said it will refuse
// would only earn another 429.
func (c *Client) waitForBudget(ctx context.Context, op operation) error {
	for {
		if remaining := c.gateRemaining(op); remaining > 0 {
			if remaining > c.maxRetryAfter {
				return &RateLimitedError{Operation: string(op), RetryAfter: remaining}
			}
			if err := c.sleep(ctx, remaining); err != nil {
				return c.interruptedBackOff(op, err)
			}
			continue
		}
		if err := c.limiters[op].Wait(ctx); err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			// rate.Limiter refuses up front when the wait would outlast the deadline.
			return fmt.Errorf("doffin: %s budget cannot be met before the deadline: %w", op, context.DeadlineExceeded)
		}
		if c.gateRemaining(op) > 0 {
			continue
		}
		return nil
	}
}

// interruptedBackOff reports a context that ended during a 429 back-off. The
// error wraps the context error and a *RateLimitedError whose RetryAfter is the
// gate still closed at that moment, so the caller can reschedule for the same instant.
func (c *Client) interruptedBackOff(op operation, ctxErr error) error {
	left := max(c.gateRemaining(op), 0)
	return fmt.Errorf("doffin: %s back-off interrupted: %w: %w", op, ctxErr,
		&RateLimitedError{Operation: string(op), RetryAfter: left})
}

// gateRemaining reports how long the 429 gate for op stays closed.
func (c *Client) gateRemaining(op operation) time.Duration {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.blockedUntil[op].Sub(c.now())
}

func (c *Client) closeGate(op operation, d time.Duration) {
	until := c.now().Add(d)
	c.mu.Lock()
	if until.After(c.blockedUntil[op]) {
		c.blockedUntil[op] = until
	}
	c.mu.Unlock()
}

// sanitize makes an upstream message safe to log or return: the key is
// redacted, control characters are dropped, and the length is bounded.
func (c *Client) sanitize(message string) string {
	if message == "" {
		return ""
	}
	// Control characters and invisible format characters (bidi overrides,
	// zero-width spaces) are dropped: both can make text read as something else
	// when it is pasted into a terminal or a chat. They go first, because an
	// upstream could otherwise split the key with them and hide it from the
	// redaction below.
	message = strings.Map(func(r rune) rune {
		if unicode.IsControl(r) || unicode.Is(unicode.Cf, r) {
			return -1
		}
		return r
	}, message)
	message = strings.ReplaceAll(message, c.key.Reveal(), redactedMarker)
	if runes := []rune(message); len(runes) > maxErrorMessageRunes {
		message = string(runes[:maxErrorMessageRunes]) + "…"
	}
	return strings.TrimSpace(message)
}

// extractMessage pulls the "message" field out of an API Management style
// error body. Anything that is not such a JSON object yields "".
func extractMessage(body []byte) string {
	var envelope struct {
		Message string `json:"message"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return ""
	}
	return envelope.Message
}

// parseRetryAfter reads a Retry-After header (delta-seconds or HTTP-date).
func parseRetryAfter(header string, now time.Time, fallback time.Duration) time.Duration {
	header = strings.TrimSpace(header)
	if header == "" {
		return fallback
	}
	if seconds, err := strconv.Atoi(header); err == nil {
		switch {
		case seconds < 0:
			return 0
		case time.Duration(seconds) > maxRetryAfterHeader/time.Second:
			return maxRetryAfterHeader
		}
		return time.Duration(seconds) * time.Second
	}
	if at, err := http.ParseTime(header); err == nil {
		switch d := at.Sub(now); {
		case d <= 0:
			return 0
		case d > maxRetryAfterHeader:
			return maxRetryAfterHeader
		default:
			return d
		}
	}
	return fallback
}

func perMinute(rpm int) rate.Limit { return rate.Limit(float64(rpm) / 60.0) }

func budget(name string, value, def, max int) (int, error) {
	switch {
	case value == 0:
		return def, nil
	case value < 0 || value > max:
		return 0, fmt.Errorf("%w: %s must be between 1 and the documented limit of %d requests per minute", ErrInvalidConfig, name, max)
	}
	return value, nil
}

func validKey(key string) bool {
	if len(key) < 16 || len(key) > 256 {
		return false
	}
	for i := 0; i < len(key); i++ {
		if key[i] < 0x21 || key[i] > 0x7e {
			return false
		}
	}
	return true
}

func parseBaseURL(raw string) (*url.URL, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		raw = DefaultBaseURL
	}
	u, err := url.Parse(raw)
	// The URL is never echoed back: it could carry credentials.
	if err != nil || u.Hostname() == "" {
		return nil, fmt.Errorf("%w: the base URL must be an absolute http(s) URL", ErrInvalidConfig)
	}
	switch u.Scheme {
	case "https":
	case "http":
		if !IsLoopbackHost(u.Hostname()) {
			return nil, fmt.Errorf("%w: the base URL must use https (http is allowed only for loopback hosts)", ErrInvalidConfig)
		}
	default:
		return nil, fmt.Errorf("%w: the base URL scheme %q is not supported", ErrInvalidConfig, u.Scheme)
	}
	if u.User != nil || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" {
		return nil, fmt.Errorf("%w: the base URL must not contain credentials, a query or a fragment", ErrInvalidConfig)
	}
	u.Path = strings.TrimRight(u.Path, "/")
	u.RawPath = ""
	return u, nil
}

// IsLoopbackHost reports whether host, as url.URL.Hostname returns it (no port,
// no brackets), is localhost or a loopback address. It is exact: a look-alike
// such as 127.0.0.1.example.com is not loopback.
func IsLoopbackHost(host string) bool {
	if strings.EqualFold(host, "localhost") {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

func sleepContext(ctx context.Context, d time.Duration) error {
	timer := time.NewTimer(d)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
