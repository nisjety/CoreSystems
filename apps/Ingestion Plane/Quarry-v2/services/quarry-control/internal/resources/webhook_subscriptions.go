package resources

// Org-scoped webhook subscriptions (INNH-5): the tenant-facing layer over the webhook store,
// the delivery fan-out and the dispatcher that already existed.
//
//	POST   /v1/webhook-subscriptions                                  create (secret shown once)
//	GET    /v1/webhook-subscriptions                                  list
//	GET    /v1/webhook-subscriptions/{id}                             read
//	PATCH  /v1/webhook-subscriptions/{id}                             active / events / description
//	DELETE /v1/webhook-subscriptions/{id}                             delete (deliveries cascade)
//	POST   /v1/webhook-subscriptions/{id}/rotate-secret               new secret, shown once
//	GET    /v1/webhook-subscriptions/{id}/deliveries                  delivery log, no payload body
//	POST   /v1/webhook-subscriptions/{id}/deliveries/{did}/retry      re-queue a failed or dlq delivery
//
// Every route takes the org from the edge-verified `org_id` query (signed into the HMAC
// canonical string) and answers 404 for a row of another org exactly as for a missing one.
// The secret is minted here, returned once, and never serialized again: the view type has no
// secret field and store.Webhook.Secret is json:"-". Mounted only when
// QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS is on (cmd/control/main.go); otherwise the path is 404.
//
// The URL is vetted twice: here at create time (https only, no userinfo or fragment, every
// resolved address through egress.Guard.ResolveAndVet) for a clear 400, and again at every
// delivery by the dispatcher's pinned-dial SafeClient, which is the real defence.

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/egress"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const (
	// WebhookSubscriptionsMaxPerOrg caps the subscriptions one org can hold, active or not.
	WebhookSubscriptionsMaxPerOrg = 10
	maxWebhookURLLen              = 2048
	maxWebhookDescriptionLen      = 200
	webhookSecretPrefix           = "whsec_"
	subscriptionsPageDefault      = 50
	subscriptionsPageMax          = 200
	webhookVetTimeout             = 5 * time.Second
)

// SubscribableEvents is the closed vocabulary a subscription may name in `events`. v1 is the
// stored event type of a detected change. Run-finished events (INNH-13) are added here; the
// fan-out already matches any stored type, so adding one is not a contract change.
var SubscribableEvents = map[string]bool{
	string(quarrycontracts.EvtChangeDetected): true,
}

// HostVetter is the create-time SSRF check: it resolves host and refuses a blocked address.
// *egress.Guard.ResolveAndVet satisfies it.
type HostVetter func(ctx context.Context, host string) error

// DefaultHostVetter vets with the process-wide egress guard, the one the dispatcher uses.
func DefaultHostVetter(ctx context.Context, host string) error {
	_, err := egress.Default().ResolveAndVet(ctx, host)
	return err
}

// WebhookSubscriptionOptions configures MountWebhookSubscriptions. The zero value is the
// production configuration.
type WebhookSubscriptionOptions struct {
	Vet       HostVetter
	MaxPerOrg int
}

// webhookSubscriptionView is a subscription as a caller sees it. It has no secret field.
type webhookSubscriptionView struct {
	ID           string               `json:"id"`
	URL          string               `json:"url"`
	Events       []string             `json:"events"`
	Active       bool                 `json:"active"`
	Description  string               `json:"description,omitempty"`
	SecretHint   string               `json:"secret_hint"`
	CreatedAt    string               `json:"created_at"`
	LastDelivery *webhookLastDelivery `json:"last_delivery,omitempty"`
}

type webhookLastDelivery struct {
	Status  string `json:"status"`
	At      string `json:"at"`
	EventID string `json:"event_id"`
}

// webhookSubscriptionCreated is the one response that carries the secret.
type webhookSubscriptionCreated struct {
	webhookSubscriptionView
	Secret string `json:"secret"`
}

