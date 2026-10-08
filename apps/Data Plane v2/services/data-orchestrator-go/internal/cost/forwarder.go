package cost

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/rs/zerolog/log"
)

// The forwarder posts every version 2 usage row to cost-core, so the Data Plane's spend
// reaches the ledger the product's cost page and budgets read (design task CR-T10).
//
// It is DB-truth: the rows of cost_events are the queue and the table records what was
// forwarded, so a restart, a crash between the POST and the bookkeeping, or two replicas
// cannot lose or duplicate anything that matters. A row is forwarded at least once and
// cost-core records it once, because the key the row is posted under is unique there.
//
// # Ships dark
//
// Off unless USAGE_FORWARDER_ENABLED=1, and it must stay off until cost-core accepts what it
// posts. cost-core decodes strictly (an unknown field is a 400), so a forwarder that runs
// before the build with the `feature` and `units` fields (CR-T4) is rejected row by row,
// and, worse, one that left the feature out would put background spend where chat budgets
// read it. The body therefore always carries the feature and never the other way round.
// Enabling also needs the catalogue prices of the models the Data Plane meters: a model
// without one is priced at cost-core's default rate, and an embedding model at that rate
// would be about 25 times too high. The runbook has the order.
//
// # Where the numbers come from
//
// Tokens are posted as the row holds them: exact for a metered row, the Data Plane's
// estimate for an estimated one (posted with no cost basis, which cost-core files under
// "unknown" instead of "metered", so a report never presents an estimate as a measurement).
// No price is computed here: cost-core prices from its catalogue (`cost_usd` is not sent),
// and posts `units` for what is not tokens.
//
// # Attribution
//
// The row is posted under the principal's own identity (the token's subject becomes the
// producer id), with no user: background spend booked to a person would count against that
// person's chat budget. The organization comes from the token, which Auth Core minted for
// that organization and cost-core pins the write to.

const (
	// keyPrefix is put in front of the stored key (organization first) when it is posted.
	// cost-core's unique index on the key is global, not per organization; the stored key
	// already starts with the organization, so a key can never be recorded under another.
	keyPrefix = "dataplane:"

	recordPath = "/api/v1/cost/record"

	maxRecordResponse = 16 * 1024

	// The closed classes of a failed forward, as migration 20261003140000 enumerates them.
	failRejected         = "rejected"
	failUnauthorized     = "unauthorized"
	failUnavailable      = "unavailable"
	failTokenUnavailable = "token_unavailable"
)

// PendingRow is one usage row waiting to be forwarded.
type PendingRow struct {
	ID              int64
	OrgID           string
	EventType       string
	Model           string
	Feature         string
	ProviderUsed    string
	InputTokens     int64
	OutputTokens    int64
	CacheReadTokens int64
	Units           *int64
	CostBasis       string
	Estimated       bool
	RequestID       string
	IdempotencyKey  string
	Attempts        int
	CreatedAt       time.Time
}

// RecordRequest is the JSON body posted to cost-core's POST /api/v1/cost/record. It is the
// forwarder's whole contract with cost-core: cost-core decodes it strictly, so every field
// here must be one cost-core names, and testdata/forwarder-record-request.golden.json pins
// the exact bytes this struct produces for reference rows. cost-core's own tests post that
// golden through its real handler, authorizer and decoder (task CR-T4 vendors the file).
//
// Fields that are not sent are not named: user_id and run_id (the row has neither), cost_usd
// (cost-core prices from its catalogue) and cache_creation_input_tokens (the ledger carries
// cache reads only; inference-core already folds both into the input count).
type RecordRequest struct {
	OrgID                string `json:"org_id"`
	RequestID            string `json:"request_id"`
	Model                string `json:"model"`
	ProviderUsed         string `json:"provider_used"`
	InputTokens          int64  `json:"input_tokens"`
	OutputTokens         int64  `json:"output_tokens"`
	CacheReadInputTokens int64  `json:"cache_read_input_tokens"`
	CostBasis            string `json:"cost_basis,omitempty"`
	IdempotencyKey       string `json:"idempotency_key"`
	Feature              string `json:"feature"`
	Units                *int64 `json:"units,omitempty"`
}

// BuildRecordRequest is the body for one row. Estimated rows carry no cost basis.
func BuildRecordRequest(row PendingRow) RecordRequest {
	basis := ""
	if !row.Estimated && row.CostBasis == "metered" {
		basis = "metered"
	}
	return RecordRequest{
		OrgID:                row.OrgID,
		RequestID:            row.RequestID,
		Model:                row.Model,
		ProviderUsed:         row.ProviderUsed,
		InputTokens:          row.InputTokens,
		OutputTokens:         row.OutputTokens,
		CacheReadInputTokens: row.CacheReadTokens,
		CostBasis:            basis,
		IdempotencyKey:       keyPrefix + row.IdempotencyKey,
		Feature:              row.Feature,
		Units:                row.Units,
	}
}

