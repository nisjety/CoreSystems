package dispatcher

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/http"
	"os"
	"sync"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/egress"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/resources"
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

// jobWebhookGetenv is the environment a job-webhook delivery reads its switch and control key from
// at send time, so turning QUARRY_CONTROL__JOB_WEBHOOK off stops the deliveries already queued.
// A test swaps it.
var jobWebhookGetenv = os.Getenv

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

	targetURL, secret, ok := deliveryTarget(db, &d, updateDelivery)
	if !ok {
		return
	}

	body := []byte(d.Payload)
	ts := time.Now().Unix()
	sig := resources.SignWebhookBody(secret, ts, body)

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, targetURL, bytes.NewReader(body))
	if err != nil {
		d.Status = "dlq"
		d.LastError = "request build: " + err.Error()
		updateDelivery(d, "build_failed")
		return
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Quarry-Signature", sig)

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

// deliveryTarget resolves where a delivery is posted and the secret that signs it. A subscription
// delivery names a webhooks row, which must be active and belong to the delivery's org (INNH-5). A
// job-webhook delivery names its job instead (WebhookID "job:<job id>"), see jobDeliveryTarget. ok
// is false when the delivery is settled here, failed or dead-lettered, and must not be sent.
func deliveryTarget(db store.DB, d *store.WebhookDelivery, updateDelivery func(store.WebhookDelivery, string)) (string, string, bool) {
	if jobID, isJob := resources.JobWebhookDeliveryJobID(d.WebhookID); isJob {
		return jobDeliveryTarget(db, d, jobID, updateDelivery)
	}
	wh, found := db.Webhooks().Get(d.WebhookID)
	if !found || !wh.Active {
		d.Status = "failed"
		d.LastError = "webhook missing or inactive"
		updateDelivery(*d, "missing_or_inactive")
		return "", "", false
	}
	// Tenancy (INNH-5), defence in depth behind the fan-out: a subscription with no org
	// (legacy), or a delivery that belongs to another org than its subscription, is never
	// sent. This also kills deliveries queued before migration 025 stamped an org on them.
	if wh.OrgID == "" || wh.OrgID != d.OrgID {
		d.Status = "failed"
		d.LastError = "webhook org mismatch"
		updateDelivery(*d, "org_mismatch")
		return "", "", false
	}
	return wh.URL, wh.Secret, true
}

// jobDeliveryTarget resolves a job-webhook delivery against its job (W3-E3.5-3c). The URL and the
// events are the job's own params.webhook, read by an org-scoped lookup so a delivery never reaches
// another org's job. The signing secret is derived now from the control key and the (org, job)
// pair and is never stored. The switch is read here too, so turning QUARRY_CONTROL__JOB_WEBHOOK off
// stops the deliveries already queued. A switch or key that is not configured dead-letters the
// delivery as job_webhook_unconfigured. The delivery then goes through the same pinned client and
// retry policy as a subscription delivery.
func jobDeliveryTarget(db store.DB, d *store.WebhookDelivery, jobID string, updateDelivery func(store.WebhookDelivery, string)) (string, string, bool) {
	settle := func(status, lastError, op string) (string, string, bool) {
		d.Status = status
		d.LastError = lastError
		updateDelivery(*d, op)
		return "", "", false
	}
	if !resources.JobWebhooksEnabled(jobWebhookGetenv) {
		return settle("dlq", "job_webhook_unconfigured", "job_webhook_unconfigured")
	}
	if d.OrgID == "" {
		return settle("failed", "webhook org mismatch", "org_mismatch")
	}
	job, found := db.Jobs().GetByOrg(d.OrgID, quarrycontracts.ID(jobID))
	if !found || job.OrgID != d.OrgID {
		return settle("failed", "job missing", "job_missing")
	}
	hook, found := resources.JobWebhookTarget(job)
	if !found || !hook.Wants(d.EventType) {
		return settle("failed", "job webhook has no target for this event", "job_webhook_missing")
	}
	key, err := resources.JobWebhookKey(jobWebhookGetenv)
	if err != nil {
		return settle("dlq", "job_webhook_unconfigured", "job_webhook_unconfigured")
	}
	secret, err := resources.JobWebhookSecret(key, d.OrgID, jobID)
	if err != nil {
		return settle("dlq", "job_webhook_unconfigured", "job_webhook_unconfigured")
	}
	return hook.URL, secret, true
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
