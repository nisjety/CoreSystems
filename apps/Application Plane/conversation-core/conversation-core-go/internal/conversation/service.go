package conversation

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

// OutboundSender delivers a human agent's reply to the customer through
// integration-corev2. *integration.Client satisfies it. A nil sender makes
// external replies unavailable; the service fails before persistence so it can
// never claim that an unsent reply was sent.
type OutboundSender interface {
	Send(ctx context.Context, req integration.SendRequest) (*integration.SendResult, error)
}

// AIProposalPolicy is the Control Plane-backed policy decision required before
// a proposed AI action is retained. The durable service boundary invokes it so
// HTTP callers, Model Plane consumers, and future workers follow one rule.
type AIProposalPolicy interface {
	AllowAIProposal(ctx context.Context, orgID string) error
}

type Service struct {
	repository       Repository
	publisher        EventPublisher
	sender           OutboundSender
	aiProposalPolicy AIProposalPolicy
	now              func() time.Time
	// feedbackMirrorOrgID is FEEDBACK_MIRROR_ORG_ID (see config.Config), the
	// Verevon-owned monitored org every feedback submission is mirrored into.
	// Empty disables mirroring -- see Service.mirrorFeedback.
	feedbackMirrorOrgID string
}

type Option func(*Service)

func WithNow(now func() time.Time) Option {
	return func(s *Service) {
		if now != nil {
			s.now = now
		}
	}
}

// WithSender wires the outbound delivery client used to actually send human
// agent replies to channel-backed conversations (whatsapp, messenger, …).
func WithSender(sender OutboundSender) Option {
	return func(s *Service) {
		s.sender = sender
	}
}

func WithAIProposalPolicy(policy AIProposalPolicy) Option {
	return func(s *Service) {
		s.aiProposalPolicy = policy
	}
}

// WithFeedbackMirrorOrgID configures the Verevon-owned monitored org that
// every Service.SubmitFeedback submission is mirrored into, in addition to
// the submitter's own org. Pass the empty string (the zero value, so this
// option can always be registered unconditionally) to disable mirroring --
// see Service.mirrorFeedback for the resulting behavior.
func WithFeedbackMirrorOrgID(orgID string) Option {
	return func(s *Service) {
		s.feedbackMirrorOrgID = strings.TrimSpace(orgID)
	}
}

func NewService(repository Repository, publisher EventPublisher, opts ...Option) *Service {
	service := &Service{
		repository: repository,
		publisher:  publisher,
		now:        time.Now,
	}
	for _, opt := range opts {
		opt(service)
	}
	return service
}

func (s *Service) ListInboxes(ctx context.Context, orgID string) ([]Inbox, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListInboxes(ctx, orgID)
}

func (s *Service) ListConversations(ctx context.Context, filter ListFilter) ([]ConversationSummary, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.InboxID = strings.TrimSpace(filter.InboxID)
	filter.ConnectionID = strings.TrimSpace(filter.ConnectionID)
	filter.Status = normalizeStatus(filter.Status)
	filter.Assigned = strings.TrimSpace(filter.Assigned)
	filter.Channel = strings.TrimSpace(filter.Channel)
	filter.Query = strings.TrimSpace(filter.Query)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListConversations(ctx, filter)
}

func (s *Service) GetConversation(ctx context.Context, orgID, conversationID string) (*ConversationDetail, error) {
	orgID = strings.TrimSpace(orgID)
	conversationID = strings.TrimSpace(conversationID)
	if orgID == "" || conversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	return s.repository.GetConversation(ctx, orgID, conversationID)
}

func (s *Service) IngestEvent(ctx context.Context, event InboundEvent) (*StoredEventResult, error) {
	event = normalizeInboundEvent(event, s.now)
	if err := validateInboundEvent(event); err != nil {
		return nil, err
	}
	result, err := s.repository.StoreInboundEvent(ctx, event)
	if err != nil {
		return nil, fmt.Errorf("store inbound event: %w", err)
	}
	if isMachineDeliveryFailureReport(event) {
		if recorder, ok := s.repository.(interface {
			RecordEmailDeliveryFailure(context.Context, EmailDeliveryFailureInput) (bool, error)
		}); ok {
			if _, receiptErr := recorder.RecordEmailDeliveryFailure(ctx, EmailDeliveryFailureInput{
				OrgID: event.OrgID, Provider: event.Provider,
				OutboundIntentID: event.OutboundCorrelationID, OccurredAt: event.OccurredAt,
			}); receiptErr != nil {
				return nil, fmt.Errorf("record email delivery failure: %w", receiptErr)
			}
		}
	}
	if result != nil && result.Created {
		subject := SubjectMessageReceived
		if event.Direction == DirectionOutbound {
			subject = SubjectMessageSent
		}
		data := map[string]any{
			"provider":            event.Provider,
			"provider_event_id":   event.ProviderEventID,
			"provider_message_id": event.ProviderMessageID,
		}
		// Follower ids are a bounded, content-free projection for the
		// notification consumer. This keeps recipient selection with the
		// conversation authority while leaving delivery, membership checks,
		// preferences, and feed rendering in notification-core.
		if subject == SubjectMessageReceived && result.Detail != nil {
			// Keep fan-out optional at this generic repository boundary. It
			// avoids making unrelated lightweight service doubles implement a
			// notification-only projection, while PGRepository supplies it in
			// production.
			if followers, ok := s.repository.(interface {
				ListConversationFollowerIDs(context.Context, string, string) ([]string, error)
			}); ok {
				followerIDs, followerErr := followers.ListConversationFollowerIDs(ctx, result.Detail.OrgID, result.Detail.ID)
				if followerErr != nil {
					log.Printf("conversation-core-go: list followers for notification fan-out (org=%s conversation=%s): %v", result.Detail.OrgID, result.Detail.ID, followerErr)
				} else if len(followerIDs) > 0 {
					data["follower_user_ids"] = followerIDs
				}
			}
		}
		s.publish(ctx, subject, result.Detail, result.Message, "", data)
	}
	return result, nil
}

// SubmitFeedback stores a signed-in org member's one-line friction report as
// a new conversation on the same inbound-ingest path real provider webhooks
// use (IngestEvent), tags it FeedbackTag so the Inbox can surface it in its
// own queue, and mirrors it into the configured FEEDBACK_MIRROR_ORG_ID org
// (see mirrorFeedback) so the team can see it even when the submitter's own
// org is an external pilot tenant. Each call is idempotent on
// input.IdempotencyKey -- a retried submission (e.g. a double-click) resolves
// to the conversation created by the first attempt instead of creating a
// duplicate ticket, and the duplicate is not re-tagged or re-mirrored.
func (s *Service) SubmitFeedback(ctx context.Context, input FeedbackInput) (*ConversationDetail, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.BodyText = strings.TrimSpace(input.BodyText)
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	input.PageURL = strings.TrimSpace(input.PageURL)
	// A caller-supplied idempotency key is mandatory (unlike IngestEvent's
	// fallback derivation from provider refs): feedback has no provider
	// event/message/thread id to fall back to, so every submission from the
	// same org would otherwise derive the SAME key and silently collapse into
	// one conversation, dropping every submission after the first.
	if input.OrgID == "" || input.BodyText == "" || input.IdempotencyKey == "" {
		return nil, fmt.Errorf("%w: org_id, body_text, and idempotency_key are required", ErrInvalidInput)
	}

	fromName := strings.TrimSpace(input.FromName)
	fromEmail := strings.TrimSpace(input.FromEmail)
	if fromName == "" && fromEmail == "" {
		fromName = "Feedback submitter"
	}
	bodyText := input.BodyText
	if input.PageURL != "" {
		bodyText = fmt.Sprintf("%s\n\nReported from: %s", bodyText, input.PageURL)
	}
	subject := input.BodyText
	if len(subject) > 80 {
		subject = subject[:80] + "…"
	}
	result, err := s.IngestEvent(ctx, InboundEvent{
		IDempotencyKey: input.IdempotencyKey,
		OrgID:          input.OrgID,
		Provider:       FeedbackProvider,
		Direction:      DirectionInbound,
		Subject:        "Feedback: " + subject,
		From:           ParticipantInput{Name: fromName, Email: fromEmail},
		BodyText:       bodyText,
	})
	if err != nil {
		return nil, err
	}
	if result == nil || result.Detail == nil {
		return nil, fmt.Errorf("%w: feedback conversation was not created", ErrInvalidInput)
	}

	var detail *ConversationDetail
	if !result.Created {
		// Idempotent replay of an earlier submission -- already tagged (and
		// already mirrored, if mirroring was configured at the time).
		detail = result.Detail
	} else {
		detail, err = s.AddTag(ctx, input.OrgID, result.Detail.ID, FeedbackTag, input.ActorUserID)
		if err != nil {
			return nil, err
		}
	}

	// Best-effort: never let a mirroring problem fail the submitter's own
	// (already-succeeded) submission above.
	s.mirrorFeedback(ctx, input, result.Detail.ID)

	return detail, nil
}

// mirrorFeedback copies a feedback submission into the Verevon-owned
// FEEDBACK_MIRROR_ORG_ID org, in addition to the submitter's own org.
// Without this, feedback submitted from inside an external pilot org's own
// isolated tenant would be invisible to the team: conversation-core-go has no
// cross-org/platform-admin read bypass, so nobody on the team is a member of
// that org's Inbox. The mirrored conversation's body names the submitting
// org, the submitter's identity, and the original conversation id, so a team
// member reading it in their OWN org's Inbox knows exactly where it came
// from -- the whole point of the mirror is that it now lives outside the
// customer's own tenant.
//
// Best-effort and never returns an error to the caller: a mirroring failure
// is logged, never propagated, so a mirroring outage can never turn an
// already-successful feedback submission into a failed one. Mirroring is
// skipped entirely when FEEDBACK_MIRROR_ORG_ID is unset, and when the
// submitter's own org already IS the mirror-target org (e.g. a team member
// testing their own product) -- that case would just duplicate the
// conversation already created in SubmitFeedback above.
func (s *Service) mirrorFeedback(ctx context.Context, input FeedbackInput, originalConversationID string) {
	mirrorOrgID := s.feedbackMirrorOrgID
	if mirrorOrgID == "" {
		log.Printf("conversation-core-go: FEEDBACK_MIRROR_ORG_ID is unset -- skipping feedback mirror (org=%s conversation=%s)", input.OrgID, originalConversationID)
		return
	}
	if mirrorOrgID == input.OrgID {
		// The submitter's own org already IS the monitored org -- mirroring
		// would just create a redundant duplicate of the conversation above.
		return
	}

	fromName := strings.TrimSpace(input.FromName)
	fromEmail := strings.TrimSpace(input.FromEmail)
	reporter := fromName
	switch {
	case reporter != "" && fromEmail != "":
		reporter = fmt.Sprintf("%s <%s>", reporter, fromEmail)
	case reporter == "" && fromEmail != "":
		reporter = fromEmail
	case reporter == "":
		reporter = "unknown submitter"
	}
	if input.ActorUserID != "" {
		reporter = fmt.Sprintf("%s (user %s)", reporter, input.ActorUserID)
	}

	body := fmt.Sprintf(
		"%s\n\n---\nMirrored cross-org feedback -- not filed in this org.\nSubmitting org: %s\nSubmitted by: %s\nOriginal conversation: %s (org %s)",
		input.BodyText, input.OrgID, reporter, originalConversationID, input.OrgID,
	)
	if input.PageURL != "" {
		body += fmt.Sprintf("\nReported from: %s", input.PageURL)
	}

	subject := input.BodyText
	if len(subject) > 60 {
		subject = subject[:60] + "…"
	}
	subject = fmt.Sprintf("Feedback [%s]: %s", input.OrgID, subject)

	// Distinct from the original's idempotency key so the two conversations
	// (own-org and mirror) are independently idempotent -- a replayed
	// SubmitFeedback call resolves each side to its own first attempt instead
	// of colliding with, or skipping, the other.
	mirrorResult, err := s.IngestEvent(ctx, InboundEvent{
		IDempotencyKey: input.IdempotencyKey + ":mirror",
		OrgID:          mirrorOrgID,
		Provider:       FeedbackProvider,
		Direction:      DirectionInbound,
		Subject:        subject,
		From:           ParticipantInput{Name: reporter, Email: fromEmail},
		BodyText:       body,
	})
	if err != nil {
		log.Printf("conversation-core-go: feedback mirror ingest failed (org=%s mirror_org=%s): %v", input.OrgID, mirrorOrgID, err)
		return
	}
	if mirrorResult == nil || mirrorResult.Detail == nil {
		log.Printf("conversation-core-go: feedback mirror produced no conversation (org=%s mirror_org=%s)", input.OrgID, mirrorOrgID)
		return
	}
	if !mirrorResult.Created {
		// Idempotent replay -- already tagged from the first mirror attempt.
		return
	}
	if _, err := s.AddTag(ctx, mirrorOrgID, mirrorResult.Detail.ID, FeedbackTag, input.ActorUserID); err != nil {
		log.Printf("conversation-core-go: feedback mirror tag failed (org=%s mirror_org=%s conversation=%s): %v", input.OrgID, mirrorOrgID, mirrorResult.Detail.ID, err)
		return
	}
	if _, err := s.AddTag(ctx, mirrorOrgID, mirrorResult.Detail.ID, FeedbackMirrorTag, input.ActorUserID); err != nil {
		log.Printf("conversation-core-go: feedback mirror cross-org tag failed (org=%s mirror_org=%s conversation=%s): %v", input.OrgID, mirrorOrgID, mirrorResult.Detail.ID, err)
	}
}