// ForwarderConfig is what the forwarder is run with.
type ForwarderConfig struct {
	// BaseURL is cost-core's HTTP API (http://cost-core:8089).
	BaseURL string
	// BatchSize is how many rows one pass claims.
	BatchSize int
	// Interval is the pause between passes that found nothing to do.
	Interval time.Duration
	// Lease is how long a claimed row is held for one worker. It must outlast one pass.
	Lease time.Duration
	// Timeout bounds one POST.
	Timeout time.Duration
}

func (c ForwarderConfig) withDefaults() ForwarderConfig {
	if c.BatchSize <= 0 {
		c.BatchSize = 50
	}
	if c.Interval <= 0 {
		c.Interval = 15 * time.Second
	}
	if c.Lease <= 0 {
		c.Lease = 2 * time.Minute
	}
	if c.Timeout <= 0 {
		c.Timeout = 10 * time.Second
	}
	return c
}

// ForwardStore is where the queue lives.
type ForwardStore interface {
	// Claim leases up to limit rows that are due, version 2 and not forwarded, oldest first.
	Claim(ctx context.Context, limit int, lease time.Duration) ([]PendingRow, error)
	MarkForwarded(ctx context.Context, id int64) error
	// MarkFailed records a failed attempt and when to try again.
	MarkFailed(ctx context.Context, id int64, class string, next time.Time) error
	// Backlog is how many rows wait and how long the oldest has waited.
	Backlog(ctx context.Context) (pending int64, oldest time.Duration, err error)
}

// Forwarder drains the queue to cost-core.
type Forwarder struct {
	store  ForwardStore
	tokens TokenSource
	client *http.Client
	config ForwarderConfig
	now    func() time.Time
}

func NewForwarder(store ForwardStore, tokens TokenSource, client *http.Client, config ForwarderConfig) (*Forwarder, error) {
	config = config.withDefaults()
	if store == nil || tokens == nil || strings.TrimSpace(config.BaseURL) == "" {
		return nil, errors.New("the usage forwarder needs a store, a token source and cost-core's address")
	}
	if client == nil {
		client = &http.Client{CheckRedirect: refuseRedirect}
	}
	return &Forwarder{store: store, tokens: tokens, client: client, config: config, now: time.Now}, nil
}

// PassResult is what one pass did.
type PassResult struct {
	Claimed   int
	Forwarded int
	Failed    int
}

// RunOnce claims a batch and forwards it. The rows of one organization share a bearer, asked
// for once per pass; an organization whose bearer cannot be had fails its own rows with
// token_unavailable and does not stop the others. A row is posted at least once and cost-core
// records it once (the key it is posted under is unique there), so a pass that outlives its
// lease, or one that dies between the POST and the bookkeeping, costs a repeat and nothing
// else.
func (f *Forwarder) RunOnce(ctx context.Context) (PassResult, error) {
	rows, err := f.store.Claim(ctx, f.config.BatchSize, f.config.Lease)
	if err != nil {
		return PassResult{}, fmt.Errorf("claim usage rows: %w", err)
	}
	result := PassResult{Claimed: len(rows)}
	byOrg := make(map[string][]PendingRow)
	var orgs []string
	for _, row := range rows {
		if _, seen := byOrg[row.OrgID]; !seen {
			orgs = append(orgs, row.OrgID)
		}
		byOrg[row.OrgID] = append(byOrg[row.OrgID], row)
	}
	for _, org := range orgs {
		bearer, err := f.tokens.Token(ctx, org)
		refused := false
		for _, row := range byOrg[org] {
			if ctx.Err() != nil {
				// Shutdown: the rows keep their lease and are claimed again when it ends.
				return result, ctx.Err()
			}
			switch {
			case err != nil:
				forwardedTotal.WithLabelValues("token_unavailable").Inc()
				f.fail(ctx, row, failTokenUnavailable)
				result.Failed++
			case refused:
				// cost-core refused this organization's bearer a moment ago; the rest of its
				// rows would be refused the same way. They wait for a fresh one.
				f.fail(ctx, row, failUnauthorized)
				result.Failed++
			default:
				class, status := f.post(ctx, bearer, row)
				if class == "" {
					forwardedTotal.WithLabelValues("forwarded").Inc()
					if err := f.store.MarkForwarded(ctx, row.ID); err != nil {
						// cost-core has it. The row stays unforwarded and is posted again after its
						// lease, which cost-core answers 202 without a second row.
						log.Error().Err(err).Int64("row", row.ID).Msg("usage row forwarded but not marked; it will be posted again")
						result.Failed++
						continue
					}
					result.Forwarded++
					continue
				}
				forwardedTotal.WithLabelValues(class).Inc()
				log.Warn().Int64("row", row.ID).Str("class", class).Int("status", status).Int("attempt", row.Attempts+1).
					Msg("usage row was not accepted by cost-core")
				f.fail(ctx, row, class)
				result.Failed++
				if class == failUnauthorized {
					f.tokens.Invalidate(org)
					refused = true
				}
			}
		}
	}
	f.report(ctx)
	return result, nil
}

