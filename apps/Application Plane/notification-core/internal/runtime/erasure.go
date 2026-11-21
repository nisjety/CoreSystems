package runtime

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/novuhq/novu-go/v3/models/apierrors"
	"github.com/novuhq/novu-go/v3/models/components"
	"github.com/novuhq/novu-go/v3/models/operations"
)

// erasureMessagePageSize is the page size used to list a subscriber's messages during an organization erasure.
const erasureMessagePageSize = 100

// maxErasureMessagePasses bounds one call's listing passes. Each pass deletes the messages it lists, so a backlog larger
// than the bound continues on the next erasure retry instead of looping here.
const maxErasureMessagePasses = 1000

// DeleteSubscriberMessages removes every message Novu holds for one subscriber, on every channel including in-app, and
// returns how many this call removed. A subscriber with no messages, or one already deleted, removes zero and is not an
// error. Each pass lists the first page again, because the messages it deleted are no longer there to page past.
//
// Novu scopes the listing by the subscriber filter alone, so the subscriber is first resolved to its internal id, and every
// listed message must carry that id before any message on its page is deleted. A page holding another subscriber's message
// deletes nothing and fails the call, so the erasure retries instead of reaching another tenant's inbox.
func (a *NovuAdapter) DeleteSubscriberMessages(ctx context.Context, subscriberID string) (int, error) {
	if a == nil || a.client == nil {
		return 0, ErrDeliveryDisabled
	}
	subscriberID = strings.TrimSpace(subscriberID)
	if subscriberID == "" {
		return 0, errors.New("DeleteSubscriberMessages: subscriber_id required")
	}
	internalID, found, err := a.resolveErasureSubscriber(ctx, subscriberID)
	if err != nil {
		return 0, err
	}
	if !found {
		return 0, nil
	}

	deleted := 0
	for pass := 0; pass < maxErasureMessagePasses; pass++ {
		page := float64(0)
		limit := float64(erasureMessagePageSize)
		res, err := a.client.Messages.Retrieve(ctx, operations.MessagesControllerGetMessagesRequest{
			SubscriberID: &subscriberID,
			Page:         &page,
			Limit:        &limit,
		})
		if err != nil {
			if isNovuNotFound(err) {
				return deleted, nil
			}
			return deleted, fmt.Errorf("novu list subscriber messages: %w", err)
		}
		if res == nil || res.MessagesResponseDto == nil || len(res.MessagesResponseDto.Data) == 0 {
			return deleted, nil
		}
		messageIDs, err := ownedErasureMessageIDs(res.MessagesResponseDto.Data, subscriberID, internalID)
		if err != nil {
			return deleted, err
		}
		for _, messageID := range messageIDs {
			if _, err := a.client.Messages.Delete(ctx, messageID, nil); err != nil {
				if isNovuNotFound(err) {
					continue
				}
				return deleted, fmt.Errorf("novu delete subscriber message: %w", err)
			}
			deleted++
		}
		if !res.MessagesResponseDto.HasMore {
			return deleted, nil
		}
	}
	return deleted, fmt.Errorf("novu subscriber still holds messages after %d passes: erasure will retry", maxErasureMessagePasses)
}

// DeleteSubscriber removes one provider subscriber and reports whether this call removed it. The SDK has no call that
// deletes a subscriber's workflow preferences on their own, so the removal of the subscriber is assumed to take them too.
// That assumption is to be confirmed on a live tenant. A subscriber that is already gone is not an error.
func (a *NovuAdapter) DeleteSubscriber(ctx context.Context, subscriberID string) (bool, error) {
	if a == nil || a.client == nil {
		return false, ErrDeliveryDisabled
	}
	subscriberID = strings.TrimSpace(subscriberID)
	if subscriberID == "" {
		return false, errors.New("DeleteSubscriber: subscriber_id required")
	}
	if _, err := a.client.Subscribers.Delete(ctx, subscriberID, nil); err != nil {
		if isNovuNotFound(err) {
			return false, nil
		}
		return false, fmt.Errorf("novu subscriber delete: %w", err)
	}
	return true, nil
}

// resolveErasureSubscriber resolves a subscriber's external id to Novu's internal id, which each listed message carries.
// Novu's own JSON 404 means the subscriber is already gone, so there is nothing to list and found is false. Any other
// failure is returned. An answer without an internal id is refused, because its messages could not be scoped to it.
func (a *NovuAdapter) resolveErasureSubscriber(ctx context.Context, subscriberID string) (internalID string, found bool, err error) {
	res, err := a.client.Subscribers.Retrieve(ctx, subscriberID, nil)
	if err != nil {
		if isNovuNotFound(err) {
			return "", false, nil
		}
		return "", false, fmt.Errorf("novu resolve subscriber: %w", err)
	}
	if res == nil || res.SubscriberResponseDto == nil || res.SubscriberResponseDto.ID == nil || strings.TrimSpace(*res.SubscriberResponseDto.ID) == "" {
		return "", false, errors.New("novu subscriber has no internal id: erasure refuses to list its messages")
	}
	return strings.TrimSpace(*res.SubscriberResponseDto.ID), true, nil
}

// ownedErasureMessageIDs checks every listed message before any of them is deleted, and returns their ids. Each message
// must carry the resolved internal subscriber id. When Novu includes the subscriber on a message, that subscriber must
// carry the external id as well. One message that fails either check refuses the whole page.
func ownedErasureMessageIDs(messages []components.MessageResponseDto, subscriberID, internalID string) ([]string, error) {
	ids := make([]string, 0, len(messages))
	for _, message := range messages {
		if message.ID == nil || strings.TrimSpace(*message.ID) == "" {
			return nil, errors.New("novu subscriber message has no id")
		}
		if message.SubscriberID != internalID {
			return nil, errors.New("novu listed a message for another subscriber: erasure deletes nothing on this page")
		}
		if message.Subscriber != nil && message.Subscriber.SubscriberID != subscriberID {
			return nil, errors.New("novu listed a message whose subscriber is another subscriber: erasure deletes nothing on this page")
		}
		ids = append(ids, *message.ID)
	}
	return ids, nil
}

// isNovuNotFound reports whether Novu answered 404 with its own JSON error body. Only that answer means the record is
// already gone. A 404 without Novu's body, such as one from a misrouted base URL, is not treated as success, so it retries
// instead of letting the local purge run while a provider record may still exist.
func isNovuNotFound(err error) bool {
	var dto *apierrors.ErrorDto
	if errors.As(err, &dto) {
		return dto.StatusCode == 404
	}
	return false
}
