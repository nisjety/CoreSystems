package webpush

import (
	"context"
	"errors"
	"net/url"
	"strings"
)

// ErrInvalidSubscription refuses a subscription that is not an https push endpoint with usable browser keys.
var ErrInvalidSubscription = errors.New("invalid web push subscription")

// ErrSubscriptionNotFound reports a revoke for a subscription the caller does not own.
var ErrSubscriptionNotFound = errors.New("web push subscription not found")

// maxEndpointBytes bounds the endpoint a browser may register.
const maxEndpointBytes = 2048

// SubscriptionInput is the body a browser posts after PushManager.subscribe().
type SubscriptionInput struct {
	Endpoint string `json:"endpoint"`
	Keys     struct {
		P256DH string `json:"p256dh"`
		Auth   string `json:"auth"`
	} `json:"keys"`
}

// Subscription is one active endpoint of one recipient, opened from its sealed form.
type Subscription struct {
	ID       string
	Endpoint string
	P256DH   string
	Auth     string
}

// subscriptionStore is the storage the service and the notifier need. *Store is the production
// implementation; tests substitute a fake without a database.
type subscriptionStore interface {
	Upsert(ctx context.Context, organizationID, userID string, subscription Subscription, serviceClass string) (string, error)
	ListActive(ctx context.Context, organizationID, userID string, limit int) ([]Subscription, error)
	Revoke(ctx context.Context, organizationID, userID, id string) error
	MarkDelivered(ctx context.Context, organizationID, userID, id string) error
}

// Service is the HTTP-facing surface for subscriptions. It validates what a browser registers and stores it
// sealed under the caller's own organization and user.
type Service struct {
	vapid *VAPIDKeys
	store subscriptionStore
}

// NewService pairs the VAPID identity with the subscription store.
func NewService(vapid *VAPIDKeys, store subscriptionStore) *Service {
	return &Service{vapid: vapid, store: store}
}

// PublicKey is the application server key browsers subscribe with.
func (s *Service) PublicKey() string {
	return s.vapid.PublicKey()
}

// Subscribe stores one browser endpoint for one recipient and returns its id. Registering the same endpoint
// again refreshes the stored row and returns the same id.
func (s *Service) Subscribe(ctx context.Context, organizationID, userID string, input SubscriptionInput) (string, error) {
	if err := validateEndpoint(input.Endpoint); err != nil {
		return "", err
	}
	if _, _, err := parseSubscriptionKeys(input.Keys.P256DH, input.Keys.Auth); err != nil {
		return "", err
	}
	return s.store.Upsert(ctx, organizationID, userID, Subscription{
		Endpoint: input.Endpoint,
		P256DH:   input.Keys.P256DH,
		Auth:     input.Keys.Auth,
	}, classifyEndpoint(input.Endpoint))
}

// Unsubscribe revokes one of the caller's own subscriptions.
func (s *Service) Unsubscribe(ctx context.Context, organizationID, userID, id string) error {
	return s.store.Revoke(ctx, organizationID, userID, id)
}

func validateEndpoint(endpoint string) error {
	if endpoint == "" || len(endpoint) > maxEndpointBytes {
		return ErrInvalidSubscription
	}
	parsed, err := url.Parse(endpoint)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" || parsed.User != nil {
		return ErrInvalidSubscription
	}
	// The sender POSTs a signed request to this endpoint, so only the known browser push services are
	// accepted, on the default port; anything else would let a member aim notification-core at any host.
	if parsed.Port() != "" || classifyEndpoint(endpoint) == "other" {
		return ErrInvalidSubscription
	}
	return nil
}

// classifyEndpoint names the push service behind an endpoint. The class is stored instead of the endpoint
// itself, so the table can answer "which browsers are subscribed" without holding the endpoint in clear.
func classifyEndpoint(endpoint string) string {
	parsed, err := url.Parse(endpoint)
	if err != nil {
		return "other"
	}
	host := strings.ToLower(parsed.Hostname())
	switch {
	case strings.HasSuffix(host, ".googleapis.com"):
		return "google"
	case strings.HasSuffix(host, ".mozilla.com"):
		return "mozilla"
	case strings.HasSuffix(host, ".apple.com"):
		return "apple"
	case strings.HasSuffix(host, ".windows.com"):
		return "windows"
	default:
		return "other"
	}
}
