package gdpr

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/rs/zerolog"
)

// EdgePurgeTimeout bounds one call to the quarry-edge erasure route (WS17.E3).
const EdgePurgeTimeout = 5 * time.Second

// maxEdgeResponseBytes caps what the edge's answer may cost to read.
const maxEdgeResponseBytes = 1 << 20

// The edge hop reports only fixed messages. A transport or URL error carries the
// endpoint, which carries the org id, so it is never wrapped into an error here.
var (
	errEdgeRequest     = errors.New("gdpr: edge purge request could not be built")
	errEdgeUnreachable = errors.New("gdpr: edge purge call failed")
	errEdgeResponse    = errors.New("gdpr: edge purge response could not be read")
)

// EdgePurgeFamily is one erasure family's counts, as quarry-edge reports them.
type EdgePurgeFamily struct {
	Family            string `json:"family"`
	RowsDeleted       int64  `json:"rows_deleted"`
	TombstonesDeleted int64  `json:"tombstones_deleted"`
	CacheKeysDeleted  int64  `json:"cache_keys_deleted"`
	KeysEvicted       bool   `json:"keys_evicted"`
}

// EdgePurgeResult is the edge's answer for one org: one entry per family.
type EdgePurgeResult struct {
	Families []EdgePurgeFamily
}

// Total is the number of rows, tombstones and cache keys removed across families.
func (r EdgePurgeResult) Total() int64 {
	var n int64
	for _, f := range r.Families {
		n += f.RowsDeleted + f.TombstonesDeleted + f.CacheKeysDeleted
	}
	return n
}

// EdgePurger purges an org's data held by quarry-edge. A nil EdgePurger means
// the hop is off (QUARRY_EDGE_URL unset); nothing in quarry-edge is purged.
type EdgePurger interface {
	PurgeEdgeOrg(orgID string) (EdgePurgeResult, error)
}

// HTTPEdgePurger calls DELETE {base}/v1/internal/orgs/{org}/profiles on
// quarry-edge with the shared internal token. Any failure returns an error,
// which the subscriber turns into a NAK, so JetStream redelivers the event.
type HTTPEdgePurger struct {
	baseURL string
	token   string
	client  *http.Client
}

// NewHTTPEdgePurger builds the HTTP hop. baseURL is an http(s) origin with no
// trailing slash.
func NewHTTPEdgePurger(baseURL, token string, client *http.Client) *HTTPEdgePurger {
	if client == nil {
		client = &http.Client{Timeout: EdgePurgeTimeout}
	}
	return &HTTPEdgePurger{baseURL: strings.TrimRight(baseURL, "/"), token: token, client: client}
}

// PurgeEdgeOrg deletes the org's profile families in quarry-edge. Idempotent.
func (p *HTTPEdgePurger) PurgeEdgeOrg(orgID string) (EdgePurgeResult, error) {
	ctx, cancel := context.WithTimeout(context.Background(), EdgePurgeTimeout)
	defer cancel()
	endpoint := p.baseURL + "/v1/internal/orgs/" + url.PathEscape(orgID) + "/profiles"
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, endpoint, nil)
	if err != nil {
		return EdgePurgeResult{}, errEdgeRequest
	}
	req.Header.Set("Authorization", "Bearer "+p.token)
	req.Header.Set("Accept", "application/json")
	resp, err := p.client.Do(req)
	if err != nil {
		return EdgePurgeResult{}, errEdgeUnreachable
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxEdgeResponseBytes))
	if err != nil {
		return EdgePurgeResult{}, errEdgeResponse
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return EdgePurgeResult{}, fmt.Errorf("gdpr: edge purge refused with status %d", resp.StatusCode)
	}
	var envelope struct {
		Data struct {
			Families []EdgePurgeFamily `json:"families"`
		} `json:"data"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return EdgePurgeResult{}, errEdgeResponse
	}
	return EdgePurgeResult{Families: envelope.Data.Families}, nil
}

// failingEdgePurger is what a misconfigured hop returns. The erasure is refused
// on every delivery and retried, so the gap stays visible until the config is fixed.
type failingEdgePurger struct{ err error }

func (f failingEdgePurger) PurgeEdgeOrg(string) (EdgePurgeResult, error) {
	return EdgePurgeResult{}, f.err
}

// EdgePurgerFromEnv builds the edge hop from the environment. QUARRY_EDGE_URL
// unset means the hop is off and it returns nil. A set URL with a missing token
// or a non-http(s) URL returns a purger that fails every call, and logs why.
func EdgePurgerFromEnv(getenv func(string) string, logger zerolog.Logger) EdgePurger {
	base := strings.TrimRight(strings.TrimSpace(getenv("QUARRY_EDGE_URL")), "/")
	if base == "" {
		return nil
	}
	token := strings.TrimSpace(getenv("QUARRY_EDGE_INTERNAL_TOKEN"))
	if token == "" {
		logger.Error().Msg("gdpr: QUARRY_EDGE_URL is set but QUARRY_EDGE_INTERNAL_TOKEN is empty; org erasures will be retried until fixed")
		return failingEdgePurger{err: errors.New("gdpr: edge purge is not configured (token)")}
	}
	u, err := url.Parse(base)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" {
		logger.Error().Msg("gdpr: QUARRY_EDGE_URL is not an http(s) URL; org erasures will be retried until fixed")
		return failingEdgePurger{err: errors.New("gdpr: edge purge is not configured (url)")}
	}
	logger.Info().Msg("gdpr: quarry-edge erasure hop enabled")
	return NewHTTPEdgePurger(base, token, nil)
}
