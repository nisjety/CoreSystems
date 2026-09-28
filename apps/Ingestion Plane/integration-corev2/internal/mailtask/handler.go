package mailtask

import (
	"context"
	"strings"
	"time"
)

// Reason enumerates every terminal outcome HandleInboundPostmark can reach.
// It is a closed set specifically so the HTTP layer can log content-free
// (this reason string, never subject/body/attachment content) and so tests
// can assert on it precisely.
type Reason string

const (
	ReasonDisabled        Reason = "disabled"
	ReasonPayloadTooLarge Reason = "payload_too_large"
	ReasonUnauthorized    Reason = "unauthorized"
	ReasonInvalidPayload  Reason = "invalid_payload"
	ReasonWrongDomain     Reason = "wrong_receiving_domain"
	ReasonAliasNotFound   Reason = "alias_not_found"
	// ReasonAliasResolverUnavailable is distinct from ReasonAliasNotFound:
	// the alias resolver itself failed (network error, or user-core answered
	// anything other than 200/404) rather than reporting there is genuinely
	// no such alias. Unlike ReasonAliasNotFound (which must answer 200 and
	// never leak *why*, per spec §3.2), this is a transient condition the
	// HTTP layer must map to a retryable status -- collapsing it into
	// ReasonAliasNotFound would make a brief user-core outage during
	// Postmark's delivery window silently and permanently drop the email.
	ReasonAliasResolverUnavailable Reason = "alias_resolver_unavailable"
	ReasonSenderNotVerified        Reason = "sender_not_verified"
	ReasonDuplicate                Reason = "duplicate"
	ReasonGatewayUnavailable       Reason = "gateway_unavailable"
	ReasonAccepted                 Reason = "accepted"
)

// Result is what HandleInboundPostmark returns: enough to pick an HTTP status
// and log one content-free line, nothing more.
type Result struct {
	Reason Reason
	Err    error
}

// Handler wires every collaborator the inbound pipeline needs. All fields are
// required for a live deployment; a zero-value Handler (or one with
// WebhookSecret/Provider unset) always returns ReasonDisabled without
// touching any collaborator -- matching "off by default until the owner
// finishes the Postmark account + DNS setup" (spec §7.1).
type Handler struct {
	// Provider must equal "postmark" (case-insensitive) or every request is
	// ReasonDisabled. Only Postmark is implemented for v1 (owner ruling
	// 2026-09-28); this field exists so a future provider addition (spec
	// §3.2, SendGrid/Mailgun) is a config flip, not a new route.
	Provider        string
	WebhookSecret   string
	ReceivingDomain string
	Resolver        AliasResolver
	Extractor       AttachmentExtractor
	Gateway         GatewayIngestClient
	Replay          *ReplayGuard
	Now             func() time.Time
}

func (h *Handler) now() time.Time {
	if h.Now != nil {
		return h.Now()
	}
	return time.Now().UTC()
}

// enabled reports whether the feature is configured at all. Both the
// provider AND the webhook secret must be set -- an operator who only sets
// one has an obviously incomplete configuration, and refusing is safer than
// guessing which half they meant.
func (h *Handler) enabled() bool {
	return h != nil &&
		strings.EqualFold(strings.TrimSpace(h.Provider), "postmark") &&
		strings.TrimSpace(h.WebhookSecret) != ""
}

// HandleInboundPostmark runs the full pipeline: webhook auth, payload parse,
// receiving-domain check, replay dedup, alias resolution, sender
// verification, attachment extraction, and forwarding to the gateway. It
// never returns partial success -- either every step passes and Gateway.Ingest
// is called exactly once, or the pipeline stops at the first failing step.
func (h *Handler) HandleInboundPostmark(ctx context.Context, authorizationHeader string, body []byte) Result {
	if !h.enabled() {
		return Result{Reason: ReasonDisabled}
	}
	if len(body) > maxInboundPayloadBytes {
		return Result{Reason: ReasonPayloadTooLarge}
	}
	if !VerifyWebhookAuth(authorizationHeader, h.WebhookSecret) {
		return Result{Reason: ReasonUnauthorized}
	}
	event, err := ParsePostmarkInboundPayload(body)
	if err != nil {
		return Result{Reason: ReasonInvalidPayload, Err: err}
	}
	if event.Alias == "" {
		return Result{Reason: ReasonInvalidPayload}
	}
	if !VerifyReceivingDomain(event.ToDomain, h.ReceivingDomain) {
		return Result{Reason: ReasonWrongDomain}
	}
	now := h.now()
	if h.Replay != nil && h.Replay.SeenBefore("postmark\x00"+event.ProviderMessageID, now) {
		return Result{Reason: ReasonDuplicate}
	}
	if h.Resolver == nil {
		return Result{Reason: ReasonAliasNotFound}
	}
	resolved, ok, err := h.Resolver.Resolve(ctx, event.Alias, event.ProviderMessageID)
	if err != nil {
		// The resolver itself failed (transport error, or user-core answered
		// something other than 200/404) -- a transient condition, not "no
		// such alias". Retryable, unlike the genuine not-found case below.
		return Result{Reason: ReasonAliasResolverUnavailable, Err: err}
	}
	if !ok {
		// Identical outcome whether the alias never existed, was revoked, or
		// its owner has no verified email -- user-core's resolve endpoint
		// already collapsed those cases (spec §6); this handler does not
		// re-introduce the distinction.
		return Result{Reason: ReasonAliasNotFound}
	}
	if !VerifySender(event, resolved.VerifiedEmail) {
		// Dropped silently to the sender (no bounce, no content in the log --
		// only the reason code) per spec §3.2: "reject (200 + drop, never
		// leak *why* to the sender)".
		return Result{Reason: ReasonSenderNotVerified}
	}

	extracted := ExtractAttachments(ctx, h.Extractor, event.Attachments)
	attachments := make([]GatewayIngestAttachment, 0, len(extracted))
	for _, item := range extracted {
		attachments = append(attachments, GatewayIngestAttachment{
			Filename:      item.Filename,
			ExtractedText: item.Text,
			Extracted:     item.Extracted,
		})
	}

	if h.Gateway == nil {
		return Result{Reason: ReasonGatewayUnavailable}
	}
	err = h.Gateway.Ingest(ctx, GatewayIngestRequest{
		OrgID:             resolved.OrgID,
		UserID:            resolved.UserID,
		ProviderMessageID: event.ProviderMessageID,
		Subject:           event.Subject,
		BodyText:          firstNonEmptyBody(event.TextBody, event.HTMLBody),
		Attachments:       attachments,
		OccurredAt:        now,
		ThreadDecision:    resolved.ThreadDecision,
	})
	if err != nil {
		return Result{Reason: ReasonGatewayUnavailable, Err: err}
	}
	return Result{Reason: ReasonAccepted}
}

func firstNonEmptyBody(text, html string) string {
	if strings.TrimSpace(text) != "" {
		return text
	}
	return html
}
