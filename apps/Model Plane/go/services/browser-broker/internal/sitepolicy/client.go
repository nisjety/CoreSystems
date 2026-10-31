// Package sitepolicy is the browser broker's pull of org-core's agent-browser
// site decision (WS21-S4, DEC-19). org-core is the policy of record; the broker
// asks it at grant mint, for the verified caller, and never substitutes a
// caller-supplied answer. Any failure to read the answer is ErrUnavailable, and
// the server refuses the mint rather than guessing.
package sitepolicy

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"
)

// Mode says how the broker uses the site policy. Off is the default: no call is
// made and every grant is issued as requested.
type Mode string

const (
	ModeOff     Mode = "off"
	ModeShadow  Mode = "shadow"
	ModeEnforce Mode = "enforce"
)

// ParseMode reads BROWSER_BROKER_SITE_POLICY. Empty and off are off, shadow is
// shadow, and any other value is enforce: a typo narrows access instead of
// opening it.
func ParseMode(raw string) Mode {
	switch strings.TrimSpace(raw) {
	case "", string(ModeOff):
		return ModeOff
	case string(ModeShadow):
		return ModeShadow
	default:
		return ModeEnforce
	}
}

// The answers org-core resolves a host to, as org-core's browsersites package
// names them.
const (
	EffectiveDeny = "deny"
	EffectiveAsk  = "ask"
	EffectiveRead = "read"
	EffectiveAct  = "act"
)

// ErrUnavailable means the site decision could not be read: the transport
// failed, org-core answered with a non-200 status, or the body was not the
// expected shape. The message never carries the URL or the token.
var ErrUnavailable = errors.New("browser site policy is unavailable")

// Decision is org-core's answer for one canonical host.
type Decision struct {
	Host      string `json:"host"`
	Effective string `json:"effective"`
}

// Resolution is org-core's answer for a set of hosts for one caller.
type Resolution struct {
	Decisions []Decision `json:"decisions"`
}

// Resolver answers the site decision for hosts for one member. An empty userID
// asks for the admin's answer alone.
type Resolver interface {
	Resolve(ctx context.Context, orgID, userID string, hosts []string) (Resolution, error)
}

const (
	// resolveTimeout keeps a slow org-core from holding a grant mint open.
	resolveTimeout = 2 * time.Second
	// positiveTTL bounds how long an allowed answer is reused. A denial is
	// never cached, so a block takes effect on the next mint after org-core
	// answers; an allow can lag an admin's block by at most this long.
	positiveTTL = 30 * time.Second
	// maxResolveBody bounds one answer; at most 32 hosts are asked for.
	maxResolveBody = 64 << 10
	// maxCachedDecisions bounds the positive cache. It is cleared when full,
	// which only costs extra calls.
	maxCachedDecisions = 4096
)

// Client reads the resolve route with the broker's service credential and keeps
// a short cache of allowed answers.
type Client struct {
	baseURL   string
	serviceID string
	token     string
	http      *http.Client
	now       func() time.Time

	mu    sync.Mutex
	cache map[cacheKey]cacheEntry
}

type cacheKey struct {
	orgID, userID, host string
}

type cacheEntry struct {
	effective string
	expires   time.Time
}

// NewClient builds the client for org-core at baseURL. It refuses an empty URL,
// service identity or token, so a missing secret stops the broker at boot.
func NewClient(baseURL, serviceID, token string) (*Client, error) {
	return newClient(baseURL, serviceID, token, resolveTimeout, time.Now)
}

func newClient(baseURL, serviceID, token string, timeout time.Duration, now func() time.Time) (*Client, error) {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	switch {
	case baseURL == "":
		return nil, errors.New("org-core url is required")
	case serviceID == "" || token == "":
		return nil, errors.New("org-core service identity and token are required")
	}
	return &Client{
		baseURL:   baseURL,
		serviceID: serviceID,
		token:     token,
		http: &http.Client{
			Timeout: timeout,
			// The token must never follow a redirect to another host.
			CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse },
		},
		now:   now,
		cache: make(map[cacheKey]cacheEntry),
	}, nil
}

// Resolve answers hosts from the cache when every host has a live allowed
// answer, and otherwise asks org-core for all of them.
func (c *Client) Resolve(ctx context.Context, orgID, userID string, hosts []string) (Resolution, error) {
	if cached, ok := c.cached(orgID, userID, hosts); ok {
		return cached, nil
	}
	fresh, err := c.fetch(ctx, orgID, userID, hosts)
	if err != nil {
		return Resolution{}, err
	}
	c.remember(orgID, userID, fresh)
	return fresh, nil
}

func (c *Client) cached(orgID, userID string, hosts []string) (Resolution, bool) {
	if len(hosts) == 0 {
		return Resolution{}, false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	now := c.now()
	out := Resolution{Decisions: make([]Decision, 0, len(hosts))}
	for _, host := range hosts {
		entry, ok := c.cache[cacheKey{orgID, userID, host}]
		if !ok || !entry.expires.After(now) {
			return Resolution{}, false
		}
		out.Decisions = append(out.Decisions, Decision{Host: host, Effective: entry.effective})
	}
	return out, true
}

func (c *Client) remember(orgID, userID string, resolution Resolution) {
	c.mu.Lock()
	defer c.mu.Unlock()
	now := c.now()
	if len(c.cache)+len(resolution.Decisions) > maxCachedDecisions {
		c.cache = make(map[cacheKey]cacheEntry)
	}
	for _, decision := range resolution.Decisions {
		if decision.Effective != EffectiveRead && decision.Effective != EffectiveAct {
			continue
		}
		c.cache[cacheKey{orgID, userID, decision.Host}] = cacheEntry{
			effective: decision.Effective,
			expires:   now.Add(positiveTTL),
		}
	}
}

func (c *Client) fetch(ctx context.Context, orgID, userID string, hosts []string) (Resolution, error) {
	query := url.Values{}
	for _, host := range hosts {
		query.Add("host", host)
	}
	if userID != "" {
		query.Set("user_id", userID)
	}
	target := c.baseURL + "/internal/orgs/" + url.PathEscape(orgID) + "/browser-sites/resolve?" + query.Encode()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, target, nil)
	if err != nil {
		return Resolution{}, ErrUnavailable
	}
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Service-Id", c.serviceID)
	request.Header.Set("X-Service-Token", c.token)

	response, err := c.http.Do(request)
	if err != nil {
		// The transport error would embed the URL; the cause is enough.
		return Resolution{}, ErrUnavailable
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxResolveBody))
		return Resolution{}, ErrUnavailable
	}
	var resolution Resolution
	if err := json.NewDecoder(io.LimitReader(response.Body, maxResolveBody)).Decode(&resolution); err != nil {
		return Resolution{}, ErrUnavailable
	}
	return resolution, nil
}
