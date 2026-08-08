package conversation

import (
	"context"
	"fmt"
	"strings"
)

// ListOutboundIntents exposes only the authoritative, content-free outcome
// ledger for an existing conversation. The caller's organization is checked by
// GetConversation first, so a foreign or unknown conversation is not rendered
// as an indistinguishable empty delivery history.
func (s *Service) ListOutboundIntents(ctx context.Context, orgID, conversationID string) ([]OutboundIntent, error) {
	orgID = strings.TrimSpace(orgID)
	conversationID = strings.TrimSpace(conversationID)
	if orgID == "" || conversationID == "" {
		return nil, fmt.Errorf("%w: org_id and conversation_id are required", ErrInvalidInput)
	}
	if _, err := s.repository.GetConversation(ctx, orgID, conversationID); err != nil {
		return nil, err
	}
	return s.repository.ListOutboundIntents(ctx, orgID, conversationID)
}

// ListOrganizationOutboundIntents exposes the same canonical, content-free
// ledger at the Support workspace level. Only exact stored states are accepted
// so the API cannot synthesize queues such as "sent" or "scheduled".
func (s *Service) ListOrganizationOutboundIntents(ctx context.Context, filter OutboundIntentListFilter) ([]OutboundIntent, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Status = strings.TrimSpace(filter.Status)
	filter.Provider = strings.TrimSpace(filter.Provider)
	filter.DeliveryStatus = strings.TrimSpace(filter.DeliveryStatus)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Status != "" && !validOutboundIntentStatus(filter.Status) {
		return nil, fmt.Errorf("%w: outbound status is invalid", ErrInvalidInput)
	}
	if filter.DeliveryStatus != "" && !validProviderDeliveryStatus(filter.DeliveryStatus) {
		return nil, fmt.Errorf("%w: delivery status is invalid", ErrInvalidInput)
	}
	if filter.Limit <= 0 {
		filter.Limit = 50
	}
	if filter.Limit > 100 {
		filter.Limit = 100
	}
	return s.repository.ListOrganizationOutboundIntents(ctx, filter)
}

func validOutboundIntentStatus(value string) bool {
	switch value {
	case OutboundIntentSending, OutboundIntentRetryable, OutboundIntentSubmitted, OutboundIntentFailed, OutboundIntentUnknown:
		return true
	default:
		return false
	}
}

func validProviderDeliveryStatus(value string) bool {
	switch value {
	case ProviderDeliveryUnconfirmed, ProviderDeliveryDelivered, ProviderDeliveryRead, ProviderDeliveryFailed:
		return true
	default:
		return false
	}
}

// RecordProviderDeliveryReceipt persists evidence received after provider
// submission. The repository matches it by org, provider, and exact provider
// message ID, so an unmatched webhook cannot affect another conversation.
func (s *Service) RecordProviderDeliveryReceipt(ctx context.Context, input ProviderDeliveryReceiptInput) (bool, error) {
	return s.repository.RecordProviderDeliveryReceipt(ctx, input)
}