type webhookDeliveryView struct {
	ID            string `json:"id"`
	EventID       string `json:"event_id"`
	EventType     string `json:"event_type"`
	Status        string `json:"status"`
	Attempt       int    `json:"attempt"`
	NextAttemptAt string `json:"next_attempt_at,omitempty"`
	LastError     string `json:"last_error,omitempty"`
	CreatedAt     string `json:"created_at"`
}

func unixRFC3339(sec int64) string { return time.Unix(sec, 0).UTC().Format(time.RFC3339) }

func toSubscriptionView(w store.Webhook) webhookSubscriptionView {
	events := w.Events
	if events == nil {
		events = []string{}
	}
	return webhookSubscriptionView{
		ID: string(w.ID), URL: w.URL, Events: events, Active: w.Active,
		Description: w.Description, SecretHint: w.SecretHint, CreatedAt: unixRFC3339(w.CreatedAt),
	}
}

// DeliveryErrorToken maps the dispatcher's free-text last_error to the closed vocabulary the
// API shows: http_<code>, timeout, blocked_target, transport, inactive, or error. The raw
// text can carry resolver or dial detail about the customer's network and is never exposed.
func DeliveryErrorToken(raw string) string {
	switch {
	case raw == "":
		return ""
	case strings.HasPrefix(raw, "http "):
		code := strings.TrimSpace(strings.TrimPrefix(raw, "http "))
		if _, err := strconv.Atoi(code); err == nil {
			return "http_" + code
		}
		return "error"
	case strings.Contains(raw, "destination blocked by policy"):
		return "blocked_target"
	case strings.Contains(raw, "deadline exceeded") || strings.Contains(raw, "Timeout") || strings.Contains(raw, "timeout"):
		return "timeout"
	case strings.HasPrefix(raw, "transport:"):
		return "transport"
	case strings.HasPrefix(raw, "webhook missing or inactive"), strings.HasPrefix(raw, "webhook org mismatch"):
		return "inactive"
	default:
		return "error"
	}
}

func toDeliveryView(d store.WebhookDelivery) webhookDeliveryView {
	v := webhookDeliveryView{
		ID: string(d.ID), EventID: string(d.EventID), EventType: d.EventType, Status: d.Status,
		Attempt: d.Attempt, LastError: DeliveryErrorToken(d.LastError), CreatedAt: unixRFC3339(d.CreatedAt),
	}
	if d.Status == "pending" {
		v.NextAttemptAt = unixRFC3339(d.NextAttemptAt)
	}
	return v
}

// newWebhookSecret mints 32 random bytes as `whsec_<64 hex>` and its display hint.
func newWebhookSecret() (secret, hint string, err error) {
	b := make([]byte, 32)
	if _, err = rand.Read(b); err != nil {
		return "", "", err
	}
	secret = webhookSecretPrefix + hex.EncodeToString(b)
	return secret, webhookSecretPrefix + "..." + secret[len(secret)-4:], nil
}

// writeSubscriptionBadRequest answers a 400 naming the offending field and a stable reason.
func writeSubscriptionBadRequest(w http.ResponseWriter, r *http.Request, field, reason, msg string) {
	httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, msg, map[string]any{"field": field, "reason": reason})
}

// validateWebhookURL checks a subscription target. It returns a stable reason token and a
// message; an empty reason means the URL is acceptable. The check is defence in depth and a
// clear error for the caller: the dispatcher's pinned dial is what actually enforces the policy.
func validateWebhookURL(ctx context.Context, raw string, vet HostVetter) (reason, msg string) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "url_invalid", "url is required"
	}
	if len(raw) > maxWebhookURLLen {
		return "url_too_long", "url is longer than 2048 characters"
	}
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" {
		return "url_invalid", "url is not a valid absolute URL"
	}
	if !strings.EqualFold(u.Scheme, "https") {
		return "url_not_https", "url must use https"
	}
	if u.User != nil {
		return "url_userinfo", "url must not carry credentials"
	}
	if u.Fragment != "" || strings.Contains(raw, "#") {
		return "url_fragment", "url must not carry a fragment"
	}
	host := u.Hostname()
	if host == "" {
		return "url_invalid", "url has no host"
	}
	if p := u.Port(); p != "" {
		if n, err := strconv.Atoi(p); err != nil || n < 1 || n > 65535 {
			return "url_invalid", "url port is out of range"
		}
	}
	if vet == nil {
		vet = DefaultHostVetter
	}
	vctx, cancel := context.WithTimeout(ctx, webhookVetTimeout)
	defer cancel()
	if err := vet(vctx, host); err != nil {
		if errors.Is(err, egress.ErrBlocked) {
			return "url_private_address", "url resolves to a private, loopback or link-local address"
		}
		return "url_unresolvable", "url host could not be resolved"
	}
	return "", ""
}