func (s *Service) AddMessage(ctx context.Context, input AddMessageInput) (*Message, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.ActorName = strings.TrimSpace(input.ActorName)
	input.ActorEmail = strings.TrimSpace(input.ActorEmail)
	input.BodyText = strings.TrimSpace(input.BodyText)
	input.BodyHTML = strings.TrimSpace(input.BodyHTML)
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	if strings.TrimSpace(input.Direction) == "" {
		input.Direction = DirectionOutbound
	}
	if input.OccurredAt.IsZero() {
		input.OccurredAt = s.now().UTC()
	}
	if input.OrgID == "" || input.ConversationID == "" || input.BodyText == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and body_text are required", ErrInvalidInput)
	}
	if !input.Internal && input.Direction == DirectionOutbound && !validOutboundIdempotencyKey(input.IdempotencyKey) {
		return nil, fmt.Errorf("%w: a valid idempotency_key is required for external replies", ErrInvalidInput)
	}
	// Human outbound replies must have a configured sender and a supported,
	// tenant-scoped channel reference. Internal notes are the only store-only
	// path. Send first, then persist, so absent routing cannot become a false
	// customer-send success.
	if !input.Internal && input.Direction == DirectionOutbound {
		if s.sender == nil {
			return nil, ErrDeliveryUnavailable
		}
		ref, err := s.resolveReplyTarget(ctx, input.OrgID, input.ConversationID)
		if err != nil {
			return nil, err
		}
		intentID := OutboundIntentID(input.OrgID, input.IdempotencyKey)
		sendRequest := integration.SendRequest{
			OrgID: input.OrgID, ActorUserID: input.ActorUserID,
			Provider: ref.Provider, ConnectionID: ref.ConnectionID, ProviderThreadID: ref.ProviderThreadID,
			InReplyTo: ref.ReplyToMessageID, References: ref.ReferencesHeader,
			OutboundCorrelationID: intentID,
			BodyText:              input.BodyText, BodyHTML: input.BodyHTML,
			AuthorizationKind: "human_intent", AuthorizationID: intentID, ActionID: intentID,
			IdempotencyKey: "conversation:" + input.IdempotencyKey,
		}
		prepared, err := integration.PrepareSend(sendRequest)
		if err != nil {
			return nil, fmt.Errorf("%w: provider action could not be prepared", ErrDeliveryUnavailable)
		}
		sendRequest.PayloadSHA256 = prepared.PayloadSHA256
		fingerprint := OutboundRequestFingerprint(input.OrgID, input.ConversationID, input.IdempotencyKey, input.ActorUserID, ref, input.BodyText, input.BodyHTML)
		claim, err := s.repository.ClaimOutboundIntent(ctx, OutboundIntentClaimInput{
			IntentID:           intentID,
			OrgID:              input.OrgID,
			IdempotencyKey:     input.IdempotencyKey,
			ConversationID:     input.ConversationID,
			RequestFingerprint: fingerprint,
			Provider:           ref.Provider,
			ConnectionID:       ref.ConnectionID,
			ProviderThreadID:   ref.ProviderThreadID,
			AuthorizationKind:  "human_intent",
			ActorUserID:        input.ActorUserID,
			ActionID:           intentID,
			Operation:          prepared.Operation,
			PayloadSHA256:      prepared.PayloadSHA256,
		})
		if err != nil {
			return nil, err
		}
		if !claim.Claimed {
			return s.replayOutboundIntent(ctx, claim.Intent)
		}

		result, sendErr := s.sender.Send(ctx, sendRequest)
		if sendErr != nil {
			if integration.IsSafeToRetry(sendErr) {
				if markErr := s.repository.MarkOutboundIntentOutcome(ctx, OutboundIntentOutcomeInput{
					OrgID: input.OrgID, IdempotencyKey: input.IdempotencyKey,
					Status: OutboundIntentRetryable, ErrorCode: integration.ErrorCode(sendErr),
				}); markErr != nil {
					log.Printf("[cc-go] record pre-provider retryable outcome org=%s conversation=%s: %v", input.OrgID, input.ConversationID, markErr)
					return nil, ErrDeliveryUnknown
				}
				return nil, fmt.Errorf("%w: %s", ErrSendFailed, integration.ErrorCode(sendErr))
			}
			status := OutboundIntentUnknown
			resultErr := ErrDeliveryUnknown
			if integration.IsTerminal(sendErr) {
				status = OutboundIntentFailed
				resultErr = ErrSendFailed
			}
			if markErr := s.repository.MarkOutboundIntentOutcome(ctx, OutboundIntentOutcomeInput{
				OrgID: input.OrgID, IdempotencyKey: input.IdempotencyKey,
				Status: status, ErrorCode: integration.ErrorCode(sendErr),
			}); markErr != nil {
				log.Printf("[cc-go] record outbound outcome org=%s conversation=%s code=%s: %v", input.OrgID, input.ConversationID, integration.ErrorCode(sendErr), markErr)
			}
			return nil, fmt.Errorf("%w: %s", resultErr, integration.ErrorCode(sendErr))
		}
		providerMessageID := ""
		if result != nil {
			providerMessageID = result.ProviderMessageID
		}
		message, finalizeErr := s.repository.FinalizeOutboundIntent(ctx, OutboundIntentFinalizeInput{
			OrgID:              input.OrgID,
			IdempotencyKey:     input.IdempotencyKey,
			RequestFingerprint: fingerprint,
			Message:            input,
			ProviderMessageID:  providerMessageID,
		})
		if finalizeErr != nil {
			if markErr := s.repository.MarkOutboundIntentOutcome(ctx, OutboundIntentOutcomeInput{
				OrgID: input.OrgID, IdempotencyKey: input.IdempotencyKey,
				Status: OutboundIntentUnknown, ErrorCode: "local_finalize_failed",
			}); markErr != nil {
				log.Printf("[cc-go] mark outbound finalization unknown org=%s conversation=%s: %v", input.OrgID, input.ConversationID, markErr)
			}
			return nil, ErrDeliveryUnknown
		}
		s.publish(ctx, SubjectMessageSent, &ConversationDetail{ConversationSummary: ConversationSummary{ID: input.ConversationID, OrgID: input.OrgID}}, message, input.ActorUserID, map[string]any{"outbound_status": OutboundIntentSubmitted})
		return message, nil
	}
	message, err := s.repository.AddMessage(ctx, input)
	if err != nil {
		return nil, err
	}
	subject := SubjectMessageSent
	if input.Internal {
		subject = SubjectNoteCreated
	}
	s.publish(ctx, subject, &ConversationDetail{ConversationSummary: ConversationSummary{ID: input.ConversationID, OrgID: input.OrgID}}, message, input.ActorUserID, nil)
	return message, nil
}

func (s *Service) resolveReplyTarget(ctx context.Context, orgID, conversationID string) (*ChannelThreadRef, error) {
	ref, refErr := s.repository.GetChannelThreadRefByConversation(ctx, orgID, conversationID)
	if errors.Is(refErr, ErrNotFound) {
		return nil, ErrDeliveryUnavailable
	}
	if refErr != nil {
		return nil, refErr
	}
	if !integration.SupportsSend(ref.Provider) {
		return nil, fmt.Errorf("%w: unsupported provider %q", ErrDeliveryUnavailable, ref.Provider)
	}
	return ref, nil
}

func (s *Service) replayOutboundIntent(ctx context.Context, intent OutboundIntent) (*Message, error) {
	switch intent.Status {
	case OutboundIntentSubmitted:
		if intent.MessageID == "" {
			return nil, ErrDeliveryUnknown
		}
		message, err := s.repository.GetMessage(ctx, intent.OrgID, intent.MessageID)
		if err != nil {
			return nil, ErrDeliveryUnknown
		}
		return message, nil
	case OutboundIntentFailed:
		return nil, fmt.Errorf("%w: %s", ErrSendFailed, intent.ErrorCode)
	case OutboundIntentSending, OutboundIntentUnknown:
		return nil, ErrDeliveryUnknown
	default:
		return nil, ErrDeliveryUnknown
	}
}

// ReconcileStaleOutboundIntents is the stuck-send sweep for the review-approve-send
// path: it flips every outbound intent still `sending` after staleAfter to
// `unknown` (a crash between ClaimOutboundIntent and
// FinalizeOutboundIntent/MarkOutboundIntentOutcome — see
// PGRepository.ReconcileStaleOutboundIntents) and publishes the same
// SubjectAIActionSendUnknown alert the live send path already emits for an
// ambiguous outcome, so operators see it exactly once regardless of which
// path produced it. It never contacts a provider and never retries
// automatically — a `sending` row does not prove the provider was never
// called, so blind retransmission could double-send a message the customer
// already received (e.g. a duplicate email). Intended to be invoked
// periodically by consumers.OutboundIntentReconciler.
func (s *Service) ReconcileStaleOutboundIntents(ctx context.Context, staleAfter time.Duration) ([]OutboundIntent, error) {
	reconciled, err := s.repository.ReconcileStaleOutboundIntents(ctx, staleAfter)
	if err != nil {
		return nil, err
	}
	for _, intent := range reconciled {
		log.Printf("conversation-core-go: reconciled stale outbound intent %s (org=%s conversation=%s ai_action=%s provider=%s) sending -> unknown", intent.ID, intent.OrgID, intent.ConversationID, intent.AIActionID, intent.Provider)
		s.publish(ctx, SubjectAIActionSendUnknown, &ConversationDetail{ConversationSummary: ConversationSummary{
			ID:    intent.ConversationID,
			OrgID: intent.OrgID,
		}}, nil, intent.ActorUserID, map[string]any{
			"ai_action_id":    intent.AIActionID,
			"idempotency_key": intent.IdempotencyKey,
			"provider":        intent.Provider,
			"status":          OutboundIntentUnknown,
			"error_code":      OutboundIntentErrorStaleSendingTimeout,
		})
	}
	return reconciled, nil
}

// OutboundRequestFingerprint binds a durable intent to its exact tenant,
// conversation, approval, actor, route, and content without persisting a second
// copy of message content in the ledger.
func OutboundRequestFingerprint(orgID, conversationID, approvalID, actorUserID string, ref *ChannelThreadRef, bodyText, bodyHTML string) string {
	canonical := struct {
		OrgID, ConversationID, ApprovalID, ActorUserID string
		Provider, ConnectionID, ProviderThreadID       string
		BodyText, BodyHTML                             string
	}{
		OrgID: strings.TrimSpace(orgID), ConversationID: strings.TrimSpace(conversationID),
		ApprovalID: strings.TrimSpace(approvalID), ActorUserID: strings.TrimSpace(actorUserID),
		Provider: strings.TrimSpace(ref.Provider), ConnectionID: strings.TrimSpace(ref.ConnectionID),
		ProviderThreadID: strings.TrimSpace(ref.ProviderThreadID), BodyText: strings.TrimSpace(bodyText), BodyHTML: strings.TrimSpace(bodyHTML),
	}
	payload, _ := json.Marshal(canonical)
	digest := sha256.Sum256(payload)
	return hex.EncodeToString(digest[:])
}

func validOutboundIdempotencyKey(value string) bool {
	if len(value) < 16 || len(value) > 128 {
		return false
	}
	for _, char := range value {
		if (char < 'a' || char > 'z') && (char < 'A' || char > 'Z') &&
			(char < '0' || char > '9') && char != '-' && char != '_' && char != ':' && char != '.' {
			return false
		}
	}
	return true
}

func (s *Service) UpdateStatus(ctx context.Context, input StatusUpdate) (*ConversationDetail, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.Status = normalizeStatus(input.Status)
	if input.OrgID == "" || input.ConversationID == "" || input.Status == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and status are required", ErrInvalidInput)
	}
	detail, err := s.repository.UpdateStatus(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectStatusChanged, detail, nil, input.ActorUserID, map[string]any{"status": input.Status})
	return detail, nil
}

func (s *Service) UpdateAssignment(ctx context.Context, input AssignmentUpdate) (*ConversationDetail, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.AssigneeUserID = strings.TrimSpace(input.AssigneeUserID)
	input.AssigneeName = strings.TrimSpace(input.AssigneeName)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	detail, err := s.repository.UpdateAssignment(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectAssignmentChanged, detail, nil, input.ActorUserID, map[string]any{
		"assignee_user_id": input.AssigneeUserID,
		"assignee_name":    input.AssigneeName,
	})
	return detail, nil
}

func (s *Service) AddTag(ctx context.Context, orgID, conversationID, tag, actorUserID string) (*ConversationDetail, error) {
	tag = strings.TrimSpace(tag)
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || tag == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and tag are required", ErrInvalidInput)
	}
	detail, err := s.repository.AddTag(ctx, orgID, conversationID, tag)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTagAdded, detail, nil, actorUserID, map[string]any{"tag": tag})
	return detail, nil
}

func (s *Service) RemoveTag(ctx context.Context, orgID, conversationID, tag, actorUserID string) (*ConversationDetail, error) {
	tag = strings.TrimSpace(tag)
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(conversationID) == "" || tag == "" {
		return nil, fmt.Errorf("%w: org_id, conversation_id, and tag are required", ErrInvalidInput)
	}
	detail, err := s.repository.RemoveTag(ctx, orgID, conversationID, tag)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTagRemoved, detail, nil, actorUserID, map[string]any{"tag": tag})
	return detail, nil
}

// aiActionEditableFields is the closed set of review fields the executor can
// consume: ticket fields live under payload.suggested_fields, while body_text
// replaces the exact top-level payload sent by draft.reply/internal.note. The
// status field is deliberately constrained further below to active work states;
// an AI proposal can never resolve, close, or snooze a ticket. Any other
// edited-fields key is dropped silently -- mirroring the JSON-decode posture
// where an unrecognized field is simply never bound, not an error over the
// whole request.
var aiActionEditableFields = map[string]bool{
	"body_text":       true,
	"category":        true,
	"priority":        true,
	"severity":        true,
	"intent":          true,
	"work_type":       true,
	"status":          true,
	"team_id":         true,
	"team_name":       true,
	"title":           true,
	"customer_impact": true,
	"summary":         true,
	"root_cause":      true,
}

// whitelistAIActionFieldEdits filters a reviewer's edited fields down to the
// non-empty, whitelisted keys, and only for an approval -- a reject must never
// carry edited fields through to the repository, even defensively (the
// frontend never sends them for reject; this is the backstop).
func whitelistAIActionFieldEdits(fields map[string]string, decision string) map[string]string {
	if decision != "approved" || len(fields) == 0 {
		return nil
	}
	out := map[string]string{}
	for key, value := range fields {
		if !aiActionEditableFields[key] {
			continue
		}
		value = strings.TrimSpace(value)
		if key == "work_type" {
			value = strings.ToLower(value)
			if !isTicketWorkType(value) {
				continue
			}
		}
		if key == "severity" && !isTicketSeverity(value) {
			continue
		}
		if key == "status" {
			value = normalizeTicketStatus(value)
			if !isAIReviewableTicketStatus(value) {
				continue
			}
		}
		if key == "title" && utf8.RuneCountInString(value) > 300 {
			continue
		}
		if key == "customer_impact" && utf8.RuneCountInString(value) > 2_000 {
			continue
		}
		if (key == "summary" || key == "root_cause") && utf8.RuneCountInString(value) > 2_000 {
			continue
		}
		if value == "" {
			continue
		}
		out[key] = value
	}
	if len(out) == 0 {
		return nil
	}
	return out
}

func (s *Service) ReviewAIAction(ctx context.Context, input AIActionReview) error {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.AIActionID = strings.TrimSpace(input.AIActionID)
	input.ReviewerID = strings.TrimSpace(input.ReviewerID)
	input.Decision = strings.TrimSpace(input.Decision)
	input.Comment = strings.TrimSpace(input.Comment)
	if input.OccurredAt.IsZero() {
		input.OccurredAt = s.now().UTC()
	}
	if input.OrgID == "" || input.AIActionID == "" || input.ReviewerID == "" || input.Decision == "" {
		return fmt.Errorf("%w: org_id, ai_action_id, reviewer_id, and decision are required", ErrInvalidInput)
	}
	if input.Decision != "approved" && input.Decision != "rejected" {
		return fmt.Errorf("%w: decision must be 'approved' or 'rejected'", ErrInvalidInput)
	}
	if input.Decision == "approved" {
		if bodyText, ok := input.EditedFields["body_text"]; ok {
			trimmedBodyText := strings.TrimSpace(bodyText)
			if trimmedBodyText == "" || utf8.RuneCountInString(trimmedBodyText) > maxConversationDraftRunes {
				return fmt.Errorf("%w: edited body_text must be between 1 and %d characters", ErrInvalidInput, maxConversationDraftRunes)
			}
		}
	}
	input.EditedFields = whitelistAIActionFieldEdits(input.EditedFields, input.Decision)
	if err := s.repository.ReviewAIAction(ctx, input); err != nil {
		return err
	}
	s.publish(ctx, SubjectAIActionReviewed, &ConversationDetail{ConversationSummary: ConversationSummary{OrgID: input.OrgID}}, nil, input.ReviewerID, map[string]any{
		"ai_action_id": input.AIActionID,
		"decision":     input.Decision,
	})
	return nil
}

