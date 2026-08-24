// Change-webhook receiver — the Go control-plane counterpart to the Rust
// edge producer (crates/quarry-edge/src/change_webhook.rs).
//
// Contract (docs/CHANGE_TRACKING.md §"Webhook emission"):
//   - POST /v1/webhooks/change?org_id=<org>, signed with internal_auth's
//     X-Quarry-Sig triple over method/path-with-query/body-hash/ts/nonce.
//
// Signature verification (including stale-timestamp rejection beyond
// httpx.SkewToleranceSecs and nonce replay dedup) is NOT repeated here:
// cmd/control/main.go mounts this route inside the r.Group whose
// hmacVerifier.Middleware already performs exactly that verification and
// restores the body for downstream handlers. Re-verifying per-handler would
// double-consume every nonce and break legitimate retries.
//
// Persistence: each accepted record becomes one durable
// quarrycontracts.Event{Type: change_detected} via store.EventLog.Append,
// with the full wire document (record + subject/emitted_at envelope) as the
// payload so job-history consumers see the same shape the edge produced.
// The Idempotency-Key header makes edge retries idempotent: a pre-append
// lookup short-circuits known keys, and migration 013's partial unique index
// turns a concurrent double-delivery into a benign ErrConflict that resolves
// to the original event.
//
// ZDR: the producer refuses to emit zero-data-retention traffic fail-closed
// upstream; any body that still carries a truthy "zdr" flag is rejected here
// defensively rather than persisted.
package resources

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/notify"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// maxChangeWebhookBytes caps the request body. A real ChangeRecord is a few
// KB (two baselines + fingerprints); 1 MB leaves generous headroom while
// keeping a malicious oversized body from being buffered at all.
const maxChangeWebhookBytes = 1 << 20

// MountChangeWebhook registers POST /v1/webhooks/change. Call from inside
// the HMAC-signed route group in cmd/control/main.go. sink may be nil (dev).
func MountChangeWebhook(r chi.Router, db store.DB, sink notify.Sink) {
	r.Post("/v1/webhooks/change", changeWebhookHandler(db, sink))
}

func changeWebhookHandler(db store.DB, sink notify.Sink) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		// Org scoping: the edge stamps ?org_id=<verified-org> into the
		// signed path (sign_change_webhook signs
		// "/v1/webhooks/change?org_id=<org>"), so the query param is part
		// of the authenticated canonical string — safe to trust after the
		// group middleware's signature check.
		org := orgFromQuery(r)
		if org == "" {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
			return
		}

		r.Body = http.MaxBytesReader(w, r.Body, maxChangeWebhookBytes)
		raw, err := io.ReadAll(r.Body)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "read body: "+err.Error(), nil)
			return
		}

		// Defensive ZDR gate. The producer never emits for ZDR orgs; if a
		// body nonetheless asserts zero-data-retention, refuse to persist
		// anything rather than trusting the upstream guard alone.
		var probe map[string]any
		if err := json.Unmarshal(raw, &probe); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "decode body: "+err.Error(), nil)
			return
		}
		if bodyAssertsZDR(probe) {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest,
				"zero-data-retention payloads are never persisted", map[string]any{"zdr": true})
			return
		}

		var rec quarrycontracts.ChangeWebhook
		if err := json.Unmarshal(raw, &rec); err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "decode ChangeRecord: "+err.Error(), nil)
			return
		}
		switch {
		case rec.Subject != quarrycontracts.SubjectChangeDetected:
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "unexpected subject",
				map[string]any{"got": rec.Subject, "want": quarrycontracts.SubjectChangeDetected})
			return
		case rec.OrgID == "":
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "body.org_id required", nil)
			return
		case rec.OrgID != org:
			// The edge derives both from the same verified session value;
			// a mismatch means a buggy or hostile caller. Scope the error
			// message to the mismatch itself, never echo foreign org ids
			// beyond what the caller already sent.
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest,
				"body.org_id does not match org_id query param", nil)
			return
		case !rec.Status.Valid():
			httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "invalid status",
				map[string]any{"got": string(rec.Status)})
			return
		}

		// Idempotency — fast path. A retry whose key was already recorded
		// resolves to the original event without touching the writer.
		if key := r.Header.Get("Idempotency-Key"); key != "" {
			if existing, ok := db.Events().FindByIdempotencyKey(key); ok {
				writeChangeAck(w, r, http.StatusOK, existing.EventID, true)
				return
			}
		}

		payload, err := changeEventPayload(rec)
		if err != nil {
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}
		ts := time.Now().UTC()
		if rec.EmittedAt != nil {
			ts = *rec.EmittedAt
		}
		evt := quarrycontracts.Event{
			EventID:        quarrycontracts.NewID(quarrycontracts.KindEvent),
			Type:           quarrycontracts.EvtChangeDetected,
			Timestamp:      ts,
			Payload:        payload,
			IdempotencyKey: r.Header.Get("Idempotency-Key"),
		}
		if err := db.Events().Append(evt); err != nil {
			if errors.Is(err, store.ErrConflict) {
				// Lost a race against a concurrent delivery of the same
				// key (migration 013's partial unique index). Resolve to
				// the winner — that IS the successful outcome of an
				// idempotent retry.
				if key := evt.IdempotencyKey; key != "" {
					if existing, ok := db.Events().FindByIdempotencyKey(key); ok {
						writeChangeAck(w, r, http.StatusOK, existing.EventID, true)
						return
					}
				}
				httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "idempotency key conflict", nil)
				return
			}
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, err.Error(), nil)
			return
		}

		// Same side effects the batch event-ingest endpoint applies, so
		// change_detected events born at the webhook are indistinguishable
		// downstream: external webhook subscribers get delivery rows and
		// the schedule creator gets their in-product notification.
		fanoutWebhooks(db, evt)
		notifyOnChange(sink, evt)

		writeChangeAck(w, r, http.StatusOK, evt.EventID, false)
	}
}

// writeChangeAck emits the typed ack. WriteJSON's {request_id,data}
// envelope matches every other control endpoint; the producer ignores the
// body and acts on status alone, but subscribers/debuggers benefit from a
// structured receipt.
func writeChangeAck(w http.ResponseWriter, r *http.Request, status int, eventID quarrycontracts.ID, duplicate bool) {
	httpx.WriteJSON(w, r, status, map[string]any{
		"status":    map[bool]string{true: "duplicate", false: "recorded"}[duplicate],
		"event_id":  eventID,
		"duplicate": duplicate,
	})
}

// changeEventPayload serializes the full decoded wire document (record plus
// subject/emitted_at envelope) into the Event payload map, preserving the
// exact field names the Rust producer sent.
func changeEventPayload(rec quarrycontracts.ChangeWebhook) (map[string]any, error) {
	raw, err := json.Marshal(rec)
	if err != nil {
		return nil, err
	}
	var m map[string]any
	if err := json.Unmarshal(raw, &m); err != nil {
		return nil, err
	}
	return m, nil
}

// bodyAssertsZDR reports whether the decoded body carries a truthy "zdr"
// flag. Accepts JSON booleans and the string spellings producers actually
// use ("true"/"1"); anything else is treated as absent.
func bodyAssertsZDR(m map[string]any) bool {
	v, ok := m["zdr"]
	if !ok || v == nil {
		return false
	}
	switch t := v.(type) {
	case bool:
		return t
	case string:
		switch t {
		case "true", "1", "yes":
			return true
		default:
			return false
		}
	case float64:
		return t != 0
	default:
		return false
	}
}