// normalizeSubscriptionEvents validates and canonicalises an events list: non-empty, every
// entry in SubscribableEvents, duplicates dropped, sorted so equal sets compare equal.
func normalizeSubscriptionEvents(in []string) (events []string, reason, msg string) {
	if len(in) == 0 {
		return nil, "events_empty", "events must name at least one event type"
	}
	seen := map[string]bool{}
	for _, e := range in {
		e = strings.TrimSpace(e)
		if !SubscribableEvents[e] {
			return nil, "events_unsupported", "unsupported event type: " + e
		}
		if !seen[e] {
			seen[e] = true
			events = append(events, e)
		}
	}
	sort.Strings(events)
	return events, "", ""
}

// MountWebhookSubscriptions registers the org-scoped subscription routes.
func MountWebhookSubscriptions(r chi.Router, db store.DB, opts WebhookSubscriptionOptions) {
	if opts.MaxPerOrg <= 0 {
		opts.MaxPerOrg = WebhookSubscriptionsMaxPerOrg
	}
	h := &subscriptionHandlers{db: db, opts: opts}
	r.Route("/v1/webhook-subscriptions", func(r chi.Router) {
		r.Post("/", h.create)
		r.Get("/", h.list)
		r.Get("/{id}", h.get)
		r.Patch("/{id}", h.patch)
		r.Delete("/{id}", h.delete)
		r.Post("/{id}/rotate-secret", h.rotateSecret)
		r.Get("/{id}/deliveries", h.deliveries)
		r.Post("/{id}/deliveries/{did}/retry", h.retryDelivery)
	})
}

type subscriptionHandlers struct {
	db   store.DB
	opts WebhookSubscriptionOptions
}

// orgOr400 reads the verified org; an absent one is a 400 and the handler stops.
func orgOr400(w http.ResponseWriter, r *http.Request) (string, bool) {
	org := orgFromQuery(r)
	if org == "" {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "org_id required", nil)
		return "", false
	}
	return org, true
}

func writeSubscriptionNotFound(w http.ResponseWriter, r *http.Request, id string) {
	httpx.WriteErr(w, r, quarrycontracts.CodeNotFound, "not found", map[string]any{"id": id})
}

// load reads the caller's subscription or answers 404.
func (h *subscriptionHandlers) load(w http.ResponseWriter, r *http.Request, org string) (store.Webhook, bool) {
	id := chi.URLParam(r, "id")
	wh, ok := h.db.Webhooks().GetByOrg(quarrycontracts.ID(id), org)
	if !ok {
		writeSubscriptionNotFound(w, r, id)
		return store.Webhook{}, false
	}
	return wh, true
}

func (h *subscriptionHandlers) viewWithLast(org string, wh store.Webhook) webhookSubscriptionView {
	v := toSubscriptionView(wh)
	if last, _ := h.db.WebhookDeliveries().ListByWebhook(org, wh.ID, "", 1, ""); len(last) > 0 {
		v.LastDelivery = &webhookLastDelivery{
			Status: last[0].Status, At: unixRFC3339(last[0].CreatedAt), EventID: string(last[0].EventID),
		}
	}
	return v
}

type createSubscriptionBody struct {
	URL         string   `json:"url"`
	Events      []string `json:"events"`
	Description string   `json:"description"`
	CreatedBy   string   `json:"created_by"`
}