// allowedAIActionKinds is the closed set of kinds a human/hook may propose via
// CreateAIAction. Keep it explicit so an arbitrary kind can never be queued and
// later "executed" — each kind must have a dedicated, audited act-leg.
var allowedAIActionKinds = map[string]bool{
	"draft.reply":     true,
	"internal.note":   true,
	"ticket.update":   true,
	"incident.create": true,
	"problem.create":  true,
}

const maxDraftReplyRunes = 8_000

func normalizeProposalGroupID(value string) (string, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return "", nil
	}
	if utf8.RuneCountInString(value) > 120 {
		return "", fmt.Errorf("%w: proposal_group_id exceeds 120 characters", ErrInvalidInput)
	}
	for _, char := range value {
		if !(char >= 'a' && char <= 'z') && !(char >= 'A' && char <= 'Z') && !(char >= '0' && char <= '9') && char != '_' && char != '-' {
			return "", fmt.Errorf("%w: proposal_group_id is invalid", ErrInvalidInput)
		}
	}
	return value, nil
}

// normalizeDraftReplyPayload creates a new payload map with the exact plain
// text the executor may send. Keeping this validation at the service boundary
// protects every proposer (HTTP, model consumer, or a future worker), rather
// than relying on a particular gateway client to sanitize content.
func normalizeAITextPayload(kind string, payload map[string]any) (map[string]any, error) {
	body, ok := payload["body_text"].(string)
	if !ok {
		return nil, fmt.Errorf("%w: %s body_text is required", ErrInvalidInput, kind)
	}
	body = strings.TrimSpace(body)
	if body == "" {
		return nil, fmt.Errorf("%w: %s body_text is required", ErrInvalidInput, kind)
	}
	if utf8.RuneCountInString(body) > maxDraftReplyRunes {
		return nil, fmt.Errorf("%w: %s body_text exceeds %d characters", ErrInvalidInput, kind, maxDraftReplyRunes)
	}
	return map[string]any{"body_text": body}, nil
}

// CreateAIAction validates and persists a model-proposed action into the HITL
// review queue (status 'suggested'). It is the generic propose path shared by
// the POST /ai-actions route and the model-proposed consumer. The kind must be
// in the allowlist; org_id and conversation_id are required.
func (s *Service) CreateAIAction(ctx context.Context, input CreateAIActionInput) (*AIAction, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	groupID, err := normalizeProposalGroupID(input.ProposalGroupID)
	if err != nil {
		return nil, err
	}
	input.Kind = strings.TrimSpace(input.Kind)
	input.CreatedBy = strings.TrimSpace(input.CreatedBy)
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if input.Kind == "" {
		return nil, fmt.Errorf("%w: kind is required", ErrInvalidInput)
	}
	if !allowedAIActionKinds[input.Kind] {
		return nil, fmt.Errorf("%w: unsupported action kind %q", ErrInvalidInput, input.Kind)
	}
	if s.aiProposalPolicy != nil {
		if err := s.aiProposalPolicy.AllowAIProposal(ctx, input.OrgID); err != nil {
			return nil, err
		}
	}
	if _, err := s.repository.GetConversation(ctx, input.OrgID, input.ConversationID); err != nil {
		return nil, err
	}
	payload, err := normalizeAIActionPayload(ctx, input.Kind, input.OrgID, input.ConversationID, input.Payload, s.repository)
	if err != nil {
		return nil, err
	}
	return s.repository.CreateAIAction(ctx, CreateAIActionInput{
		OrgID:           input.OrgID,
		ConversationID:  input.ConversationID,
		ProposalGroupID: groupID,
		Kind:            input.Kind,
		Payload:         payload,
		CreatedBy:       input.CreatedBy,
	})
}

func normalizeAIActionPayload(ctx context.Context, kind, orgID, conversationID string, payload map[string]any, repository Repository) (map[string]any, error) {
	if kind == "draft.reply" || kind == "internal.note" {
		return normalizeAITextPayload(kind, payload)
	}
	if kind == "incident.create" {
		return normalizeIncidentCreatePayload(ctx, orgID, conversationID, payload, repository)
	}
	if kind == "problem.create" {
		return normalizeProblemCreatePayload(payload)
	}
	// ticket.update permits taxonomy, urgency, paired team routing, and only the
	// active operational states that a human can inspect and approve. Terminal
	// state, snooze, and ownership remain outside this AI proposal contract.
	ticketID, _ := payload["ticket_id"].(string)
	ticketID = strings.TrimSpace(ticketID)
	if ticketID == "" {
		return nil, fmt.Errorf("%w: ticket.update ticket_id is required", ErrInvalidInput)
	}
	ticket, err := repository.GetTicket(ctx, orgID, ticketID)
	if err != nil {
		return nil, err
	}
	if ticket.ConversationID != conversationID {
		return nil, ErrNotFound
	}
	fields, ok := payload["suggested_fields"].(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%w: ticket.update suggested_fields are required", ErrInvalidInput)
	}
	allowed := map[string]bool{"category": true, "intent": true, "work_type": true, "priority": true, "severity": true, "status": true, "team_id": true, "team_name": true}
	normalized := map[string]any{}
	for key, value := range fields {
		if !allowed[key] {
			return nil, fmt.Errorf("%w: ticket.update field %q is unsupported", ErrInvalidInput, key)
		}
		text, ok := value.(string)
		text = strings.TrimSpace(text)
		maxLength := 120
		if key == "team_name" {
			maxLength = 160
		}
		if !ok || text == "" || utf8.RuneCountInString(text) > maxLength {
			return nil, fmt.Errorf("%w: ticket.update field %q is invalid", ErrInvalidInput, key)
		}
		if key == "category" && utf8.RuneCountInString(text) > 80 {
			return nil, fmt.Errorf("%w: ticket.update category exceeds 80 characters", ErrInvalidInput)
		}
		if key == "priority" && text != "low" && text != "normal" && text != "high" && text != "urgent" {
			return nil, fmt.Errorf("%w: ticket.update priority is invalid", ErrInvalidInput)
		}
		if key == "severity" && text != "low" && text != "medium" && text != "high" && text != "critical" {
			return nil, fmt.Errorf("%w: ticket.update severity is invalid", ErrInvalidInput)
		}
		if key == "status" {
			text = normalizeTicketStatus(text)
			if !isAIReviewableTicketStatus(text) {
				return nil, fmt.Errorf("%w: ticket.update status is not reviewable", ErrInvalidInput)
			}
		}
		if key == "work_type" {
			text = strings.ToLower(text)
			if !isTicketWorkType(text) {
				return nil, fmt.Errorf("%w: ticket.update work_type is invalid", ErrInvalidInput)
			}
		}
		normalized[key] = text
	}
	_, hasTeamID := normalized["team_id"]
	_, hasTeamName := normalized["team_name"]
	if hasTeamID != hasTeamName {
		return nil, fmt.Errorf("%w: ticket.update routing needs both team_id and team_name", ErrInvalidInput)
	}
	if len(normalized) == 0 {
		return nil, fmt.Errorf("%w: ticket.update needs at least one field", ErrInvalidInput)
	}
	confidence, ok := payload["confidence"].(float64)
	if !ok || confidence < 0 || confidence > 1 {
		return nil, fmt.Errorf("%w: ticket.update confidence must be between 0 and 1", ErrInvalidInput)
	}
	reason, ok := payload["reason"].(string)
	reason = strings.TrimSpace(reason)
	if !ok || reason == "" || utf8.RuneCountInString(reason) > 500 {
		return nil, fmt.Errorf("%w: ticket.update reason is invalid", ErrInvalidInput)
	}
	evidence, ok := payload["evidence_message_ids"].([]any)
	if !ok || len(evidence) > 25 {
		return nil, fmt.Errorf("%w: ticket.update evidence_message_ids are invalid", ErrInvalidInput)
	}
	normalizedEvidence := make([]string, 0, len(evidence))
	for _, value := range evidence {
		id, ok := value.(string)
		id = strings.TrimSpace(id)
		if !ok || id == "" || utf8.RuneCountInString(id) > 120 {
			return nil, fmt.Errorf("%w: ticket.update evidence message identifier is invalid", ErrInvalidInput)
		}
		normalizedEvidence = append(normalizedEvidence, id)
	}
	return map[string]any{
		"ticket_id": ticketID, "confidence": confidence, "reason": reason,
		"evidence_message_ids": normalizedEvidence, "suggested_fields": normalized,
	}, nil
}

func normalizeIncidentCreatePayload(ctx context.Context, orgID, conversationID string, payload map[string]any, repository Repository) (map[string]any, error) {
	ticketID, _ := payload["ticket_id"].(string)
	title, _ := payload["title"].(string)
	severity, _ := payload["severity"].(string)
	impact, _ := payload["customer_impact"].(string)
	reason, _ := payload["reason"].(string)
	ticketID, title = strings.TrimSpace(ticketID), strings.TrimSpace(title)
	severity = strings.ToLower(strings.TrimSpace(severity))
	impact, reason = strings.TrimSpace(impact), strings.TrimSpace(reason)
	if ticketID == "" || title == "" || utf8.RuneCountInString(title) > 300 || !isTicketSeverity(severity) || utf8.RuneCountInString(impact) > 2_000 {
		return nil, fmt.Errorf("%w: incident.create payload is invalid", ErrInvalidInput)
	}
	confidence, ok := payload["confidence"].(float64)
	if !ok || confidence < 0 || confidence > 1 || reason == "" || utf8.RuneCountInString(reason) > 500 {
		return nil, fmt.Errorf("%w: incident.create confidence or reason is invalid", ErrInvalidInput)
	}
	evidence, ok := payload["evidence_message_ids"].([]any)
	if !ok || len(evidence) > 25 {
		return nil, fmt.Errorf("%w: incident.create evidence_message_ids are invalid", ErrInvalidInput)
	}
	normalizedEvidence := make([]string, 0, len(evidence))
	for _, value := range evidence {
		messageID, ok := value.(string)
		messageID = strings.TrimSpace(messageID)
		if !ok || messageID == "" || utf8.RuneCountInString(messageID) > 120 {
			return nil, fmt.Errorf("%w: incident.create evidence message identifier is invalid", ErrInvalidInput)
		}
		normalizedEvidence = append(normalizedEvidence, messageID)
	}
	ticket, err := repository.GetTicket(ctx, orgID, ticketID)
	if err != nil {
		return nil, err
	}
	if ticket.ConversationID != conversationID {
		return nil, ErrNotFound
	}
	return map[string]any{
		"ticket_id": ticketID, "title": title, "severity": severity, "customer_impact": impact,
		"confidence": confidence, "reason": reason, "evidence_message_ids": normalizedEvidence,
	}, nil
}

// normalizeProblemCreatePayload admits a reviewable root-cause candidate, not
// a lifecycle instruction. It has no Incident/Ticket identifier by design:
// automatic linking could accidentally assert a causal relationship. The
// resulting Problem starts investigating under the approving reviewer only.
func normalizeProblemCreatePayload(payload map[string]any) (map[string]any, error) {
	title, _ := payload["title"].(string)
	summary, _ := payload["summary"].(string)
	rootCause, _ := payload["root_cause"].(string)
	reason, _ := payload["reason"].(string)
	title, summary, rootCause, reason = strings.TrimSpace(title), strings.TrimSpace(summary), strings.TrimSpace(rootCause), strings.TrimSpace(reason)
	if title == "" || utf8.RuneCountInString(title) > 300 || summary == "" || utf8.RuneCountInString(summary) > 2_000 || utf8.RuneCountInString(rootCause) > 2_000 {
		return nil, fmt.Errorf("%w: problem.create payload is invalid", ErrInvalidInput)
	}
	confidence, ok := payload["confidence"].(float64)
	if !ok || confidence < 0 || confidence > 1 || reason == "" || utf8.RuneCountInString(reason) > 500 {
		return nil, fmt.Errorf("%w: problem.create confidence or reason is invalid", ErrInvalidInput)
	}
	evidence, ok := payload["evidence_message_ids"].([]any)
	if !ok || len(evidence) > 25 {
		return nil, fmt.Errorf("%w: problem.create evidence_message_ids are invalid", ErrInvalidInput)
	}
	normalizedEvidence := make([]string, 0, len(evidence))
	for _, value := range evidence {
		messageID, ok := value.(string)
		messageID = strings.TrimSpace(messageID)
		if !ok || messageID == "" || utf8.RuneCountInString(messageID) > 120 {
			return nil, fmt.Errorf("%w: problem.create evidence message identifier is invalid", ErrInvalidInput)
		}
		normalizedEvidence = append(normalizedEvidence, messageID)
	}
	return map[string]any{
		"title": title, "summary": summary, "root_cause": rootCause,
		"confidence": confidence, "reason": reason, "evidence_message_ids": normalizedEvidence,
	}, nil
}

