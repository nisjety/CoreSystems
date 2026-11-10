package resources

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"sort"
	"strings"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// MountWebhooksGone answers the retired unscoped webhook CRUD with 410 Gone (INNH-5).
//
// GET/POST /v1/webhooks and GET /v1/webhooks/{id} listed and created webhooks with no org
// and returned each secret in cleartext. Nothing in the repo calls them; the tenant surface
// is /v1/webhook-subscriptions. The 410 covers every method and every path under
// /v1/webhooks EXCEPT the edge's signed change receiver, POST /v1/webhooks/change, which
// chi's static-segment precedence keeps on its own handler. The legacy
// /v1/webhook-deliveries list/get/retry is retired the same way: it listed every org's
// delivery payloads. Not flag-gated: this closes a hole that has no legitimate user.
func MountWebhooksGone(r chi.Router) {
	gone := http.HandlerFunc(writeWebhooksGone)
	r.Handle("/v1/webhooks", gone)
	r.Handle("/v1/webhooks/*", gone)
	r.Handle("/v1/webhook-deliveries", gone)
	r.Handle("/v1/webhook-deliveries/*", gone)
}

func writeWebhooksGone(w http.ResponseWriter, r *http.Request) {
	env := quarrycontracts.Err(httpx.RequestIDOf(r.Context()), quarrycontracts.ErrorEnvelope{
		Code:    blocklistsGoneCode,
		Message: "the unscoped webhook routes are retired; use the org-scoped /v1/webhook-subscriptions",
		Details: map[string]any{
			"successor": "/v1/webhook-subscriptions",
			"reference": "docs/CHANGE_TRACKING.md",
		},
		Retryable: false,
	})
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusGone)
	_ = json.NewEncoder(w).Encode(env)
}

// Per-job webhooks (W3-E3.5-3).
//
// A crawl or batch request may carry `webhook: {url, events}` (or a bare URL string). The edge
// accepts it only while QUARRY_EDGE__JOB_WEBHOOK is on, checks the syntax and literal
// addresses (crawl_request.rs, check_job_webhook), and forwards it in the job params under
// JobWebhookParamKey. Control vets it again here exactly as a subscription target
// (validateWebhookURL: https, no credentials or fragment, every resolved address through the
// egress guard), normalises the events against the closed JobWebhookEvents set, and stores the
// result on the job. A ZDR job may not carry one: the delivery row keeps its payload.
//
// Deliveries are signed like subscription deliveries (X-Quarry-Signature, t=<unix>,v1=<hex
// HMAC-SHA256 over "<t>.<body>">) with a per-job secret derived from the control key
// QUARRY_CONTROL__JOB_WEBHOOK_KEY and the (org, job) pair. The secret is never stored: it is
// derived again at delivery time and shown once, in the job-create response.
//
// QUARRY_CONTROL__JOB_WEBHOOK (default off) is control's half of the switch: off, a stored
// webhook param is ignored and nothing is delivered.

const (
	// JobWebhookParamKey is the job params key a per-job webhook is stored under.
	JobWebhookParamKey = "webhook"
	// jobWebhookSecretPrefix marks a derived per-job secret, as whsec_ marks a subscription's.
	jobWebhookSecretPrefix = "whjob_"
	// jobWebhookKeyMinBytes is the shortest control key accepted (256 bits).
	jobWebhookKeyMinBytes = 32
	// jobWebhookSecretContext separates this derivation from any other use of the key.
	jobWebhookSecretContext = "quarry-job-webhook/v1"
)

// JobWebhookEvents is the closed vocabulary a per-job webhook may name in `events`: the stored
// event types of the job's own run. The edge holds the same set (JOB_WEBHOOK_EVENTS).
var JobWebhookEvents = map[string]bool{
	string(quarrycontracts.EvtRunStarted):   true,
	string(quarrycontracts.EvtRunCompleted): true,
	string(quarrycontracts.EvtRunFailed):    true,
	string(quarrycontracts.EvtRunCancelled): true,
	string(quarrycontracts.EvtRunTimedOut):  true,
	string(quarrycontracts.EvtPageFetched):  true,
	string(quarrycontracts.EvtPageFailed):   true,
}

// DefaultJobWebhookEvents is what a hook gets when `events` is absent: the terminal events, sorted.
var DefaultJobWebhookEvents = []string{
	string(quarrycontracts.EvtRunCancelled),
	string(quarrycontracts.EvtRunCompleted),
	string(quarrycontracts.EvtRunFailed),
	string(quarrycontracts.EvtRunTimedOut),
}

// JobWebhook is a per-job delivery target as it is stored in the job params.
type JobWebhook struct {
	URL    string   `json:"url"`
	Events []string `json:"events"`
}