// post sends one row. It returns the failure class ("" when cost-core accepted it) and the
// HTTP status for the log. Nothing from the response body reaches a log or the row: it can
// name the organization.
func (f *Forwarder) post(ctx context.Context, bearer string, row PendingRow) (string, int) {
	body, err := json.Marshal(BuildRecordRequest(row))
	if err != nil {
		return failRejected, 0
	}
	ctx, cancel := context.WithTimeout(ctx, f.config.Timeout)
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(f.config.BaseURL, "/")+recordPath, bytes.NewReader(body))
	if err != nil {
		return failRejected, 0
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+bearer)
	response, err := f.client.Do(request)
	if err != nil {
		return failUnavailable, 0
	}
	defer response.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxRecordResponse))
	return classify(response.StatusCode), response.StatusCode
}

// classify maps cost-core's answer to a failure class. Any 2xx is accepted (cost-core answers
// 202 for a new row and for a duplicate alike).
func classify(status int) string {
	switch {
	case status >= 200 && status < 300:
		return ""
	case status == http.StatusUnauthorized || status == http.StatusForbidden:
		return failUnauthorized
	case status == http.StatusRequestTimeout || status == http.StatusTooEarly || status == http.StatusTooManyRequests || status >= 500:
		return failUnavailable
	default:
		// 400, 404, 405, 413, 422: cost-core read the row and said no. Retrying the same body
		// changes nothing until cost-core or the row does, which is why this class backs off
		// the furthest.
		return failRejected
	}
}

// retryDelay is the pause before a row's next attempt: it doubles with each failure from
// thirty seconds, and a row cost-core rejected starts at five minutes and goes up to six
// hours (it needs a person or a deploy, not a retry); the others top out at an hour.
func retryDelay(class string, attempts int) time.Duration {
	base, ceiling := 30*time.Second, time.Hour
	if class == failRejected {
		base, ceiling = 5*time.Minute, 6*time.Hour
	}
	delay := base
	for i := 0; i < attempts && delay < ceiling; i++ {
		delay *= 2
	}
	return min(delay, ceiling)
}

func (f *Forwarder) fail(ctx context.Context, row PendingRow, class string) {
	next := f.now().Add(retryDelay(class, row.Attempts))
	if err := f.store.MarkFailed(ctx, row.ID, class, next); err != nil {
		log.Error().Err(err).Int64("row", row.ID).Msg("could not record a failed usage forward; the row is retried when its lease ends")
	}
}

// report publishes the backlog gauges.
func (f *Forwarder) report(ctx context.Context) {
	pending, oldest, err := f.store.Backlog(ctx)
	if err != nil {
		return
	}
	forwardPending.Set(float64(pending))
	forwardLagSeconds.Set(oldest.Seconds())
}

// Run forwards until the context ends. A pass that filled its batch is followed at once by
// another, so a backlog drains at the speed of cost-core and not of the interval.
func (f *Forwarder) Run(ctx context.Context) {
	log.Info().Dur("interval", f.config.Interval).Int("batch", f.config.BatchSize).Msg("usage forwarder started")
	timer := time.NewTimer(0)
	defer timer.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
		}
		wait := f.config.Interval
		result, err := f.RunOnce(ctx)
		switch {
		case err != nil && ctx.Err() != nil:
			return
		case err != nil:
			log.Error().Err(err).Msg("usage forwarder pass failed")
		case result.Claimed >= f.config.BatchSize:
			wait = 0
		}
		timer.Reset(wait)
	}
}

// ── Postgres ─────────────────────────────────────────────────────────────────

// PGForwardStore is the queue in cost_events.
//
// Phase 1 RLS: deliberately NOT wrapped in an organization scope, for the reason persist
// gives: this worker drains every organization's rows, so there is no single tenant to scope
// to. The organization is the row's own, written from the verified signature, and the bearer
// that posts it is minted for that organization.
type PGForwardStore struct {
	pool *pgxpool.Pool
}