func (s *Service) ListAIActions(ctx context.Context, filter AIActionListFilter) ([]AIAction, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Status = strings.TrimSpace(filter.Status)
	filter.ConversationID = strings.TrimSpace(filter.ConversationID)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	// Default to the review queue (suggested actions awaiting a human decision).
	// "all" is the explicit escape hatch to list every status for the org.
	switch strings.ToLower(filter.Status) {
	case "":
		filter.Status = "suggested"
	case "review":
		// Review is the operator-facing union of both pending ledger states.
		// Ticket classifications historically use suggest_ticket, while newer
		// proposal kinds use suggested. Neither is terminal or executable.
		filter.Status = "review"
	case "all":
		filter.Status = ""
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListAIActions(ctx, filter)
}

func (s *Service) ListTickets(ctx context.Context, filter TicketListFilter) ([]Ticket, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Queue = strings.TrimSpace(filter.Queue)
	filter.Status = normalizeTicketStatus(filter.Status)
	filter.WorkType = strings.ToLower(strings.TrimSpace(filter.WorkType))
	filter.Assigned = strings.TrimSpace(filter.Assigned)
	filter.TeamID = strings.TrimSpace(filter.TeamID)
	filter.Label = strings.TrimSpace(filter.Label)
	filter.Priority = normalizeOptionalTicketPriority(filter.Priority)
	filter.Severity = normalizeOptionalTicketSeverity(filter.Severity)
	filter.SLAState = normalizeSLAState(filter.SLAState)
	filter.Query = strings.TrimSpace(filter.Query)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.WorkType != "" && !isTicketWorkType(filter.WorkType) {
		return nil, fmt.Errorf("%w: work_type must be customer_case, internal_work, or incident", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListTickets(ctx, filter)
}

func (s *Service) GetTicket(ctx context.Context, orgID, ticketID string) (*Ticket, error) {
	orgID = strings.TrimSpace(orgID)
	ticketID = strings.TrimSpace(ticketID)
	if orgID == "" || ticketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	return s.repository.GetTicket(ctx, orgID, ticketID)
}

// ListTicketActivity returns the most recent bounded ticket audit history.
// Resolving the ticket first preserves a useful not-found response and ensures
// an empty history never becomes a way to probe another tenant's ticket ID.
func (s *Service) ListTicketActivity(ctx context.Context, orgID, ticketID string, limit int) ([]TicketActivity, error) {
	orgID = strings.TrimSpace(orgID)
	ticketID = strings.TrimSpace(ticketID)
	if orgID == "" || ticketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	if limit < 1 || limit > 50 {
		limit = 20
	}
	if _, err := s.repository.GetTicket(ctx, orgID, ticketID); err != nil {
		return nil, err
	}
	reader, ok := s.repository.(TicketActivityRepository)
	if !ok {
		return nil, fmt.Errorf("ticket activity is unavailable")
	}
	return reader.ListTicketActivity(ctx, orgID, ticketID, limit)
}

// ListConversationActivity returns a bounded timeline for one conversation.
// Resolving the conversation before reading the audit projection prevents an
// empty result from becoming an oracle for cross-tenant conversation IDs.
func (s *Service) ListConversationActivity(ctx context.Context, orgID, conversationID string, limit int) ([]ConversationActivity, error) {
	orgID = strings.TrimSpace(orgID)
	conversationID = strings.TrimSpace(conversationID)
	if orgID == "" || conversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if limit < 1 || limit > 50 {
		limit = 20
	}
	if _, err := s.repository.GetConversation(ctx, orgID, conversationID); err != nil {
		return nil, err
	}
	reader, ok := s.repository.(ConversationActivityRepository)
	if !ok {
		return nil, fmt.Errorf("conversation activity is unavailable")
	}
	return reader.ListConversationActivity(ctx, orgID, conversationID, limit)
}

func (s *Service) CreateTicket(ctx context.Context, input CreateTicketInput) (*Ticket, error) {
	input = normalizeCreateTicketInput(input)
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if !isTicketWorkType(input.WorkType) {
		return nil, fmt.Errorf("%w: work_type must be customer_case, internal_work, or incident", ErrInvalidInput)
	}
	if err := s.canonicalizeCreateTicketTeam(ctx, &input); err != nil {
		return nil, err
	}
	if _, err := s.repository.GetConversation(ctx, input.OrgID, input.ConversationID); err != nil {
		return nil, err
	}
	ticket, err := s.repository.CreateTicket(ctx, input)
	if err != nil {
		return nil, err
	}
	subject := SubjectTicketCreated
	if ticket.Status == "suggested" {
		subject = SubjectTicketSuggested
	}
	s.publishTicket(ctx, subject, ticket, input.ActorUserID)
	return s.evaluateTicketAutomationRules(ctx, "ticket.created", ticket, input.ActorUserID)
}

// CreateTicketOperation is the first owner-plane operation-envelope vertical
// slice. It keeps gateway routing stateless: the owner derives the stable
// operation ID and request digest, persists the ticket/audit/outbox together,
// and returns the exact durable receipt on a retry.
func (s *Service) CreateTicketOperation(ctx context.Context, input CreateTicketInput) (*TicketOperationReceipt, error) {
	input = normalizeCreateTicketInput(input)
	if input.AgentActionAuthorization != nil {
		if err := input.AgentActionAuthorization.Validate(); err != nil {
			return nil, err
		}
		// Normalize a copied authorization value. The owner transaction is the
		// only downstream consumer; callers never receive a mutable pointer.
		authorization := *input.AgentActionAuthorization
		authorization.DecisionRef = strings.TrimSpace(authorization.DecisionRef)
		authorization.SpaceRef = strings.TrimSpace(authorization.SpaceRef)
		authorization.SubjectID = strings.TrimSpace(authorization.SubjectID)
		authorization.RecipientAudienceRef = strings.TrimSpace(authorization.RecipientAudienceRef)
		authorization.RecipientAudienceHash = strings.TrimSpace(authorization.RecipientAudienceHash)
		authorization.PrivacyPolicyRef = strings.TrimSpace(authorization.PrivacyPolicyRef)
		input.AgentActionAuthorization = &authorization
	}
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	if input.IdempotencyKey == "" || len(input.IdempotencyKey) > 200 {
		return nil, fmt.Errorf("%w: idempotency_key is required and must be at most 200 characters", ErrInvalidInput)
	}
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if !isTicketWorkType(input.WorkType) {
		return nil, fmt.Errorf("%w: work_type must be customer_case, internal_work, or incident", ErrInvalidInput)
	}
	if err := s.canonicalizeCreateTicketTeam(ctx, &input); err != nil {
		return nil, err
	}
	if _, err := s.repository.GetConversation(ctx, input.OrgID, input.ConversationID); err != nil {
		return nil, err
	}
	input.ActionID = "tickets.create"
	input.OperationID = TicketOperationID(input.OrgID, input.IdempotencyKey)
	input.RequestSHA256 = ticketOperationRequestSHA256(input)
	receipt, err := s.repository.CreateTicketOperation(ctx, input)
	if err != nil {
		return nil, err
	}
	if receipt == nil || receipt.Ticket == nil || receipt.OperationID != input.OperationID || receipt.Status != "completed" {
		return nil, fmt.Errorf("ticket operation returned an invalid durable receipt")
	}
	// The repository's transaction created the outbox event. Do not publish
	// directly here: a process crash after a direct publish but before durable
	// acknowledgement would create an untraceable duplicate path. The leased
	// outbox dispatcher owns at-least-once broker delivery.
	return receipt, nil
}

// ResolveAgentTicketActionGrant obtains the opaque current grant identifier
// solely to bind a Control owner-effect reservation. It is intentionally not
// an effect authorization: the eventual ticket transaction must resolve the
// exact same grant again under its local lock.
func (s *Service) ResolveAgentTicketActionGrant(ctx context.Context, input CreateTicketInput) (string, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	if input.OrgID == "" || input.ConversationID == "" || input.AgentActionAuthorization == nil {
		return "", fmt.Errorf("%w: agent ticket grant lookup requires owner authorization", ErrInvalidInput)
	}
	authorization := *input.AgentActionAuthorization
	if err := authorization.Validate(); err != nil {
		return "", err
	}
	input.AgentActionAuthorization = &authorization
	grantID, err := s.repository.ResolveAgentTicketActionGrant(ctx, input)
	if err != nil {
		return "", err
	}
	if strings.TrimSpace(grantID) == "" || len(strings.TrimSpace(grantID)) > 200 {
		return "", fmt.Errorf("agent ticket grant lookup returned an invalid grant")
	}
	return strings.TrimSpace(grantID), nil
}

// BeginAgentTicketOperationIntent persists the content-free owner intent
// before any Control reservation is attempted. The private route must fail
// closed when the repository has not adopted owner-effect-reservation-v1.
func (s *Service) BeginAgentTicketOperationIntent(ctx context.Context, input TicketOperationIntentInput) (*TicketOperationReceipt, error) {
	store, ok := s.repository.(AgentTicketOperationIntentStore)
	if !ok {
		return nil, ErrTicketOperationIntentUnavailable
	}
	return store.BeginAgentTicketOperationIntent(ctx, input)
}

// BindAgentTicketOperationReservation records the committed Control receipt
// before the owner effect is attempted. A replay of the same reservation is
// idempotent; a different reservation or commitment is a conflict.
func (s *Service) BindAgentTicketOperationReservation(ctx context.Context, input TicketOperationReservationInput) (*TicketOperationReceipt, error) {
	store, ok := s.repository.(AgentTicketOperationIntentStore)
	if !ok {
		return nil, ErrTicketOperationIntentUnavailable
	}
	return store.BindAgentTicketOperationReservation(ctx, input)
}

func (s *Service) MarkAgentTicketOperationUnknown(ctx context.Context, input TicketOperationOutcomeInput) error {
	store, ok := s.repository.(AgentTicketOperationIntentStore)
	if !ok {
		return ErrTicketOperationIntentUnavailable
	}
	return store.MarkAgentTicketOperationUnknown(ctx, input)
}

func (s *Service) MarkAgentTicketOperationCancelled(ctx context.Context, input TicketOperationOutcomeInput) error {
	store, ok := s.repository.(AgentTicketOperationIntentStore)
	if !ok {
		return ErrTicketOperationIntentUnavailable
	}
	return store.MarkAgentTicketOperationCancelled(ctx, input)
}

// CreateAgentTicketActionGrant persists Conversation Core's own exact
// owner-resource permission for the currently disabled Model ticket action.
// Control supplies a short-lived, current Space/policy decision at HTTP
// ingress; this service receives only its verified, non-secret claims and
// keeps the owner write and durable receipt transactional.
func (s *Service) CreateAgentTicketActionGrant(ctx context.Context, input CreateAgentTicketActionGrantInput) (*AgentTicketActionGrantReceipt, error) {
	input = normalizeCreateAgentTicketActionGrantInput(input)
	if err := validateCreateAgentTicketActionGrantInput(input); err != nil {
		return nil, err
	}
	receipt, err := s.repository.CreateAgentTicketActionGrant(ctx, input)
	if err != nil {
		return nil, err
	}
	if receipt == nil || receipt.Grant == nil || receipt.Grant.ID == "" || receipt.Status != "created" {
		return nil, fmt.Errorf("agent ticket grant returned an invalid durable receipt")
	}
	return receipt, nil
}

// RevokeAgentTicketActionGrant removes the owner-resource intersection before
// returning a receipt. The repository locks the grant and a Model ticket
// effect takes a compatible lock in its own transaction, so revoke cannot race
// an unchecked effect into existence.
func (s *Service) RevokeAgentTicketActionGrant(ctx context.Context, input RevokeAgentTicketActionGrantInput) (*AgentTicketActionGrantReceipt, error) {
	input = normalizeRevokeAgentTicketActionGrantInput(input)
	if err := validateRevokeAgentTicketActionGrantInput(input); err != nil {
		return nil, err
	}
	receipt, err := s.repository.RevokeAgentTicketActionGrant(ctx, input)
	if err != nil {
		return nil, err
	}
	if receipt == nil || receipt.Grant == nil || receipt.Grant.ID != input.GrantID || receipt.Status != "revoked" {
		return nil, fmt.Errorf("agent ticket grant revoke returned an invalid durable receipt")
	}
	return receipt, nil
}

func normalizeCreateAgentTicketActionGrantInput(input CreateAgentTicketActionGrantInput) CreateAgentTicketActionGrantInput {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.ActionID = strings.TrimSpace(input.ActionID)
	input.SpaceRef = strings.TrimSpace(input.SpaceRef)
	input.SubjectID = strings.TrimSpace(input.SubjectID)
	input.RecipientAudienceRef = strings.TrimSpace(input.RecipientAudienceRef)
	input.RecipientAudienceHash = strings.TrimSpace(input.RecipientAudienceHash)
	input.PrivacyPolicyRef = strings.TrimSpace(input.PrivacyPolicyRef)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	input.RequestSHA256 = strings.TrimSpace(input.RequestSHA256)
	input.ControlDecisionRef = strings.TrimSpace(input.ControlDecisionRef)
	return input
}

func normalizeRevokeAgentTicketActionGrantInput(input RevokeAgentTicketActionGrantInput) RevokeAgentTicketActionGrantInput {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.GrantID = strings.TrimSpace(input.GrantID)
	input.SpaceRef = strings.TrimSpace(input.SpaceRef)
	input.SubjectID = strings.TrimSpace(input.SubjectID)
	input.RevokedByUserID = strings.TrimSpace(input.RevokedByUserID)
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	input.RequestSHA256 = strings.TrimSpace(input.RequestSHA256)
	input.ControlDecisionRef = strings.TrimSpace(input.ControlDecisionRef)
	return input
}

func validateCreateAgentTicketActionGrantInput(input CreateAgentTicketActionGrantInput) error {
	for name, value := range map[string]string{
		"org_id": input.OrgID, "conversation_id": input.ConversationID, "action_id": input.ActionID,
		"space_ref": input.SpaceRef, "subject_id": input.SubjectID,
		"recipient_audience_ref": input.RecipientAudienceRef, "recipient_audience_hash": input.RecipientAudienceHash,
		"privacy_policy_ref": input.PrivacyPolicyRef, "created_by_user_id": input.CreatedByUserID,
		"idempotency_key": input.IdempotencyKey, "request_sha256": input.RequestSHA256,
		"control_decision_ref": input.ControlDecisionRef,
	} {
		if value == "" || len(value) > 200 {
			return fmt.Errorf("%w: agent ticket grant %s is invalid", ErrInvalidInput, name)
		}
	}
	if input.ActionID != "tickets.create" || input.RecipientAudienceRevision <= 0 || input.AuthorityRevision <= 0 || !validTicketGrantSHA256(input.RequestSHA256) {
		return fmt.Errorf("%w: agent ticket grant authority is invalid", ErrInvalidInput)
	}
	return nil
}

func validateRevokeAgentTicketActionGrantInput(input RevokeAgentTicketActionGrantInput) error {
	for name, value := range map[string]string{
		"org_id": input.OrgID, "conversation_id": input.ConversationID, "grant_id": input.GrantID,
		"space_ref": input.SpaceRef, "subject_id": input.SubjectID,
		"revoked_by_user_id": input.RevokedByUserID, "idempotency_key": input.IdempotencyKey,
		"request_sha256": input.RequestSHA256, "control_decision_ref": input.ControlDecisionRef,
	} {
		if value == "" || len(value) > 200 {
			return fmt.Errorf("%w: agent ticket grant revoke %s is invalid", ErrInvalidInput, name)
		}
	}
	if !validTicketGrantSHA256(input.RequestSHA256) {
		return fmt.Errorf("%w: agent ticket grant revoke request is invalid", ErrInvalidInput)
	}
	return nil
}

func validTicketGrantSHA256(value string) bool {
	value = strings.TrimSpace(value)
	if !strings.HasPrefix(value, "sha256:") || len(value) != len("sha256:")+64 {
		return false
	}
	_, err := hex.DecodeString(value[len("sha256:"):])
	return err == nil
}

// GetTicketOperation reconciles an ambiguous request without attempting the
// effect again. The repository binds the lookup to the same authenticated
// actor that created the operation, so an organization peer cannot probe
// another user's idempotency keys or receipts.
func (s *Service) GetTicketOperation(ctx context.Context, orgID, actorUserID, idempotencyKey string) (*TicketOperationReceipt, error) {
	orgID = strings.TrimSpace(orgID)
	actorUserID = strings.TrimSpace(actorUserID)
	idempotencyKey = strings.TrimSpace(idempotencyKey)
	if orgID == "" || actorUserID == "" || idempotencyKey == "" || len(idempotencyKey) > 200 {
		return nil, fmt.Errorf("%w: operation lookup requires org, actor, and bounded idempotency_key", ErrInvalidInput)
	}
	receipt, err := s.repository.GetTicketOperation(ctx, orgID, actorUserID, idempotencyKey)
	if err != nil {
		return nil, err
	}
	if receipt == nil || receipt.OperationID == "" || receipt.Status == "" {
		return nil, fmt.Errorf("ticket operation lookup returned an invalid durable receipt")
	}
	if receipt.Status == "completed" && (receipt.AuditEventID == "" || receipt.Ticket == nil) {
		return nil, fmt.Errorf("completed ticket operation lookup returned an invalid durable receipt")
	}
	if receipt.Status != "completed" && receipt.Ticket != nil {
		return nil, fmt.Errorf("non-completed ticket operation lookup returned ticket content")
	}
	return receipt, nil
}

func ticketOperationRequestSHA256(input CreateTicketInput) string {
	// encoding/json orders map keys, so this records a stable, content-minimized
	// semantic request binding without retaining the raw generic action body.
	payload, _ := json.Marshal(map[string]any{
		"action_id": input.ActionID, "actor_user_id": input.ActorUserID,
		"conversation_id": input.ConversationID, "status": input.Status,
		"work_type": input.WorkType, "priority": input.Priority, "severity": input.Severity,
		"category": input.Category, "intent": input.Intent, "assignee_user_id": input.AssigneeUserID,
		"assignee_name": input.AssigneeName, "team_id": input.TeamID, "team_name": input.TeamName,
		"due_at": input.DueAt, "source": input.Source, "labels": input.Labels,
		"ai_confidence": input.AIConfidence, "ai_reason": input.AIReason,
		"created_by": input.CreatedBy, "waiting_since": input.WaitingSince,
		"last_customer_reply_at": input.LastCustomerReplyAt,
		"first_response_at":      input.FirstResponseAt, "resolved_at": input.ResolvedAt,
		"snoozed_until": input.SnoozedUntil, "sla_policy_id": input.SLAPolicyID,
		"escalation_at": input.EscalationAt,
	})
	digest := sha256.Sum256(payload)
	return hex.EncodeToString(digest[:])
}

// TicketOperationRequestSHA256 exposes the same canonical, content-minimized
// request binding used by the owner transaction so a pre-reservation intent
// can be created with the exact digest that finalization will verify.
func TicketOperationRequestSHA256(input CreateTicketInput) string {
	return ticketOperationRequestSHA256(normalizeCreateTicketInput(input))
}

func (s *Service) UpdateTicket(ctx context.Context, input UpdateTicketInput) (*Ticket, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	normalizeStringPtr(input.Status, normalizeTicketStatus)
	trimStringPtr(input.WorkType)
	normalizeStringPtr(input.Priority, normalizeTicketPriority)
	normalizeStringPtr(input.Severity, normalizeTicketSeverity)
	trimStringPtr(input.Category)
	trimStringPtr(input.Intent)
	trimStringPtr(input.AssigneeUserID)
	trimStringPtr(input.AssigneeName)
	trimStringPtr(input.TeamID)
	trimStringPtr(input.TeamName)
	trimStringPtr(input.Source)
	trimStringPtr(input.AIReason)
	trimStringPtr(input.SLAPolicyID)
	if input.WorkType != nil {
		*input.WorkType = strings.ToLower(*input.WorkType)
		if !isTicketWorkType(*input.WorkType) {
			return nil, fmt.Errorf("%w: work_type must be customer_case, internal_work, or incident", ErrInvalidInput)
		}
	}
	if err := s.canonicalizeUpdateTicketTeam(ctx, &input); err != nil {
		return nil, err
	}
	if input.Labels != nil {
		labels := normalizeLabels(*input.Labels)
		input.Labels = &labels
	}
	if input.Status != nil {
		switch *input.Status {
		case "resolved", "closed":
			if input.ResolvedAt == nil {
				resolvedAt := s.now().UTC()
				input.ResolvedAt = &resolvedAt
			}
		case "waiting_customer", "waiting_team":
			if input.WaitingSince == nil {
				waitingSince := s.now().UTC()
				input.WaitingSince = &waitingSince
			}
		}
	}
	// A resolution event is a state transition, not a record of every PATCH
	// whose desired value happens to be resolved. Reading the canonical ticket
	// first prevents duplicate downstream quality metrics and future customer
	// surveys when a UI retry or later metadata edit repeats terminal status.
	currentTicket, err := s.repository.GetTicket(ctx, input.OrgID, input.TicketID)
	if err != nil {
		return nil, err
	}
	wasTerminal := currentTicket.Status == "resolved" || currentTicket.Status == "closed"
	ticket, err := s.repository.UpdateTicket(ctx, input)
	if err != nil {
		return nil, err
	}
	subject := SubjectTicketUpdated
	if input.AssigneeUserID != nil || input.AssigneeName != nil || input.TeamID != nil || input.TeamName != nil {
		subject = SubjectTicketAssigned
	}
	if input.Status != nil && !wasTerminal && (*input.Status == "resolved" || *input.Status == "closed") {
		subject = SubjectTicketResolved
	}
	s.publishTicket(ctx, subject, ticket, input.ActorUserID)
	return s.evaluateTicketAutomationRules(ctx, "ticket.updated", ticket, input.ActorUserID)
}

// canonicalizeCreateTicketTeam ensures a durable Ticket never accepts a
// provider group label as its routing authority. Names are derived from the
// organization-scoped TicketTeam directory instead of trusting the caller.
func (s *Service) canonicalizeCreateTicketTeam(ctx context.Context, input *CreateTicketInput) error {
	if input == nil {
		return fmt.Errorf("%w: ticket input is required", ErrInvalidInput)
	}
	if input.TeamID == "" {
		if input.TeamName != "" {
			return fmt.Errorf("%w: team_name requires a canonical team_id", ErrInvalidInput)
		}
		return nil
	}
	team, err := s.repository.GetTicketTeam(ctx, input.OrgID, input.TeamID)
	if err != nil {
		return err
	}
	if !team.Active {
		return fmt.Errorf("%w: ticket team is inactive", ErrInvalidInput)
	}
	input.TeamName = team.Name
	return nil
}

func (s *Service) canonicalizeUpdateTicketTeam(ctx context.Context, input *UpdateTicketInput) error {
	if input == nil {
		return fmt.Errorf("%w: ticket input is required", ErrInvalidInput)
	}
	if input.TeamID == nil {
		if input.TeamName != nil {
			return fmt.Errorf("%w: team_name requires a canonical team_id", ErrInvalidInput)
		}
		return nil
	}
	if *input.TeamID == "" {
		empty := ""
		input.TeamName = &empty
		return nil
	}
	team, err := s.repository.GetTicketTeam(ctx, input.OrgID, *input.TeamID)
	if err != nil {
		return err
	}
	if !team.Active {
		return fmt.Errorf("%w: ticket team is inactive", ErrInvalidInput)
	}
	name := team.Name
	input.TeamName = &name
	return nil
}

func (s *Service) LinkTicketResource(ctx context.Context, input LinkTicketResourceInput) (*TicketLinkedResource, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ResourceKind = strings.TrimSpace(input.ResourceKind)
	input.LinkType = normalizeLinkType(input.LinkType)
	input.ResourceID = strings.TrimSpace(input.ResourceID)
	input.ResourceURL = strings.TrimSpace(input.ResourceURL)
	input.Label = strings.TrimSpace(input.Label)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	if input.OrgID == "" || input.TicketID == "" || input.ResourceKind == "" {
		return nil, fmt.Errorf("%w: org_id, ticket_id, and resource_kind are required", ErrInvalidInput)
	}
	// A conversation source is a first-class support handoff: it must resolve
	// inside the same organization before an existing ticket can reference it.
	// Do not accept an arbitrary conversation identifier as generic metadata.
	if input.ResourceKind == "conversation_source" {
		if input.ResourceID == "" {
			return nil, fmt.Errorf("%w: conversation links require a conversation identifier", ErrInvalidInput)
		}
		source, err := s.repository.GetConversation(ctx, input.OrgID, input.ResourceID)
		if err != nil {
			return nil, err
		}
		if source.OrgID != input.OrgID {
			return nil, ErrNotFound
		}
		target, err := s.repository.GetTicket(ctx, input.OrgID, input.TicketID)
		if err != nil {
			return nil, err
		}
		if target.OrgID != input.OrgID {
			return nil, ErrNotFound
		}
		if target.ConversationID == source.ID {
			return nil, fmt.Errorf("%w: a ticket already owns that conversation as its primary source", ErrInvalidInput)
		}
		if existing, lookupErr := s.repository.GetTicketByConversation(ctx, input.OrgID, source.ID); lookupErr == nil {
			return nil, fmt.Errorf("%w: conversation is already attached to ticket %s", ErrConflict, existing.ID)
		} else if !errors.Is(lookupErr, ErrNotFound) {
			return nil, lookupErr
		}
	}
	// A ticket-to-ticket link is a durable work dependency, not an arbitrary
	// string reference. Resolve the target through the tenant-scoped repository
	// before persisting the link so one org cannot encode a foreign ticket id,
	// and reject self-links which make parent/child/related semantics useless.
	if input.ResourceKind == "ticket" {
		if input.ResourceID == "" || input.ResourceID == input.TicketID {
			return nil, fmt.Errorf("%w: ticket links require a distinct target ticket", ErrInvalidInput)
		}
		target, err := s.repository.GetTicket(ctx, input.OrgID, input.ResourceID)
		if err != nil {
			return nil, err
		}
		// Repositories must scope by org; retain this defensive check at the
		// service boundary so a faulty implementation cannot create a cross-org
		// work dependency.
		if target.OrgID != input.OrgID {
			return nil, ErrNotFound
		}
	}
	link, err := s.repository.LinkTicketResource(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTicketLinked, &ConversationDetail{ConversationSummary: ConversationSummary{
		ID:    link.ConversationID,
		OrgID: link.OrgID,
	}}, nil, input.CreatedByUserID, map[string]any{"link": link})
	return link, nil
}

func (s *Service) incidentProblemRepository() (IncidentProblemRepository, error) {
	repository, ok := s.repository.(IncidentProblemRepository)
	if !ok {
		return nil, fmt.Errorf("%w: incident and problem management is unavailable", ErrDeliveryUnavailable)
	}
	return repository, nil
}

func (s *Service) ListIncidents(ctx context.Context, orgID string) ([]Incident, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	return repository.ListIncidents(ctx, orgID)
}

func (s *Service) GetIncident(ctx context.Context, orgID, incidentID string) (*Incident, error) {
	orgID = strings.TrimSpace(orgID)
	incidentID = strings.TrimSpace(incidentID)
	if orgID == "" || incidentID == "" {
		return nil, fmt.Errorf("%w: org_id and incident_id are required", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	return repository.GetIncident(ctx, orgID, incidentID)
}

func (s *Service) CreateIncident(ctx context.Context, input CreateIncidentInput) (*Incident, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Title = strings.TrimSpace(input.Title)
	rawSeverity := strings.ToLower(strings.TrimSpace(input.Severity))
	if rawSeverity != "" && !isTicketSeverity(rawSeverity) {
		return nil, fmt.Errorf("%w: incident severity is invalid", ErrInvalidInput)
	}
	input.Status = normalizeIncidentStatus(input.Status)
	input.Severity = normalizeTicketSeverity(input.Severity)
	input.OwnerUserID = strings.TrimSpace(input.OwnerUserID)
	input.OwnerName = strings.TrimSpace(input.OwnerName)
	input.CustomerImpact = strings.TrimSpace(input.CustomerImpact)
	input.ProblemID = strings.TrimSpace(input.ProblemID)
	input.DeclaredByUserID = strings.TrimSpace(input.DeclaredByUserID)
	if input.OrgID == "" || input.Title == "" || len(input.Title) > 300 {
		return nil, fmt.Errorf("%w: org_id and a title of at most 300 characters are required", ErrInvalidInput)
	}
	if !isIncidentStatus(input.Status) || !isTicketSeverity(input.Severity) {
		return nil, fmt.Errorf("%w: incident status or severity is invalid", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	if input.ProblemID != "" {
		if _, err := repository.GetProblem(ctx, input.OrgID, input.ProblemID); err != nil {
			return nil, err
		}
	}
	return repository.CreateIncident(ctx, input)
}

// CreateIncidentForApprovedAction is the only AI executor path for declaring
// an Incident. The repository commits the Incident, affected-ticket link, and
// operational audits in one transaction keyed by AIActionID; an at-least-once
// review event therefore cannot create duplicate operational work.
func (s *Service) CreateIncidentForApprovedAction(ctx context.Context, input ApprovedIncidentCreateInput) (*Incident, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.AIActionID = strings.TrimSpace(input.AIActionID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.Title = strings.TrimSpace(input.Title)
	input.Severity = strings.ToLower(strings.TrimSpace(input.Severity))
	input.CustomerImpact = strings.TrimSpace(input.CustomerImpact)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.ConversationID == "" || input.AIActionID == "" || input.TicketID == "" || input.Title == "" || len(input.Title) > 300 || !isTicketSeverity(input.Severity) {
		return nil, fmt.Errorf("%w: approved incident proposal is invalid", ErrInvalidInput)
	}
	ticket, err := s.repository.GetTicket(ctx, input.OrgID, input.TicketID)
	if err != nil {
		return nil, err
	}
	if ticket.ConversationID != input.ConversationID {
		return nil, ErrNotFound
	}
	repository, ok := s.repository.(ApprovedIncidentActionRepository)
	if !ok {
		return nil, fmt.Errorf("%w: approved incident execution is unavailable", ErrDeliveryUnavailable)
	}
	return repository.CreateIncidentForApprovedAction(ctx, input)
}

// CreateProblemForApprovedAction is the only executor path for a model's
// root-cause candidate. The action had already been scoped to an existing
// conversation when it entered the ledger; this method keeps the final effect
// idempotent by action ID and deliberately creates no relationship or state
// transition outside the new Problem record.
func (s *Service) CreateProblemForApprovedAction(ctx context.Context, input ApprovedProblemCreateInput) (*Problem, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.AIActionID = strings.TrimSpace(input.AIActionID)
	input.Title = strings.TrimSpace(input.Title)
	input.Summary = strings.TrimSpace(input.Summary)
	input.RootCause = strings.TrimSpace(input.RootCause)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.ConversationID == "" || input.AIActionID == "" || input.Title == "" || utf8.RuneCountInString(input.Title) > 300 || input.Summary == "" || utf8.RuneCountInString(input.Summary) > 2_000 || utf8.RuneCountInString(input.RootCause) > 2_000 {
		return nil, fmt.Errorf("%w: approved problem proposal is invalid", ErrInvalidInput)
	}
	if _, err := s.repository.GetConversation(ctx, input.OrgID, input.ConversationID); err != nil {
		return nil, err
	}
	repository, ok := s.repository.(ApprovedProblemActionRepository)
	if !ok {
		return nil, fmt.Errorf("%w: approved problem execution is unavailable", ErrDeliveryUnavailable)
	}
	return repository.CreateProblemForApprovedAction(ctx, input)
}

func (s *Service) UpdateIncident(ctx context.Context, input UpdateIncidentInput) (*Incident, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.IncidentID = strings.TrimSpace(input.IncidentID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Title)
	normalizeStringPtr(input.Status, normalizeIncidentStatus)
	if input.Severity != nil && !isTicketSeverity(*input.Severity) {
		return nil, fmt.Errorf("%w: incident severity is invalid", ErrInvalidInput)
	}
	normalizeStringPtr(input.Severity, normalizeTicketSeverity)
	trimStringPtr(input.OwnerUserID)
	trimStringPtr(input.OwnerName)
	trimStringPtr(input.CustomerImpact)
	trimStringPtr(input.ProblemID)
	if input.OrgID == "" || input.IncidentID == "" {
		return nil, fmt.Errorf("%w: org_id and incident_id are required", ErrInvalidInput)
	}
	if input.Title != nil && (*input.Title == "" || len(*input.Title) > 300) {
		return nil, fmt.Errorf("%w: title must be 1 to 300 characters", ErrInvalidInput)
	}
	if input.Status != nil && !isIncidentStatus(*input.Status) {
		return nil, fmt.Errorf("%w: incident status is invalid", ErrInvalidInput)
	}
	if input.Severity != nil && !isTicketSeverity(*input.Severity) {
		return nil, fmt.Errorf("%w: incident severity is invalid", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	if input.ProblemID != nil && *input.ProblemID != "" {
		if _, err := repository.GetProblem(ctx, input.OrgID, *input.ProblemID); err != nil {
			return nil, err
		}
	}
	return repository.UpdateIncident(ctx, input)
}

func (s *Service) LinkIncidentTicket(ctx context.Context, input LinkIncidentTicketInput) (*IncidentTicketLink, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.IncidentID = strings.TrimSpace(input.IncidentID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.Relationship = normalizeIncidentTicketRelationship(input.Relationship)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	if input.OrgID == "" || input.IncidentID == "" || input.TicketID == "" || !isIncidentTicketRelationship(input.Relationship) {
		return nil, fmt.Errorf("%w: org_id, incident_id, ticket_id, and a valid relationship are required", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	if _, err := repository.GetIncident(ctx, input.OrgID, input.IncidentID); err != nil {
		return nil, err
	}
	if _, err := s.repository.GetTicket(ctx, input.OrgID, input.TicketID); err != nil {
		return nil, err
	}
	return repository.LinkIncidentTicket(ctx, input)
}

func (s *Service) ListProblems(ctx context.Context, orgID string) ([]Problem, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	return repository.ListProblems(ctx, orgID)
}

func (s *Service) GetProblem(ctx context.Context, orgID, problemID string) (*Problem, error) {
	orgID = strings.TrimSpace(orgID)
	problemID = strings.TrimSpace(problemID)
	if orgID == "" || problemID == "" {
		return nil, fmt.Errorf("%w: org_id and problem_id are required", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	return repository.GetProblem(ctx, orgID, problemID)
}

func (s *Service) CreateProblem(ctx context.Context, input CreateProblemInput) (*Problem, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Title = strings.TrimSpace(input.Title)
	input.Status = normalizeProblemStatus(input.Status)
	input.OwnerUserID = strings.TrimSpace(input.OwnerUserID)
	input.OwnerName = strings.TrimSpace(input.OwnerName)
	input.Summary = strings.TrimSpace(input.Summary)
	input.RootCause = strings.TrimSpace(input.RootCause)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	if input.OrgID == "" || input.Title == "" || len(input.Title) > 300 || !isProblemStatus(input.Status) {
		return nil, fmt.Errorf("%w: org_id, valid status, and a title of at most 300 characters are required", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	return repository.CreateProblem(ctx, input)
}

func (s *Service) UpdateProblem(ctx context.Context, input UpdateProblemInput) (*Problem, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ProblemID = strings.TrimSpace(input.ProblemID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Title)
	normalizeStringPtr(input.Status, normalizeProblemStatus)
	trimStringPtr(input.OwnerUserID)
	trimStringPtr(input.OwnerName)
	trimStringPtr(input.Summary)
	trimStringPtr(input.RootCause)
	if input.OrgID == "" || input.ProblemID == "" {
		return nil, fmt.Errorf("%w: org_id and problem_id are required", ErrInvalidInput)
	}
	if input.Title != nil && (*input.Title == "" || len(*input.Title) > 300) {
		return nil, fmt.Errorf("%w: title must be 1 to 300 characters", ErrInvalidInput)
	}
	if input.Status != nil && !isProblemStatus(*input.Status) {
		return nil, fmt.Errorf("%w: problem status is invalid", ErrInvalidInput)
	}
	repository, err := s.incidentProblemRepository()
	if err != nil {
		return nil, err
	}
	return repository.UpdateProblem(ctx, input)
}

func (s *Service) RecordTicketClassification(ctx context.Context, input TicketClassificationInput) (*TicketClassification, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.Reason = strings.TrimSpace(input.Reason)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.SuggestedFields == nil {
		input.SuggestedFields = map[string]any{}
	}
	if proposedWorkType := stringField(input.SuggestedFields, "work_type"); proposedWorkType != "" {
		proposedWorkType = strings.ToLower(strings.TrimSpace(proposedWorkType))
		if !isTicketWorkType(proposedWorkType) {
			return nil, fmt.Errorf("%w: ticket classification work_type is invalid", ErrInvalidInput)
		}
		normalizedFields := make(map[string]any, len(input.SuggestedFields))
		for key, value := range input.SuggestedFields {
			normalizedFields[key] = value
		}
		normalizedFields["work_type"] = proposedWorkType
		input.SuggestedFields = normalizedFields
	}
	if input.OrgID == "" || input.ConversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if input.Confidence < 0 {
		input.Confidence = 0
	}
	if input.Confidence > 1 {
		input.Confidence = 1
	}
	input.Outcome = normalizeClassificationOutcome(input.Outcome, input.Confidence, input.SuggestedFields, input.EvidenceMessageIDs)
	payload := map[string]any{
		"outcome":              input.Outcome,
		"confidence":           input.Confidence,
		"reason":               input.Reason,
		"suggested_fields":     input.SuggestedFields,
		"evidence_message_ids": input.EvidenceMessageIDs,
	}
	classification, err := s.repository.RecordTicketClassification(ctx, input, payload)
	if err != nil {
		return nil, err
	}
	if input.Outcome == "no_ticket" {
		return classification, nil
	}
	status := "suggested"
	if input.Outcome == "auto_ticket" {
		status = StatusOpen
	}
	ticket, err := s.CreateTicket(ctx, CreateTicketInput{
		OrgID:          input.OrgID,
		ConversationID: input.ConversationID,
		Status:         status,
		WorkType:       stringField(input.SuggestedFields, "work_type"),
		Priority:       stringField(input.SuggestedFields, "priority"),
		Severity:       stringField(input.SuggestedFields, "severity"),
		Category:       stringField(input.SuggestedFields, "category"),
		Intent:         stringField(input.SuggestedFields, "intent"),
		TeamID:         stringField(input.SuggestedFields, "team_id"),
		TeamName:       stringField(input.SuggestedFields, "team_name"),
		DueAt:          timeField(input.SuggestedFields, "due_at"),
		Source:         "ai",
		AIConfidence:   input.Confidence,
		AIReason:       input.Reason,
		CreatedBy:      "ai",
		ActorUserID:    input.ActorUserID,
	})
	if err != nil {
		if errors.Is(err, ErrConflict) {
			existing, lookupErr := s.repository.GetTicketByConversation(ctx, input.OrgID, input.ConversationID)
			if lookupErr != nil {
				return nil, lookupErr
			}
			classification.Ticket = existing
			return classification, nil
		}
		return nil, err
	}
	classification.Ticket = ticket
	return classification, nil
}

func (s *Service) ListTicketTeams(ctx context.Context, orgID string) ([]TicketTeam, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketTeams(ctx, orgID)
}

func (s *Service) CreateTicketTeam(ctx context.Context, input CreateTicketTeamInput) (*TicketTeam, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.Description = strings.TrimSpace(input.Description)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and team name are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketTeam(ctx, input)
}

func (s *Service) UpdateTicketTeam(ctx context.Context, input UpdateTicketTeamInput) (*TicketTeam, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	trimStringPtr(input.Description)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket team id are required", ErrInvalidInput)
	}
	if input.Name != nil && *input.Name == "" {
		return nil, fmt.Errorf("%w: team name is required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketTeam(ctx, input)
}

func (s *Service) ListTicketViews(ctx context.Context, orgID string) ([]TicketView, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketViews(ctx, orgID)
}

func (s *Service) CreateTicketView(ctx context.Context, input CreateTicketViewInput) (*TicketView, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.Scope = normalizeScopedValue(input.Scope, "org", "org", "user", "team")
	input.OwnerUserID = strings.TrimSpace(input.OwnerUserID)
	input.TeamID = strings.TrimSpace(input.TeamID)
	input.Visibility = normalizeScopedValue(input.Visibility, "sidebar", "sidebar", "hidden")
	input.GroupBy = strings.TrimSpace(input.GroupBy)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Filter == nil {
		input.Filter = map[string]any{}
	}
	if input.Sort == nil {
		input.Sort = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketView(ctx, input)
}

func (s *Service) UpdateTicketView(ctx context.Context, input UpdateTicketViewInput) (*TicketView, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	normalizeScopedPtr(input.Scope, "org", "org", "user", "team")
	trimStringPtr(input.OwnerUserID)
	trimStringPtr(input.TeamID)
	normalizeScopedPtr(input.Visibility, "sidebar", "sidebar", "hidden")
	trimStringPtr(input.GroupBy)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket view id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketView(ctx, input)
}

func (s *Service) ListTicketMacros(ctx context.Context, orgID string) ([]TicketMacro, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketMacros(ctx, orgID)
}

func (s *Service) CreateTicketMacro(ctx context.Context, input CreateTicketMacroInput) (*TicketMacro, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.Description = strings.TrimSpace(input.Description)
	input.Visibility = normalizeScopedValue(input.Visibility, "team", "personal", "team", "org")
	input.TeamID = strings.TrimSpace(input.TeamID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Actions == nil {
		input.Actions = map[string]any{}
	}
	if input.Conditions == nil {
		input.Conditions = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketMacro(ctx, input)
}

func (s *Service) UpdateTicketMacro(ctx context.Context, input UpdateTicketMacroInput) (*TicketMacro, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	trimStringPtr(input.Description)
	normalizeScopedPtr(input.Visibility, "team", "personal", "team", "org")
	trimStringPtr(input.TeamID)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket macro id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketMacro(ctx, input)
}

func (s *Service) RunTicketMacro(ctx context.Context, input TicketMacroRunInput) (*TicketMacroRunResult, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.MacroID = strings.TrimSpace(input.MacroID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.ExpectedMacroUpdatedAt = strings.TrimSpace(input.ExpectedMacroUpdatedAt)
	if input.OrgID == "" || input.TicketID == "" || input.MacroID == "" {
		return nil, fmt.Errorf("%w: org_id, ticket_id, and macro_id are required", ErrInvalidInput)
	}
	macro, err := s.repository.GetTicketMacro(ctx, input.OrgID, input.MacroID)
	if err != nil {
		return nil, err
	}
	if !macro.Active {
		return nil, fmt.Errorf("%w: ticket macro is inactive", ErrInvalidInput)
	}
	if input.ExpectedMacroUpdatedAt != "" {
		expected, err := time.Parse(time.RFC3339Nano, input.ExpectedMacroUpdatedAt)
		if err != nil {
			return nil, fmt.Errorf("%w: expected macro revision must be RFC3339", ErrInvalidInput)
		}
		if !macro.UpdatedAt.Equal(expected) {
			return nil, fmt.Errorf("%w: ticket macro changed after review", ErrConflict)
		}
	}
	patch := updateTicketInputFromActions(input.OrgID, input.TicketID, input.ActorUserID, macro.Actions)
	ticket, err := s.UpdateTicket(ctx, patch)
	if err != nil {
		return nil, err
	}
	input.Actions = macro.Actions
	if err := s.repository.RecordTicketMacroRun(ctx, input); err != nil {
		return nil, err
	}
	return &TicketMacroRunResult{Ticket: ticket, Macro: *macro}, nil
}

func (s *Service) ListTicketAutomationRules(ctx context.Context, orgID string) ([]TicketAutomationRule, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListTicketAutomationRules(ctx, orgID)
}

func (s *Service) CreateTicketAutomationRule(ctx context.Context, input CreateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.EventName = normalizeAutomationEvent(input.EventName)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Conditions == nil {
		input.Conditions = map[string]any{}
	}
	if input.Actions == nil {
		input.Actions = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" || input.EventName == "" {
		return nil, fmt.Errorf("%w: org_id, name, and event_name are required", ErrInvalidInput)
	}
	if err := normalizeTicketAutomationRule(&input.EventName, &input.Conditions, &input.Actions); err != nil {
		return nil, err
	}
	rule, err := s.repository.CreateTicketAutomationRule(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTicketAutomationRuleCreated, &ConversationDetail{ConversationSummary: ConversationSummary{OrgID: rule.OrgID}}, nil, input.ActorUserID, map[string]any{"ticket_automation_rule": rule})
	return rule, nil
}

func (s *Service) evaluateTicketAutomationRules(ctx context.Context, eventName string, ticket *Ticket, actorUserID string) (*Ticket, error) {
	if ticket == nil {
		return ticket, nil
	}
	rules, err := s.repository.ListTicketAutomationRules(ctx, ticket.OrgID)
	if err != nil {
		return ticket, err
	}
	current := ticket
	for _, rule := range rules {
		if !rule.Active || rule.EventName != eventName || !ticketAutomationConditionsMatch(rule.Conditions, current) {
			continue
		}
		patch := updateTicketInputFromActions(current.OrgID, current.ID, actorUserID, rule.Actions)
		if !updateTicketInputHasChanges(patch) {
			continue
		}
		updated, err := s.repository.UpdateTicket(ctx, patch)
		if err != nil {
			return current, err
		}
		current = updated
		s.publishTicket(ctx, SubjectTicketUpdated, current, actorUserID)
	}
	return current, nil
}

func (s *Service) UpdateTicketAutomationRule(ctx context.Context, input UpdateTicketAutomationRuleInput) (*TicketAutomationRule, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	normalizeAutomationEventPtr(input.EventName)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and automation rule id are required", ErrInvalidInput)
	}
	if input.Conditions != nil || input.Actions != nil || input.EventName != nil {
		if err := normalizeTicketAutomationRule(input.EventName, input.Conditions, input.Actions); err != nil {
			return nil, err
		}
	}
	rule, err := s.repository.UpdateTicketAutomationRule(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectTicketAutomationRuleUpdated, &ConversationDetail{ConversationSummary: ConversationSummary{OrgID: rule.OrgID}}, nil, input.ActorUserID, map[string]any{"ticket_automation_rule": rule})
	return rule, nil
}

func (s *Service) ListSLAPolicies(ctx context.Context, orgID string) ([]SLAPolicy, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListSLAPolicies(ctx, orgID)
}

func (s *Service) CreateSLAPolicy(ctx context.Context, input CreateSLAPolicyInput) (*SLAPolicy, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.CalendarRef = strings.TrimSpace(input.CalendarRef)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.Conditions == nil {
		input.Conditions = map[string]any{}
	}
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	return s.repository.CreateSLAPolicy(ctx, input)
}

func (s *Service) UpdateSLAPolicy(ctx context.Context, input UpdateSLAPolicyInput) (*SLAPolicy, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ID = strings.TrimSpace(input.ID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	trimStringPtr(input.Name)
	trimStringPtr(input.CalendarRef)
	if input.OrgID == "" || input.ID == "" {
		return nil, fmt.Errorf("%w: org_id and SLA policy id are required", ErrInvalidInput)
	}
	return s.repository.UpdateSLAPolicy(ctx, input)
}

func (s *Service) CreateTicketChecklist(ctx context.Context, input CreateTicketChecklistInput) (*TicketChecklist, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.Name = strings.TrimSpace(input.Name)
	input.TemplateID = strings.TrimSpace(input.TemplateID)
	input.CreatedByUserID = strings.TrimSpace(input.CreatedByUserID)
	if input.Name == "" {
		input.Name = "Support checklist"
	}
	input.Items = normalizeLabels(input.Items)
	if input.OrgID == "" || input.TicketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketChecklist(ctx, input)
}

func (s *Service) UpdateTicketChecklistItem(ctx context.Context, input UpdateTicketChecklistItemInput) (*TicketChecklist, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ChecklistID = strings.TrimSpace(input.ChecklistID)
	input.ItemID = strings.TrimSpace(input.ItemID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" || input.ChecklistID == "" || input.ItemID == "" {
		return nil, fmt.Errorf("%w: org_id, ticket_id, checklist_id, and item_id are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketChecklistItem(ctx, input)
}

func (s *Service) CreateTicketSideConversation(ctx context.Context, input CreateTicketSideConversationInput) (*TicketSideConversation, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.Subject = strings.TrimSpace(input.Subject)
	input.BodyText = strings.TrimSpace(input.BodyText)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" || input.Subject == "" || input.BodyText == "" || len(input.Subject) > 160 || len(input.BodyText) > 4000 || strings.ContainsAny(input.Subject, "\r\n") {
		return nil, fmt.Errorf("%w: a bounded ticket, subject, and first internal message are required", ErrInvalidInput)
	}
	return s.repository.CreateTicketSideConversation(ctx, input)
}

func (s *Service) AddTicketSideConversationMessage(ctx context.Context, input AddTicketSideConversationMessageInput) (*TicketSideConversation, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.SideConversationID = strings.TrimSpace(input.SideConversationID)
	input.BodyText = strings.TrimSpace(input.BodyText)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" || input.SideConversationID == "" || input.BodyText == "" || len(input.BodyText) > 4000 {
		return nil, fmt.Errorf("%w: a bounded ticket-side message is required", ErrInvalidInput)
	}
	return s.repository.AddTicketSideConversationMessage(ctx, input)
}

func (s *Service) UpdateTicketSideConversation(ctx context.Context, input UpdateTicketSideConversationInput) (*TicketSideConversation, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.SideConversationID = strings.TrimSpace(input.SideConversationID)
	input.Status = strings.ToLower(strings.TrimSpace(input.Status))
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" || input.SideConversationID == "" || !isTicketSideConversationStatus(input.Status) {
		return nil, fmt.Errorf("%w: ticket-side conversation and an open or closed status are required", ErrInvalidInput)
	}
	return s.repository.UpdateTicketSideConversation(ctx, input)
}

func (s *Service) RecordTicketChatHandoff(ctx context.Context, input TicketChatHandoffInput) (*Ticket, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.TicketID = strings.TrimSpace(input.TicketID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.TicketID == "" {
		return nil, fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	return s.repository.RecordTicketChatHandoff(ctx, input)
}

// HardPurgeByOrg hard-deletes every conversation_* row this service holds for
// orgID. It is the conversation-core half of the cross-plane GDPR erasure
// fan-out (verevon.gdpr.erasure.requested, consumed by
// consumers.OrgErasureConsumer). No lifecycle event is published for it: the
// org — and everyone who could ever read one — is gone by the time this runs,
// so there is no audience left to notify.
func (s *Service) HardPurgeByOrg(ctx context.Context, orgID string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.HardPurgeByOrg(ctx, orgID)
}

// PurgeConversationDraftsByOrg deletes the personal draft-recovery records
// for one organization. It is the narrow retention-toggle cleanup consumed
// from Control Plane; the durable customer-support record is out of scope.
func (s *Service) PurgeConversationDraftsByOrg(ctx context.Context, orgID string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.PurgeConversationDraftsByOrg(ctx, orgID)
}

// DistinctOrgIDsWithActiveTickets, ActiveTicketsForSupportRecurrenceCorpus,
// UpsertSupportRecurrenceCorpusEntry, EvictStaleSupportRecurrenceCorpusEntries,
// PurgeSupportRecurrenceCorpusByOrg, and ListSupportRecurrenceCorpus back the
// semantic support-recurrence corpus builder (consumers package) and, later,
// the anchor-ticket similarity search. Thin, validated pass-throughs — the
// ZDR-ineligibility and permission/membership checks live in the caller
// (the corpus builder, and the gateway respectively), not here.

func (s *Service) DistinctOrgIDsWithActiveTickets(ctx context.Context) ([]string, error) {
	return s.repository.DistinctOrgIDsWithActiveTickets(ctx)
}

func (s *Service) ActiveTicketsForSupportRecurrenceCorpus(ctx context.Context, orgID string) ([]Ticket, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ActiveTicketsForSupportRecurrenceCorpus(ctx, orgID)
}

func (s *Service) UpsertSupportRecurrenceCorpusEntry(ctx context.Context, orgID, ticketID string, embedding []float32, algorithmVersion string, corpusWindowStart time.Time) error {
	orgID = strings.TrimSpace(orgID)
	ticketID = strings.TrimSpace(ticketID)
	if orgID == "" || ticketID == "" {
		return fmt.Errorf("%w: org_id and ticket_id are required", ErrInvalidInput)
	}
	if len(embedding) == 0 {
		return fmt.Errorf("%w: embedding must not be empty", ErrInvalidInput)
	}
	return s.repository.UpsertSupportRecurrenceCorpusEntry(ctx, orgID, ticketID, embedding, algorithmVersion, corpusWindowStart)
}

func (s *Service) EvictStaleSupportRecurrenceCorpusEntries(ctx context.Context, orgID string, windowStart time.Time) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.EvictStaleSupportRecurrenceCorpusEntries(ctx, orgID, windowStart)
}

func (s *Service) PurgeSupportRecurrenceCorpusByOrg(ctx context.Context, orgID string) error {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.PurgeSupportRecurrenceCorpusByOrg(ctx, orgID)
}

func (s *Service) ListSupportRecurrenceCorpus(ctx context.Context, orgID string) ([]SupportRecurrenceCorpusEntry, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	return s.repository.ListSupportRecurrenceCorpus(ctx, orgID)
}

func (s *Service) publishTicket(ctx context.Context, subject string, ticket *Ticket, actorUserID string) {
	if ticket == nil {
		return
	}
	s.publish(ctx, subject, &ConversationDetail{ConversationSummary: ConversationSummary{
		ID:    ticket.ConversationID,
		OrgID: ticket.OrgID,
	}}, nil, actorUserID, map[string]any{"ticket": ticket})
}

func (s *Service) publish(ctx context.Context, subject string, detail *ConversationDetail, message *Message, actorUserID string, data map[string]any) {
	if s.publisher == nil {
		return
	}
	orgID := ""
	conversationID := ""
	if detail != nil {
		orgID = detail.OrgID
		conversationID = detail.ID
	}
	if orgID == "" && message != nil {
		orgID = message.OrgID
	}
	if conversationID == "" && message != nil {
		conversationID = message.ConversationID
	}
	messageID := ""
	if message != nil {
		messageID = message.ID
	}
	if data == nil {
		data = map[string]any{}
	}
	if detail != nil && detail.ID != "" {
		data["conversation"] = detail.ConversationSummary
	}
	if message != nil {
		data["message"] = message
	}
	eventType := strings.TrimPrefix(subject, "verevon.application.conversation.")
	if err := s.publisher.Publish(ctx, subject, LifecycleEvent{
		ID:             newID("evt"),
		Type:           eventType,
		OrgID:          orgID,
		ConversationID: conversationID,
		MessageID:      messageID,
		ActorUserID:    actorUserID,
		Data:           data,
		OccurredAt:     s.now().UTC(),
	}); err != nil {
		log.Printf("conversation-core-go: publish %s: %v", subject, err)
	}
}

func normalizeInboundEvent(event InboundEvent, now func() time.Time) InboundEvent {
	event.IDempotencyKey = strings.TrimSpace(event.IDempotencyKey)
	event.OrgID = strings.TrimSpace(event.OrgID)
	event.ConnectionID = strings.TrimSpace(event.ConnectionID)
	event.Provider = strings.TrimSpace(strings.ToLower(event.Provider))
	if event.Provider == "" {
		event.Provider = "email"
	}
	event.ProviderEventID = strings.TrimSpace(event.ProviderEventID)
	event.ProviderMessageID = strings.TrimSpace(event.ProviderMessageID)
	event.ProviderThreadID = strings.TrimSpace(event.ProviderThreadID)
	event.MessageIDHeader = sanitizeStorableText(event.MessageIDHeader)
	event.ReferencesHeader = sanitizeStorableText(event.ReferencesHeader)
	event.InReplyToHeader = sanitizeStorableText(event.InReplyToHeader)
	event.AutoSubmitted = strings.ToLower(sanitizeStorableText(event.AutoSubmitted))
	event.ContentType = strings.ToLower(sanitizeStorableText(event.ContentType))
	event.OutboundCorrelationID = sanitizeStorableText(event.OutboundCorrelationID)
	event.Direction = strings.TrimSpace(event.Direction)
	if event.Direction == "" {
		event.Direction = DirectionInbound
	}
	// Postgres TEXT columns reject the NUL byte (0x00) with SQLSTATE 22021
	// ("invalid byte sequence for encoding UTF8: 0x00"), and other C0 control
	// bytes corrupt rendering. Real provider payloads carry them routinely —
	// Outlook/Graph HTML bodies, quoted-printable email, some Slack/Teams
	// blocks — so a single such message would otherwise 500 the ingest and
	// stall the poller's cursor forever (it retries the same message every
	// cycle). Strip storage-unsafe runes at this one chokepoint, which every
	// inbound path (email bridge + webhook consumer) funnels through.
	event.Subject = sanitizeStorableText(event.Subject)
	event.BodyText = sanitizeStorableText(event.BodyText)
	event.BodyHTML = sanitizeStorableText(event.BodyHTML)
	event.ProviderEventID = sanitizeStorableText(event.ProviderEventID)
	event.ProviderMessageID = sanitizeStorableText(event.ProviderMessageID)
	event.ProviderThreadID = sanitizeStorableText(event.ProviderThreadID)
	event.From.Name = sanitizeStorableText(event.From.Name)
	event.From.Email = strings.ToLower(sanitizeStorableText(event.From.Email))
	if len(event.Attachments) > 0 {
		attachments := make([]AttachmentInput, 0, len(event.Attachments))
		for _, attachment := range event.Attachments {
			attachments = append(attachments, AttachmentInput{
				Filename:    sanitizeStorableText(attachment.Filename),
				MimeType:    strings.ToLower(sanitizeStorableText(attachment.MimeType)),
				SizeBytes:   attachment.SizeBytes,
				StorageRef:  sanitizeStorableText(attachment.StorageRef),
				ProviderRef: sanitizeStorableText(attachment.ProviderRef),
			})
		}
		event.Attachments = attachments
	}
	if event.OccurredAt.IsZero() {
		event.OccurredAt = now().UTC()
	} else {
		event.OccurredAt = event.OccurredAt.UTC()
	}
	if event.IDempotencyKey == "" {
		parts := []string{event.OrgID, event.Provider, event.ProviderEventID, event.ProviderMessageID, event.ProviderThreadID}
		event.IDempotencyKey = strings.Join(parts, ":")
	}
	return event
}

// sanitizeStorableText trims the value and removes runes that Postgres TEXT
// columns cannot store or that corrupt display: the NUL byte (0x00, rejected
// with SQLSTATE 22021) and other C0/C1 control characters, keeping only the
// whitespace controls tab, newline, and carriage return. The Unicode
// replacement char (U+FFFD) from earlier lossy decoding is also dropped. This
// is deliberately conservative — it never rewrites otherwise-valid content.
func sanitizeStorableText(value string) string {
	trimmed := strings.TrimSpace(value)
	if !strings.ContainsFunc(trimmed, isStorageUnsafeRune) {
		return trimmed
	}
	var b strings.Builder
	b.Grow(len(trimmed))
	for _, r := range trimmed {
		if isStorageUnsafeRune(r) {
			continue
		}
		b.WriteRune(r)
	}
	return strings.TrimSpace(b.String())
}

func isStorageUnsafeRune(r rune) bool {
	switch r {
	case '\t', '\n', '\r':
		return false
	case '�':
		return true
	}
	// C0 controls (incl. NUL) and C1 controls.
	return r < 0x20 || (r >= 0x7f && r <= 0x9f)
}

func validateInboundEvent(event InboundEvent) error {
	if event.OrgID == "" {
		return fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if event.IDempotencyKey == "" || event.IDempotencyKey == ":::" {
		return fmt.Errorf("%w: idempotency_key or provider refs are required", ErrInvalidInput)
	}
	if event.Direction != DirectionInbound && !(event.Provider == "teams" && event.Direction == DirectionOutbound) {
		return fmt.Errorf("%w: event direction must be inbound, except for trusted Teams sync history", ErrInvalidInput)
	}
	if event.Subject == "" {
		event.Subject = "(no subject)"
	}
	if event.BodyText == "" && event.BodyHTML == "" {
		return fmt.Errorf("%w: body_text or body_html is required", ErrInvalidInput)
	}
	if event.From.Email == "" && event.From.Name == "" {
		return fmt.Errorf("%w: sender is required", ErrInvalidInput)
	}
	if event.OutboundCorrelationID != "" && !validOutboundCorrelationID(event.OutboundCorrelationID) {
		return fmt.Errorf("%w: outbound_correlation_id is invalid", ErrInvalidInput)
	}
	if len(event.Attachments) > 25 {
		return fmt.Errorf("%w: at most 25 attachments are supported", ErrInvalidInput)
	}
	for _, attachment := range event.Attachments {
		if attachment.Filename == "" || utf8.RuneCountInString(attachment.Filename) > 255 {
			return fmt.Errorf("%w: attachment filename is invalid", ErrInvalidInput)
		}
		if utf8.RuneCountInString(attachment.MimeType) > 127 || attachment.SizeBytes < 0 || attachment.SizeBytes > 100*1024*1024 {
			return fmt.Errorf("%w: attachment metadata is invalid", ErrInvalidInput)
		}
		if utf8.RuneCountInString(attachment.StorageRef) > 2_000 || utf8.RuneCountInString(attachment.ProviderRef) > 2_000 {
			return fmt.Errorf("%w: attachment reference is invalid", ErrInvalidInput)
		}
	}
	return nil
}

func validOutboundCorrelationID(value string) bool {
	if len(value) < 8 || len(value) > 128 {
		return false
	}
	for _, character := range value {
		if !(character >= 'a' && character <= 'z') && !(character >= 'A' && character <= 'Z') && !(character >= '0' && character <= '9') && character != '_' && character != '-' {
			return false
		}
	}
	return true
}

func isMachineDeliveryFailureReport(event InboundEvent) bool {
	if event.Provider != "google" && event.Provider != "microsoft" {
		return false
	}
	if !validOutboundCorrelationID(event.OutboundCorrelationID) {
		return false
	}
	contentType := strings.ToLower(event.ContentType)
	return strings.HasPrefix(strings.ToLower(event.AutoSubmitted), "auto-replied") &&
		strings.Contains(contentType, "multipart/report") &&
		strings.Contains(contentType, "report-type=delivery-status")
}

func normalizeStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "new", "open":
		return StatusOpen
	case "pending", "pending reminder":
		return StatusPending
	case "solved", "closed":
		return StatusSolved
	case "archived":
		return StatusClosed
	default:
		return strings.ToLower(strings.TrimSpace(status))
	}
}

func normalizeCreateTicketInput(input CreateTicketInput) CreateTicketInput {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ConversationID = strings.TrimSpace(input.ConversationID)
	input.Status = normalizeTicketStatus(input.Status)
	if input.Status == "" {
		input.Status = StatusOpen
	}
	input.WorkType = strings.ToLower(strings.TrimSpace(input.WorkType))
	if input.WorkType == "" {
		input.WorkType = "customer_case"
	}
	input.Priority = normalizeTicketPriority(input.Priority)
	input.Severity = normalizeTicketSeverity(input.Severity)
	input.Category = strings.TrimSpace(input.Category)
	input.Intent = strings.TrimSpace(input.Intent)
	input.AssigneeUserID = strings.TrimSpace(input.AssigneeUserID)
	input.AssigneeName = strings.TrimSpace(input.AssigneeName)
	input.TeamID = strings.TrimSpace(input.TeamID)
	input.TeamName = strings.TrimSpace(input.TeamName)
	input.Source = strings.TrimSpace(input.Source)
	if input.Source == "" {
		input.Source = "manual"
	}
	input.AIReason = strings.TrimSpace(input.AIReason)
	input.CreatedBy = strings.TrimSpace(input.CreatedBy)
	if input.CreatedBy == "" {
		input.CreatedBy = input.ActorUserID
	}
	input.SLAPolicyID = strings.TrimSpace(input.SLAPolicyID)
	input.Labels = normalizeLabels(input.Labels)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.AIConfidence < 0 {
		input.AIConfidence = 0
	}
	if input.AIConfidence > 1 {
		input.AIConfidence = 1
	}
	switch input.Status {
	case "resolved", "closed":
		if input.ResolvedAt == nil {
			resolvedAt := time.Now().UTC()
			input.ResolvedAt = &resolvedAt
		}
	case "waiting_customer", "waiting_team":
		if input.WaitingSince == nil {
			waitingSince := time.Now().UTC()
			input.WaitingSince = &waitingSince
		}
	}
	return input
}

func isTicketWorkType(value string) bool {
	switch value {
	case "customer_case", "internal_work", "incident":
		return true
	default:
		return false
	}
}

func normalizeIncidentStatus(value string) string {
	value = strings.ToLower(strings.TrimSpace(value))
	if value == "" {
		return "declared"
	}
	return value
}

func isIncidentStatus(value string) bool {
	switch value {
	case "declared", "investigating", "monitoring", "resolved":
		return true
	default:
		return false
	}
}

func normalizeProblemStatus(value string) string {
	value = strings.ToLower(strings.TrimSpace(value))
	if value == "" {
		return "investigating"
	}
	return value
}

func isProblemStatus(value string) bool {
	switch value {
	case "investigating", "known_error", "resolved":
		return true
	default:
		return false
	}
}

func normalizeIncidentTicketRelationship(value string) string {
	return strings.ToLower(strings.TrimSpace(value))
}

func isIncidentTicketRelationship(value string) bool {
	switch value {
	case "affected", "root_cause", "related":
		return true
	default:
		return false
	}
}

func normalizeTicketStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "":
		return ""
	case "new", "open":
		return StatusOpen
	case "suggested", "suggestion":
		return "suggested"
	case "pending", "waiting", "waiting_customer", "waiting-customer":
		return "waiting_customer"
	case "waiting_team", "waiting-team":
		return "waiting_team"
	case "snoozed", "snooze":
		return "snoozed"
	case "escalated":
		return "escalated"
	case "resolved", "solved":
		return "resolved"
	case "closed":
		return "closed"
	default:
		return strings.ToLower(strings.TrimSpace(status))
	}
}

// isAIReviewableTicketStatus is intentionally narrower than UpdateTicket's
// lifecycle model. A reviewed AI proposal may advance ongoing work, but it
// must never represent a resolution, closure, or snooze decision.
func isAIReviewableTicketStatus(status string) bool {
	switch status {
	case StatusOpen, "waiting_customer", "waiting_team", "escalated":
		return true
	default:
		return false
	}
}

func normalizeOptionalTicketPriority(priority string) string {
	value := strings.ToLower(strings.TrimSpace(priority))
	if value == "" {
		return ""
	}
	return normalizeTicketPriority(value)
}

func normalizeTicketPriority(priority string) string {
	switch strings.ToLower(strings.TrimSpace(priority)) {
	case "low", "normal", "high", "urgent":
		return strings.ToLower(strings.TrimSpace(priority))
	default:
		return "normal"
	}
}

func normalizeOptionalTicketSeverity(severity string) string {
	value := strings.ToLower(strings.TrimSpace(severity))
	if value == "" {
		return ""
	}
	return normalizeTicketSeverity(value)
}

func normalizeTicketSeverity(severity string) string {
	switch strings.ToLower(strings.TrimSpace(severity)) {
	case "low", "medium", "high", "critical":
		return strings.ToLower(strings.TrimSpace(severity))
	default:
		return "medium"
	}
}

func isTicketSeverity(severity string) bool {
	switch strings.ToLower(strings.TrimSpace(severity)) {
	case "low", "medium", "high", "critical":
		return true
	default:
		return false
	}
}

func isTicketSideConversationStatus(value string) bool {
	return value == TicketSideConversationOpen || value == TicketSideConversationClosed
}

func normalizeClassificationOutcome(outcome string, confidence float64, fields map[string]any, evidence []string) string {
	requested := strings.ToLower(strings.TrimSpace(outcome))
	category := stringField(fields, "category")
	if category == "" {
		category = stringField(fields, "intent")
	}
	if requested == "auto_ticket" && canAutoCreateTicket(confidence, category, evidence) {
		return "auto_ticket"
	}
	if requested == "suggest_ticket" || requested == "suggested" {
		return "suggest_ticket"
	}
	if requested == "no_ticket" {
		return "no_ticket"
	}
	if confidence < 0.60 {
		return "no_ticket"
	}
	if canAutoCreateTicket(confidence, category, evidence) {
		return "auto_ticket"
	}
	return "suggest_ticket"
}

func canAutoCreateTicket(confidence float64, category string, evidence []string) bool {
	return confidence >= 0.90 && len(evidence) > 0 && !isSensitiveTicketCategory(category)
}

func isSensitiveTicketCategory(category string) bool {
	normalized := strings.ToLower(strings.TrimSpace(category))
	for _, sensitive := range []string{"legal", "security", "abuse", "payment dispute", "medical", "regulated"} {
		if strings.Contains(normalized, sensitive) {
			return true
		}
	}
	return false
}

func normalizeSLAState(value string) string {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "ok", "risk", "breached":
		return strings.ToLower(strings.TrimSpace(value))
	default:
		return ""
	}
}

func normalizeLinkType(value string) string {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "parent", "child", "related", "external":
		return strings.ToLower(strings.TrimSpace(value))
	default:
		return "normal"
	}
}

func normalizeLabels(values []string) []string {
	if len(values) == 0 {
		return []string{}
	}
	seen := map[string]struct{}{}
	labels := make([]string, 0, len(values))
	for _, value := range values {
		label := strings.TrimSpace(value)
		if label == "" {
			continue
		}
		key := strings.ToLower(label)
		if _, ok := seen[key]; ok {
			continue
		}
		seen[key] = struct{}{}
		labels = append(labels, label)
	}
	return labels
}

func normalizeScopedValue(value, fallback string, allowed ...string) string {
	normalized := strings.ToLower(strings.TrimSpace(value))
	for _, candidate := range allowed {
		if normalized == candidate {
			return normalized
		}
	}
	return fallback
}

func normalizeScopedPtr(value *string, fallback string, allowed ...string) {
	if value == nil {
		return
	}
	*value = normalizeScopedValue(*value, fallback, allowed...)
}

func normalizeAutomationEvent(value string) string {
	normalized := strings.ToLower(strings.TrimSpace(value))
	switch normalized {
	case "ticket.created", "ticket.updated", "message.received", "sla.risk", "customer.replied", "ticket.resolved":
		return normalized
	default:
		return ""
	}
}

func normalizeAutomationEventPtr(value *string) {
	if value == nil {
		return
	}
	*value = normalizeAutomationEvent(*value)
}

func normalizeTicketAutomationRule(eventName *string, conditions, actions *map[string]any) error {
	if eventName != nil && *eventName != "ticket.created" && *eventName != "ticket.updated" {
		return fmt.Errorf("%w: ticket automation supports ticket.created or ticket.updated", ErrInvalidInput)
	}
	if conditions != nil {
		if len(*conditions) == 0 || len(*conditions) > 4 {
			return fmt.Errorf("%w: automation rules require 1-4 conditions", ErrInvalidInput)
		}
		for key, raw := range *conditions {
			if key != "status" && key != "priority" && key != "severity" && key != "category" && key != "intent" && key != "label" && key != "work_type" {
				return fmt.Errorf("%w: unsupported automation condition %q", ErrInvalidInput, key)
			}
			if !validAutomationValue(key, raw, key == "label") {
				return fmt.Errorf("%w: invalid automation condition %q", ErrInvalidInput, key)
			}
		}
	}
	if actions != nil {
		if len(*actions) == 0 || len(*actions) > 3 {
			return fmt.Errorf("%w: automation rules require 1-3 actions", ErrInvalidInput)
		}
		for key, raw := range *actions {
			if key != "status" && key != "priority" && key != "severity" && key != "category" && key != "intent" && key != "labels" {
				return fmt.Errorf("%w: unsupported automation action %q", ErrInvalidInput, key)
			}
			if !validAutomationValue(key, raw, key == "labels") {
				return fmt.Errorf("%w: invalid automation action %q", ErrInvalidInput, key)
			}
		}
	}
	return nil
}

func validAutomationText(raw any, allowList bool) bool {
	if text, ok := raw.(string); ok {
		return strings.TrimSpace(text) != "" && len([]rune(strings.TrimSpace(text))) <= 120
	}
	if !allowList {
		return false
	}
	items := stringSliceField(map[string]any{"value": raw}, "value")
	if len(items) == 0 || len(items) > 10 {
		return false
	}
	for _, item := range items {
		if strings.TrimSpace(item) == "" || len([]rune(strings.TrimSpace(item))) > 80 {
			return false
		}
	}
	return true
}

func validAutomationValue(key string, raw any, allowList bool) bool {
	if key == "labels" {
		switch raw.(type) {
		case []string, []any:
		default:
			return false
		}
	}
	if !validAutomationText(raw, allowList) {
		return false
	}
	value, ok := raw.(string)
	if !ok {
		return true
	}
	value = strings.ToLower(strings.TrimSpace(value))
	switch key {
	case "priority":
		return value == "low" || value == "normal" || value == "high" || value == "urgent"
	case "severity":
		return value == "low" || value == "medium" || value == "high" || value == "critical"
	case "status":
		return value == "open" || value == "suggested" || value == "waiting_customer" || value == "waiting_team" || value == "snoozed" || value == "escalated" || value == "resolved" || value == "closed"
	case "work_type":
		return isTicketWorkType(value)
	default:
		return true
	}
}

func updateTicketInputFromActions(orgID, ticketID, actorUserID string, actions map[string]any) UpdateTicketInput {
	input := UpdateTicketInput{
		OrgID:       orgID,
		TicketID:    ticketID,
		ActorUserID: actorUserID,
	}
	if status := stringField(actions, "status"); status != "" {
		input.Status = &status
	}
	if priority := stringField(actions, "priority"); priority != "" {
		input.Priority = &priority
	}
	if severity := stringField(actions, "severity"); severity != "" {
		input.Severity = &severity
	}
	if category := stringField(actions, "category"); category != "" {
		input.Category = &category
	}
	if intent := stringField(actions, "intent"); intent != "" {
		input.Intent = &intent
	}
	if assigneeUserID := stringField(actions, "assignee_user_id"); assigneeUserID != "" {
		input.AssigneeUserID = &assigneeUserID
	}
	if assigneeName := stringField(actions, "assignee_name"); assigneeName != "" {
		input.AssigneeName = &assigneeName
	}
	if teamID := stringField(actions, "team_id"); teamID != "" {
		input.TeamID = &teamID
	}
	if teamName := stringField(actions, "team_name"); teamName != "" {
		input.TeamName = &teamName
	}
	if slaPolicyID := stringField(actions, "sla_policy_id"); slaPolicyID != "" {
		input.SLAPolicyID = &slaPolicyID
	}
	if labels := stringSliceField(actions, "labels"); len(labels) > 0 {
		input.Labels = &labels
	}
	if dueAt := timeField(actions, "due_at"); dueAt != nil {
		input.DueAt = dueAt
	}
	if snoozedUntil := timeField(actions, "snoozed_until"); snoozedUntil != nil {
		input.SnoozedUntil = snoozedUntil
	}
	return input
}

func updateTicketInputHasChanges(input UpdateTicketInput) bool {
	return input.Status != nil ||
		input.Priority != nil ||
		input.Severity != nil ||
		input.Category != nil ||
		input.Intent != nil ||
		input.AssigneeUserID != nil ||
		input.AssigneeName != nil ||
		input.TeamID != nil ||
		input.TeamName != nil ||
		input.DueAt != nil ||
		input.Source != nil ||
		input.AIConfidence != nil ||
		input.AIReason != nil ||
		input.WaitingSince != nil ||
		input.LastCustomerReplyAt != nil ||
		input.FirstResponseAt != nil ||
		input.ResolvedAt != nil ||
		input.SnoozedUntil != nil ||
		input.SLAPolicyID != nil ||
		input.EscalationAt != nil ||
		input.Labels != nil
}

func ticketAutomationConditionsMatch(conditions map[string]any, ticket *Ticket) bool {
	if len(conditions) == 0 {
		return true
	}
	values := map[string]string{
		"status":    ticket.Status,
		"priority":  ticket.Priority,
		"severity":  ticket.Severity,
		"category":  ticket.Category,
		"intent":    ticket.Intent,
		"team_id":   ticket.TeamID,
		"team_name": ticket.TeamName,
		"sla_state": ticket.SLAState,
		"assignee":  ticket.AssigneeUserID,
		"source":    ticket.Source,
		"work_type": ticket.WorkType,
	}
	for key, expected := range conditions {
		if key == "label" || key == "labels" {
			if !conditionMatchesAnyLabel(expected, ticket.Labels) {
				return false
			}
			continue
		}
		actual, ok := values[key]
		if !ok {
			continue
		}
		if !conditionMatchesString(expected, actual) {
			return false
		}
	}
	return true
}

func conditionMatchesAnyLabel(expected any, labels []string) bool {
	for _, label := range labels {
		if conditionMatchesString(expected, label) {
			return true
		}
	}
	return false
}

func conditionMatchesString(expected any, actual string) bool {
	actual = strings.ToLower(strings.TrimSpace(actual))
	switch value := expected.(type) {
	case string:
		return strings.ToLower(strings.TrimSpace(value)) == actual
	case []string:
		for _, item := range value {
			if strings.ToLower(strings.TrimSpace(item)) == actual {
				return true
			}
		}
	case []any:
		for _, item := range value {
			if text, ok := item.(string); ok && strings.ToLower(strings.TrimSpace(text)) == actual {
				return true
			}
		}
	}
	return false
}

func normalizeStringPtr(value *string, normalize func(string) string) {
	if value == nil {
		return
	}
	*value = normalize(*value)
}

func trimStringPtr(value *string) {
	if value == nil {
		return
	}
	*value = strings.TrimSpace(*value)
}

func stringField(fields map[string]any, key string) string {
	if fields == nil {
		return ""
	}
	if value, ok := fields[key].(string); ok {
		return strings.TrimSpace(value)
	}
	return ""
}

func stringSliceField(fields map[string]any, key string) []string {
	if fields == nil {
		return nil
	}
	switch value := fields[key].(type) {
	case []string:
		return normalizeLabels(value)
	case []any:
		labels := make([]string, 0, len(value))
		for _, item := range value {
			if label, ok := item.(string); ok {
				labels = append(labels, label)
			}
		}
		return normalizeLabels(labels)
	case string:
		return normalizeLabels(strings.Split(value, ","))
	default:
		return nil
	}
}

func timeField(fields map[string]any, key string) *time.Time {
	value := stringField(fields, key)
	if value == "" {
		return nil
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return nil
	}
	utc := parsed.UTC()
	return &utc
}

func IsInvalidInput(err error) bool {
	return errors.Is(err, ErrInvalidInput)
}