// Wants reports whether the hook asked for eventType.
func (h JobWebhook) Wants(eventType string) bool {
	for _, e := range h.Events {
		if e == eventType {
			return true
		}
	}
	return false
}

// ParamsValue is the value stored under JobWebhookParamKey.
func (h JobWebhook) ParamsValue() map[string]any {
	events := make([]any, 0, len(h.Events))
	for _, e := range h.Events {
		events = append(events, e)
	}
	return map[string]any{"url": h.URL, "events": events}
}

// JobWebhooksEnabled reports whether QUARRY_CONTROL__JOB_WEBHOOK is set to a truthy value
// (true, 1, yes, on). Default false: no per-job webhook is accepted or delivered.
func JobWebhooksEnabled(getenv func(string) string) bool {
	switch strings.ToLower(strings.TrimSpace(getenv("QUARRY_CONTROL__JOB_WEBHOOK"))) {
	case "true", "1", "yes", "on":
		return true
	}
	return false
}

// normalizeJobWebhookEvents validates and canonicalises a per-job events list. A nil list (the
// key was absent) is the default set; an empty one is refused.
func normalizeJobWebhookEvents(in []string, present bool) (events []string, reason, msg string) {
	if !present {
		return append([]string(nil), DefaultJobWebhookEvents...), "", ""
	}
	if len(in) == 0 {
		return nil, "events_empty", "webhook.events must name at least one event type"
	}
	seen := map[string]bool{}
	for _, e := range in {
		e = strings.TrimSpace(e)
		if !JobWebhookEvents[e] {
			return nil, "events_unsupported", "unsupported webhook event type: " + e
		}
		if !seen[e] {
			seen[e] = true
			events = append(events, e)
		}
	}
	sort.Strings(events)
	return events, "", ""
}

