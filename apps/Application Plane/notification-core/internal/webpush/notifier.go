package webpush

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"log"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// maxSubscriptionsPerRecipient bounds the fan-out of one notice. A recipient with more browsers than this is woken
// on the most recently registered ones.
const maxSubscriptionsPerRecipient = 10

// sendTimeout bounds one push, so a slow push service cannot hold the whole fan-out.
const sendTimeout = 10 * time.Second

// Notifier wakes every active subscription of one recipient. It implements notification.PushNotifier.
type Notifier struct {
	store  subscriptionStore
	sender *Sender
}

// NewNotifier pairs the subscription store with the sender.
func NewNotifier(store subscriptionStore, sender *Sender) *Notifier {
	return &Notifier{store: store, sender: sender}
}

// NotifyPush returns at once: the lookup and the sends run in the background, detached from the caller's
// cancellation, so a slow push service never delays the accepted notification. It is best-effort. A failure is
// logged without the endpoint, and it never changes what the caller already accepted.
func (n *Notifier) NotifyPush(ctx context.Context, notice notification.PushNotice) {
	go n.fanOut(context.WithoutCancel(ctx), notice)
}

func (n *Notifier) fanOut(ctx context.Context, notice notification.PushNotice) {
	subscriptions, err := n.store.ListActive(ctx, notice.OrganizationID, notice.RecipientID, maxSubscriptionsPerRecipient)
	if err != nil {
		log.Printf("notification-core: web push lookup failed for %s: %v", notice.Type, err)
		return
	}
	topic := topicFor(notice.ThreadID)
	for _, subscription := range subscriptions {
		n.wake(ctx, notice, subscription, topic)
	}
}

func (n *Notifier) wake(ctx context.Context, notice notification.PushNotice, subscription Subscription, topic string) {
	sendCtx, cancel := context.WithTimeout(ctx, sendTimeout)
	defer cancel()
	err := n.sender.Send(sendCtx, subscription, topic)
	switch {
	case err == nil:
		if markErr := n.store.MarkDelivered(ctx, notice.OrganizationID, notice.RecipientID, subscription.ID); markErr != nil {
			log.Printf("notification-core: web push delivery mark failed for %s: %v", subscription.ID, markErr)
		}
	case errors.Is(err, ErrSubscriptionGone):
		if revokeErr := n.store.Revoke(ctx, notice.OrganizationID, notice.RecipientID, subscription.ID); revokeErr != nil && !errors.Is(revokeErr, ErrSubscriptionNotFound) {
			log.Printf("notification-core: web push revoke failed for %s: %v", subscription.ID, revokeErr)
		}
		log.Printf("notification-core: web push subscription %s is gone and was revoked", subscription.ID)
	default:
		log.Printf("notification-core: web push to subscription %s failed: %v", subscription.ID, err)
	}
}

// topicFor derives the Web Push Topic from the thread: a 32-character base64url prefix of its hash. The push
// service then replaces an undelivered wake-up for the same thread, and it never learns the thread id.
func topicFor(threadID string) string {
	if threadID == "" {
		return ""
	}
	digest := sha256.Sum256([]byte(threadID))
	return base64.RawURLEncoding.EncodeToString(digest[:])[:32]
}
