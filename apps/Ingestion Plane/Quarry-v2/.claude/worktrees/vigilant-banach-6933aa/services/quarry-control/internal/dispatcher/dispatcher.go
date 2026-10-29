package dispatcher

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"time"

	"github.com/rs/zerolog"

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
		hc = &http.Client{Timeout: 30 * time.Second}
	}
	jobs := make(chan store.WebhookDelivery, opts.Workers*4)
	for i := 0; i < opts.Workers; i++ {
		go worker(ctx, db, hc, jobs, logger)
	}
	tick := time.NewTicker(1 * time.Second)
	defer tick.Stop()
	for {
		select {
		case <-ctx.Done():
			close(jobs)
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
					close(jobs)
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
	wh, ok := db.Webhooks().Get(d.WebhookID)
	if !ok || !wh.Active {
		d.Status = "failed"
		d.LastError = "webhook missing or inactive"
		_ = db.WebhookDeliveries().Update(d)
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
		_ = db.WebhookDeliveries().Update(d)
		return
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Quarry-Signature", fmt.Sprintf("t=%d,v1=%s", ts, sig))

	resp, err := hc.Do(req)
	if err != nil {
		retryOrDLQ(db, &d, "transport: "+err.Error(), true)
		return
	}
	defer resp.Body.Close()

	switch {
	case resp.StatusCode >= 200 && resp.StatusCode < 300:
		d.Status = "success"
		d.LastError = ""
		_ = db.WebhookDeliveries().Update(d)
	case resp.StatusCode >= 500 || resp.StatusCode == 408 || resp.StatusCode == 429:
		retryOrDLQ(db, &d, fmt.Sprintf("http %d", resp.StatusCode), true)
	default:
		d.Status = "dlq"
		d.LastError = fmt.Sprintf("http %d", resp.StatusCode)
		_ = db.WebhookDeliveries().Update(d)
	}
}

func retryOrDLQ(db store.DB, d *store.WebhookDelivery, reason string, retryable bool) {
	d.Attempt++
	d.LastError = reason
	if !retryable || d.Attempt >= maxAttempts {
		d.Status = "dlq"
		_ = db.WebhookDeliveries().Update(*d)
		return
	}
	idx := d.Attempt
	if idx >= len(backoff) {
		idx = len(backoff) - 1
	}
	d.Status = "pending"
	d.NextAttemptAt = time.Now().Add(backoff[idx]).Unix()
	_ = db.WebhookDeliveries().Update(*d)
}