// NormalizeJobWebhook checks the raw `webhook` value of a job request (decoded JSON: a URL
// string or an object with url and events) and returns the hook to store. field names the
// offending field and reason is a stable token; an empty reason means the hook is acceptable.
// A ZDR job is refused (zdr_refused). The URL goes through validateWebhookURL, so the reasons
// and the SSRF vet are the subscription ones.
func NormalizeJobWebhook(ctx context.Context, raw any, zdr bool, vet HostVetter) (hook JobWebhook, field, reason, msg string) {
	const base = JobWebhookParamKey
	if zdr {
		return JobWebhook{}, base, "zdr_refused", "a zero-data-retention job cannot carry a webhook"
	}
	var (
		rawURL        any
		rawEvents     any
		eventsPresent bool
		urlField      = base + ".url"
	)
	switch v := raw.(type) {
	case string:
		rawURL, urlField = v, base
	case map[string]any:
		keys := make([]string, 0, len(v))
		for k := range v {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		for _, k := range keys {
			switch k {
			case "url":
			case "events":
				if v[k] != nil {
					eventsPresent = true
				}
			case "headers":
				return JobWebhook{}, base + "." + k, "reserved_field", "a per-job webhook does not take headers"
			default:
				return JobWebhook{}, base + "." + k, "unknown_field", "unknown webhook field: " + k
			}
		}
		rawURL, rawEvents = v["url"], v["events"]
	default:
		return JobWebhook{}, base, "invalid_type", "webhook must be a URL string or an object with url and events"
	}
	u, ok := rawURL.(string)
	if !ok {
		if rawURL == nil {
			return JobWebhook{}, urlField, "url_invalid", "webhook url is required"
		}
		return JobWebhook{}, urlField, "invalid_type", "webhook url must be a string"
	}
	if reason, msg := validateWebhookURL(ctx, u, vet); reason != "" {
		return JobWebhook{}, urlField, reason, "webhook " + msg
	}
	var list []string
	if eventsPresent {
		items, ok := rawEvents.([]any)
		if !ok {
			return JobWebhook{}, base + ".events", "invalid_type", "webhook.events must be an array of event names"
		}
		for _, item := range items {
			s, ok := item.(string)
			if !ok {
				return JobWebhook{}, base + ".events", "invalid_type", "webhook.events entries must be strings"
			}
			list = append(list, s)
		}
	}
	events, reason, msg := normalizeJobWebhookEvents(list, eventsPresent)
	if reason != "" {
		return JobWebhook{}, base + ".events", reason, msg
	}
	return JobWebhook{URL: strings.TrimSpace(u), Events: events}, "", "", ""
}

// JobWebhookFromParams reads a stored, already normalised hook from job params. It does not
// vet again: the dispatcher's pinned dial does that per delivery. ok is false when the job has
// no hook or the stored value is malformed.
func JobWebhookFromParams(params map[string]any) (JobWebhook, bool) {
	raw, ok := params[JobWebhookParamKey].(map[string]any)
	if !ok {
		return JobWebhook{}, false
	}
	u, ok := raw["url"].(string)
	if !ok || u == "" {
		return JobWebhook{}, false
	}
	hook := JobWebhook{URL: u}
	switch events := raw["events"].(type) {
	case []any:
		for _, e := range events {
			if s, ok := e.(string); ok && JobWebhookEvents[s] {
				hook.Events = append(hook.Events, s)
			}
		}
	case []string:
		for _, s := range events {
			if JobWebhookEvents[s] {
				hook.Events = append(hook.Events, s)
			}
		}
	}
	if len(hook.Events) == 0 {
		return JobWebhook{}, false
	}
	return hook, true
}

// JobWebhookTarget is the delivery target of a job's own webhook, as its params store it. ok is
// false when the job has none, when the stored value is malformed, or when the job is
// zero-data-retention: a ZDR job delivers nothing, so none of its events is stored or sent
// (W3-E3.5-3c).
func JobWebhookTarget(job store.Job) (JobWebhook, bool) {
	if jobParamsZdr(job.Params) {
		return JobWebhook{}, false
	}
	return JobWebhookFromParams(job.Params)
}

// JobWebhookDeliveryPrefix marks a webhook delivery whose target is a job's own webhook. Its
// WebhookID is this prefix followed by the job id. No webhooks row has such an id, so the
// dispatcher resolves the target against the job instead (W3-E3.5-3c).
const JobWebhookDeliveryPrefix = "job:"

// JobWebhookDeliveryWebhookID is the WebhookID a job's own webhook deliveries are stored under.
func JobWebhookDeliveryWebhookID(jobID string) quarrycontracts.ID {
	return quarrycontracts.ID(JobWebhookDeliveryPrefix + jobID)
}

// JobWebhookDeliveryJobID returns the job a delivery's WebhookID names. ok is false for a
// subscription delivery, whose WebhookID names a webhooks row.
func JobWebhookDeliveryJobID(webhookID quarrycontracts.ID) (string, bool) {
	id := string(webhookID)
	if !strings.HasPrefix(id, JobWebhookDeliveryPrefix) || len(id) == len(JobWebhookDeliveryPrefix) {
		return "", false
	}
	return id[len(JobWebhookDeliveryPrefix):], true
}

// ErrJobWebhookKey is returned when the control key is missing or too short.
var ErrJobWebhookKey = errors.New("QUARRY_CONTROL__JOB_WEBHOOK_KEY must be at least 32 bytes of hex")

// JobWebhookKey reads QUARRY_CONTROL__JOB_WEBHOOK_KEY (hex, at least 32 bytes). The value is
// never logged or echoed.
func JobWebhookKey(getenv func(string) string) ([]byte, error) {
	raw := strings.TrimSpace(getenv("QUARRY_CONTROL__JOB_WEBHOOK_KEY"))
	key, err := hex.DecodeString(raw)
	if err != nil || len(key) < jobWebhookKeyMinBytes {
		return nil, ErrJobWebhookKey
	}
	return key, nil
}

// JobWebhookSecret derives the signing secret of one job's webhook: whjob_<hex HMAC-SHA256 of
// the context, org and job under the control key>. Deterministic, so nothing is stored; bound
// to the org and the job, so one job's secret verifies no other job's deliveries.
func JobWebhookSecret(key []byte, orgID, jobID string) (string, error) {
	if len(key) < jobWebhookKeyMinBytes {
		return "", ErrJobWebhookKey
	}
	if orgID == "" || jobID == "" {
		return "", errors.New("job webhook secret needs an org and a job")
	}
	mac := hmac.New(sha256.New, key)
	mac.Write([]byte(jobWebhookSecretContext))
	mac.Write([]byte{0})
	mac.Write([]byte(orgID))
	mac.Write([]byte{0})
	mac.Write([]byte(jobID))
	return jobWebhookSecretPrefix + hex.EncodeToString(mac.Sum(nil)), nil
}

// SignWebhookBody returns the X-Quarry-Signature value for body at unix time ts, in the format
// the subscription dispatcher sends: t=<ts>,v1=<hex HMAC-SHA256 of "<ts>.<body>">.
func SignWebhookBody(secret string, ts int64, body []byte) string {
	mac := hmac.New(sha256.New, []byte(secret))
	fmt.Fprintf(mac, "%d.%s", ts, body)
	return fmt.Sprintf("t=%d,v1=%s", ts, hex.EncodeToString(mac.Sum(nil)))
}