func (h *subscriptionHandlers) create(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, 64<<10)
	var body createSubscriptionBody
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "decode body: "+err.Error(), nil)
		return
	}
	if reason, msg := validateWebhookURL(r.Context(), body.URL, h.opts.Vet); reason != "" {
		writeSubscriptionBadRequest(w, r, "url", reason, msg)
		return
	}
	events, reason, msg := normalizeSubscriptionEvents(body.Events)
	if reason != "" {
		writeSubscriptionBadRequest(w, r, "events", reason, msg)
		return
	}
	if len(body.Description) > maxWebhookDescriptionLen {
		writeSubscriptionBadRequest(w, r, "description", "description_too_long", "description is longer than 200 characters")
		return
	}
	secret, hint, err := newWebhookSecret()
	if err != nil {
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "secret generation failed", nil)
		return
	}
	wh := store.Webhook{
		ID:          quarrycontracts.NewID(quarrycontracts.KindWebhook),
		OrgID:       org,
		URL:         strings.TrimSpace(body.URL),
		Secret:      secret,
		SecretHint:  hint,
		Events:      events,
		Active:      true,
		Description: body.Description,
		CreatedBy:   body.CreatedBy,
		CreatedAt:   time.Now().Unix(),
	}
	if err := h.db.Webhooks().CreateCapped(wh, h.opts.MaxPerOrg); err != nil {
		switch {
		case errors.Is(err, store.ErrConflict):
			httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "a subscription with this url and events already exists",
				map[string]any{"reason": "subscription_exists"})
		case errors.Is(err, store.ErrQuotaExceeded):
			httpx.WriteErr(w, r, quarrycontracts.CodeRateLimited, "the organization is at its webhook subscription limit",
				map[string]any{"reason": "subscription_limit", "limit": h.opts.MaxPerOrg})
		default:
			httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "create failed", nil)
		}
		return
	}
	httpx.WriteJSON(w, r, http.StatusCreated, webhookSubscriptionCreated{webhookSubscriptionView: toSubscriptionView(wh), Secret: secret})
}

func (h *subscriptionHandlers) list(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	if limit <= 0 {
		limit = subscriptionsPageDefault
	}
	if limit > subscriptionsPageMax {
		limit = subscriptionsPageMax
	}
	items, next := h.db.Webhooks().ListByOrg(org, limit, r.URL.Query().Get("cursor"))
	out := make([]webhookSubscriptionView, 0, len(items))
	for _, it := range items {
		out = append(out, h.viewWithLast(org, it))
	}
	var nextCursor any
	if next != "" {
		nextCursor = next
	}
	httpx.WriteJSON(w, r, http.StatusOK, map[string]any{"items": out, "next_cursor": nextCursor})
}

func (h *subscriptionHandlers) get(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	wh, ok := h.load(w, r, org)
	if !ok {
		return
	}
	httpx.WriteJSON(w, r, http.StatusOK, h.viewWithLast(org, wh))
}

type patchSubscriptionBody struct {
	Active      *bool     `json:"active"`
	Events      *[]string `json:"events"`
	Description *string   `json:"description"`
}

func (h *subscriptionHandlers) patch(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	wh, ok := h.load(w, r, org)
	if !ok {
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, 64<<10)
	var body patchSubscriptionBody
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "decode body: "+err.Error(), nil)
		return
	}
	if body.Active != nil {
		wh.Active = *body.Active
	}
	if body.Events != nil {
		events, reason, msg := normalizeSubscriptionEvents(*body.Events)
		if reason != "" {
			writeSubscriptionBadRequest(w, r, "events", reason, msg)
			return
		}
		wh.Events = events
	}
	if body.Description != nil {
		if len(*body.Description) > maxWebhookDescriptionLen {
			writeSubscriptionBadRequest(w, r, "description", "description_too_long", "description is longer than 200 characters")
			return
		}
		wh.Description = *body.Description
	}
	if err := h.db.Webhooks().UpdateByOrg(wh); err != nil {
		if errors.Is(err, store.ErrNotFound) {
			writeSubscriptionNotFound(w, r, string(wh.ID))
			return
		}
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "update failed", nil)
		return
	}
	httpx.WriteJSON(w, r, http.StatusOK, h.viewWithLast(org, wh))
}

