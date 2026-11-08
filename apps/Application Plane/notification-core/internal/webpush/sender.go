package webpush

import (
	"bytes"
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"time"
)

// wakeUpPayload is the only content a push carries: a payload version and a constant kind. The service worker
// shows fixed copy for it, so no conversation text, identifier or tool name leaves notification-core.
const wakeUpPayload = `{"v":1,"k":"attention"}`

// pushTTL is how long a push service may hold an undelivered wake-up. After an hour the waiting item is still in
// the feed, so a later wake-up would only be noise.
const pushTTL = time.Hour

// ErrSubscriptionGone reports that the push service no longer knows the endpoint. The subscription is revoked and
// never retried.
var ErrSubscriptionGone = errors.New("web push subscription is gone")

// Sender posts one encrypted wake-up to one endpoint with a VAPID authorization header.
type Sender struct {
	vapid   *VAPIDKeys
	subject string
	client  *http.Client
	now     func() time.Time
}

// NewSender pairs the VAPID identity with the contact subject and the HTTP client used for pushes.
func NewSender(vapid *VAPIDKeys, subject string, client *http.Client) *Sender {
	return &Sender{vapid: vapid, subject: subject, client: client, now: time.Now}
}

// Send posts the wake-up to one subscription. A non-empty topic lets the push service replace an undelivered
// wake-up for the same thread. Errors never carry the endpoint, because the endpoint is a capability.
func (s *Sender) Send(ctx context.Context, subscription Subscription, topic string) error {
	body, err := encryptWakeUp(subscription.P256DH, subscription.Auth, []byte(wakeUpPayload), rand.Reader)
	if err != nil {
		return fmt.Errorf("encrypt web push: %w", err)
	}
	authorization, err := s.vapid.authorization(subscription.Endpoint, s.subject, s.now())
	if err != nil {
		return fmt.Errorf("sign web push: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, subscription.Endpoint, bytes.NewReader(body))
	if err != nil {
		return errors.New("web push endpoint is invalid")
	}
	request.Header.Set("Content-Encoding", "aes128gcm")
	request.Header.Set("Content-Type", "application/octet-stream")
	request.Header.Set("Authorization", authorization)
	request.Header.Set("TTL", strconv.Itoa(int(pushTTL.Seconds())))
	request.Header.Set("Urgency", "high")
	if topic != "" {
		request.Header.Set("Topic", topic)
	}

	response, err := s.client.Do(request)
	if err != nil {
		return errors.New("web push request failed")
	}
	defer response.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 4096))

	switch {
	case response.StatusCode >= 200 && response.StatusCode < 300:
		return nil
	case response.StatusCode == http.StatusNotFound || response.StatusCode == http.StatusGone:
		return ErrSubscriptionGone
	default:
		return fmt.Errorf("web push endpoint answered %d", response.StatusCode)
	}
}