func NewPGForwardStore(pool *pgxpool.Pool) *PGForwardStore { return &PGForwardStore{pool: pool} }

// claimUsageRowsSQL leases the next due rows. FOR UPDATE SKIP LOCKED lets replicas claim
// different rows at the same time; the lease keeps a crashed worker's rows from being stuck.
// The predicate is the one of idx_cost_events_forward_queue, so the claim reads the small
// partial index and never the ledger.
const claimUsageRowsSQL = `
WITH due AS (
    SELECT id
      FROM cost_events
     WHERE forwarded_at IS NULL
       AND schema_version >= 2
       AND (forward_next_at IS NULL OR forward_next_at <= now())
       AND (forward_lease_until IS NULL OR forward_lease_until < now())
     ORDER BY id
     LIMIT $1
       FOR UPDATE SKIP LOCKED
)
UPDATE cost_events AS e
   SET forward_lease_until = now() + make_interval(secs => $2::double precision)
  FROM due
 WHERE e.id = due.id
RETURNING e.id, e.org_id, e.event_type, e.model, e.feature, e.provider_used,
          e.input_tokens, e.output_tokens, e.cache_read_tokens, e.units,
          e.cost_basis, e.estimated, e.request_id, e.idempotency_key,
          e.forward_attempts, e.created_at`

const markForwardedSQL = `
UPDATE cost_events
   SET forwarded_at = now(), forward_lease_until = NULL, forward_error = NULL
 WHERE id = $1 AND forwarded_at IS NULL`

const markFailedSQL = `
UPDATE cost_events
   SET forward_attempts = forward_attempts + 1, forward_error = $2, forward_next_at = $3,
       forward_lease_until = NULL
 WHERE id = $1 AND forwarded_at IS NULL`

const backlogSQL = `
SELECT count(*), COALESCE(EXTRACT(EPOCH FROM now() - min(created_at)), 0)::double precision
  FROM cost_events
 WHERE forwarded_at IS NULL AND schema_version >= 2`

func (s *PGForwardStore) Claim(ctx context.Context, limit int, lease time.Duration) ([]PendingRow, error) {
	rows, err := s.pool.Query(ctx, claimUsageRowsSQL, limit, lease.Seconds())
	if err != nil {
		return nil, err
	}
	pending, err := pgx.CollectRows(rows, func(row pgx.CollectableRow) (PendingRow, error) {
		var (
			r        PendingRow
			feature  *string
			provider *string
			basis    *string
			estimate *bool
			request  *string
			key      *string
			input    *int64
			output   *int64
			cache    *int64
		)
		if err := row.Scan(&r.ID, &r.OrgID, &r.EventType, &r.Model, &feature, &provider,
			&input, &output, &cache, &r.Units, &basis, &estimate, &request, &key,
			&r.Attempts, &r.CreatedAt); err != nil {
			return PendingRow{}, err
		}
		// A version 2 row is complete (cost_events_v2_complete_chk), so these are never NULL;
		// reading them through pointers keeps a violated invariant from becoming a zero.
		if feature == nil || provider == nil || input == nil || output == nil || cache == nil || basis == nil ||
			estimate == nil || request == nil || key == nil {
			return PendingRow{}, errors.New("a usage row is incomplete: cost_events_v2_complete_chk is not in force")
		}
		r.Feature, r.ProviderUsed, r.CostBasis, r.RequestID, r.IdempotencyKey = *feature, *provider, *basis, *request, *key
		r.InputTokens, r.OutputTokens, r.CacheReadTokens, r.Estimated = *input, *output, *cache, *estimate
		return r, nil
	})
	if err != nil {
		return nil, err
	}
	sort.Slice(pending, func(i, j int) bool { return pending[i].ID < pending[j].ID })
	return pending, nil
}

func (s *PGForwardStore) MarkForwarded(ctx context.Context, id int64) error {
	_, err := s.pool.Exec(ctx, markForwardedSQL, id)
	return err
}

func (s *PGForwardStore) MarkFailed(ctx context.Context, id int64, class string, next time.Time) error {
	_, err := s.pool.Exec(ctx, markFailedSQL, id, class, next)
	return err
}

func (s *PGForwardStore) Backlog(ctx context.Context) (int64, time.Duration, error) {
	var (
		pending int64
		seconds float64
	)
	if err := s.pool.QueryRow(ctx, backlogSQL).Scan(&pending, &seconds); err != nil {
		return 0, 0, err
	}
	return pending, time.Duration(seconds * float64(time.Second)), nil
}