func (h *subscriptionHandlers) delete(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	id := chi.URLParam(r, "id")
	if err := h.db.Webhooks().DeleteByOrg(quarrycontracts.ID(id), org); err != nil {
		if errors.Is(err, store.ErrNotFound) {
			writeSubscriptionNotFound(w, r, id)
			return
		}
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "delete failed", nil)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func (h *subscriptionHandlers) rotateSecret(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	wh, ok := h.load(w, r, org)
	if !ok {
		return
	}
	secret, hint, err := newWebhookSecret()
	if err != nil {
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "secret generation failed", nil)
		return
	}
	wh.Secret, wh.SecretHint = secret, hint
	if err := h.db.Webhooks().UpdateByOrg(wh); err != nil {
		if errors.Is(err, store.ErrNotFound) {
			writeSubscriptionNotFound(w, r, string(wh.ID))
			return
		}
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "rotate failed", nil)
		return
	}
	httpx.WriteJSON(w, r, http.StatusOK, map[string]any{"id": wh.ID, "secret": secret, "secret_hint": hint})
}

var deliveryStatuses = map[string]bool{"pending": true, "in_flight": true, "success": true, "failed": true, "dlq": true}

func (h *subscriptionHandlers) deliveries(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	wh, ok := h.load(w, r, org)
	if !ok {
		return
	}
	status := r.URL.Query().Get("status")
	if status != "" && !deliveryStatuses[status] {
		writeSubscriptionBadRequest(w, r, "status", "status_unsupported", "status must be one of pending, in_flight, success, failed, dlq")
		return
	}
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	if limit <= 0 {
		limit = subscriptionsPageDefault
	}
	if limit > subscriptionsPageMax {
		limit = subscriptionsPageMax
	}
	items, next := h.db.WebhookDeliveries().ListByWebhook(org, wh.ID, status, limit, r.URL.Query().Get("cursor"))
	out := make([]webhookDeliveryView, 0, len(items))
	for _, it := range items {
		out = append(out, toDeliveryView(it))
	}
	var nextCursor any
	if next != "" {
		nextCursor = next
	}
	httpx.WriteJSON(w, r, http.StatusOK, map[string]any{"items": out, "next_cursor": nextCursor})
}

// retryDelivery re-queues a delivery that gave up (failed or dlq). A delivery that is still
// pending, in flight or already delivered is a 409: retrying it would double-deliver.
func (h *subscriptionHandlers) retryDelivery(w http.ResponseWriter, r *http.Request) {
	org, ok := orgOr400(w, r)
	if !ok {
		return
	}
	wh, ok := h.load(w, r, org)
	if !ok {
		return
	}
	did := chi.URLParam(r, "did")
	d, ok := h.db.WebhookDeliveries().GetByOrg(quarrycontracts.ID(did), org)
	if !ok || d.WebhookID != wh.ID {
		writeSubscriptionNotFound(w, r, did)
		return
	}
	if d.Status != "dlq" && d.Status != "failed" {
		httpx.WriteErr(w, r, quarrycontracts.CodeConflict, "only a failed or dlq delivery can be retried",
			map[string]any{"reason": "delivery_not_retryable", "status": d.Status})
		return
	}
	d.Status, d.Attempt, d.LastError, d.NextAttemptAt = "pending", 0, "", time.Now().Unix()
	if err := h.db.WebhookDeliveries().Update(d); err != nil {
		httpx.WriteErr(w, r, quarrycontracts.CodeInternal, "retry failed", nil)
		return
	}
	httpx.WriteJSON(w, r, http.StatusAccepted, toDeliveryView(d))
}

// WebhookSubscriptionsEnabled reports whether QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS is set to
// a truthy value (true, 1, yes, on). Default false: the subscription routes are not mounted.
func WebhookSubscriptionsEnabled(getenv func(string) string) bool {
	switch strings.ToLower(strings.TrimSpace(getenv("QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS"))) {
	case "true", "1", "yes", "on":
		return true
	}
	return false
}
