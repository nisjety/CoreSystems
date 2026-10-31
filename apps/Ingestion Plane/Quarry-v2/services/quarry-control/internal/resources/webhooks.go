package resources

import (
	"encoding/json"
	"net/http"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
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
