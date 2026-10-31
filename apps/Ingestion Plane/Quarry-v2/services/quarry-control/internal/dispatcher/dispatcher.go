package dispatcher

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"net/http"
	"sync"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/egress"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// Options configures the dispatcher loop.
type Options struct {
	Workers int
}

// Backoff schedule per attempt index.
var backoff = []time.Duration{
	1 * time.Second,
	5 * time.Second,
	30 * time.Second,
	5 * time.Minute,
	1 * time.Hour,
}

const maxAttempts = 5

// Run starts the dispatcher loop until ctx is cancelled.
func Run(ctx context.Context, db store.DB, hc *http.Client, opts Options, logger zerolog.Logger) {
	if opts.Workers <= 0 {
		opts.Workers = 4
	}
	if hc == nil {
		hc = egress.SafeClient(egress.ClientConfig{RequestTimeout: 30 * time.Second}) // E3.5-2: webhook targets are tenant-registered; SSRF guard
	}
	jobs := make(chan store.WebhookDelivery, opts.Workers*4)
	// Both the outer select and the inner send-loop need to close the
	// jobs channel on ctx cancellation. Closing a closed channel
	// panics; sync.Once guarantees exactly one close regardless of
	// which path wins the race.
	var closeOnce sync.Once
	closeJobs := func() { closeOnce.Do(func() { close(jobs) }) }
	defer closeJobs()

	for i := 0; i < opts.Workers; i++ {
		go worker(ctx, db, hc, jobs, logger)
	}
	tick := time.NewTicker(1 * time.Second)
	defer tick.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-tick.C:
			due, err := db.WebhookDeliveries().ClaimDue(time.Now().Unix(), opts.Workers*4)
			if err != nil {
				logger.Error().Err(err).Msg("dispatcher claim_due failed")
				continue
			}
			for _, d := range due {
				select {
				case jobs <- d:
				case <-ctx.Done():
					return
				}
			}
		}
	}
}

func worker(ctx context.Context, db store.DB, hc *http.Client, jobs <-chan store.WebhookDelivery, logger zerolog.Logger) {
	for d := range jobs {
		dispatch(ctx, db, hc, d, logger)
	}
}

func dispatch(ctx context.Context, db store.DB, hc *http.Client, d store.WebhookDelivery, logger zerolog.Logger) {
	// Surfacing transient store.Update failures: previously every
	// `_ = db.WebhookDeliveries().Update(...)` swallowed errors, so a
	// row stuck in_flight because of a single DB hiccup looked
	// indistinguishable from a working delivery. Log + record metric
	// in one place.
	updateDelivery := func(rec store.WebhookDelivery, op string) {
		if err := db.WebhookDeliveries().Update(rec); err != nil {
			logger.Error().
				Err(err).
				Str("delivery_id", string(rec.ID)).
				Str("status", rec.Status).
				Str("op", op).
				Msg("webhook delivery store update failed")
		}
	}

	wh, ok := db.Webhooks().Get(d.WebhookID)
	if !ok || !wh.Active {
		d.Status = "failed"
		d.LastError = "webhook missing or inactive"
		updateDelivery(d, "missing_or_inactive")
		return
	}
	// Tenancy (INNH-5), defence in depth behind the fan-out: a subscription with no org
	// (legacy), or a delivery that belongs to another org than its subscription, is never
	// sent. This also kills deliveries queued before migration 025 stamped an org on them.
	if wh.OrgID == "" || wh.OrgID != d.OrgID {
		d.Status = "failed"
		d.LastError = "webhook org mismatch"
		updateDelivery(d, "org_mismatch")
		return
	}

	body := []byte(d.Payload)
	ts := time.Now().Unix()
	mac := hmac.New(sha256.New, []byte(wh.Secret))
	fmt.Fprintf(mac, "%d.%s", ts, body)
	sig := hex.EncodeToString(mac.Sum(nil))

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, wh.URL, bytes.NewReader(body))
	if err != nil {
		d.Status = "dlq"
		d.LastError = "request build: " + err.Error()
		updateDelivery(d, "build_failed")
		return
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Quarry-Signature", fmt.Sprintf("t=%d,v1=%s", ts, sig))

	resp, err := hc.Do(req)
	if err != nil {
		// A target the egress guard refuses (private, loopback, link-local, metadata) will
		// not become reachable by waiting: dead-letter it at once instead of five retries.
		retryOrDLQ(db, &d, "transport: "+err.Error(), !errors.Is(err, egress.ErrBlocked), logger)
		return
	}
	defer resp.Body.Close()

	switch {
	case resp.StatusCode >= 200 && resp.StatusCode < 300:
		d.Status = "success"
		d.LastError = ""
		updateDelivery(d, "success")
	case resp.StatusCode >= 500 || resp.StatusCode == 408 || resp.StatusCode == 429:
		retryOrDLQ(db, &d, fmt.Sprintf("http %d", resp.StatusCode), true, logger)
	default:
		d.Status = "dlq"
		d.LastError = fmt.Sprintf("http %d", resp.StatusCode)
		updateDelivery(d, "dlq_4xx")
	}
}

func retryOrDLQ(db store.DB, d *store.WebhookDelivery, reason string, retryable bool, logger zerolog.Logger) {
	d.Attempt++
	d.LastError = reason
	if !retryable || d.Attempt >= maxAttempts {
		d.Status = "dlq"
		if err := db.WebhookDeliveries().Update(*d); err != nil {
			logger.Error().Err(err).Str("delivery_id", string(d.ID)).Msg("dlq update failed")
		}
		return
	}
	idx := d.Attempt
	if idx >= len(backoff) {
		idx = len(backoff) - 1
	}
	d.Status = "pending"
	d.NextAttemptAt = time.Now().Add(backoff[idx]).Unix()
	if err := db.WebhookDeliveries().Update(*d); err != nil {
		logger.Error().Err(err).Str("delivery_id", string(d.ID)).Msg("retry update failed")
	}
}
