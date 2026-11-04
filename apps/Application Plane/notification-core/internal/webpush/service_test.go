package webpush

import (
	"context"
	"errors"
	"strings"
	"testing"
)

// recordingSubscriptionStore records what reaches storage, so a test can show that a refused subscription never did.
type recordingSubscriptionStore struct {
	upserts []string
}

func (f *recordingSubscriptionStore) Upsert(_ context.Context, organizationID, userID string, subscription Subscription, serviceClass string) (string, error) {
	f.upserts = append(f.upserts, organizationID+"|"+userID+"|"+serviceClass+"|"+subscription.Endpoint)
	return "wps_stored", nil
}

func (f *recordingSubscriptionStore) ListActive(context.Context, string, string, int) ([]Subscription, error) {
	return nil, errors.New("not used by the service")
}

func (f *recordingSubscriptionStore) Revoke(_ context.Context, organizationID, userID, id string) error {
	if id == "wps_missing" {
		return ErrSubscriptionNotFound
	}
	return nil
}

func (f *recordingSubscriptionStore) MarkDelivered(context.Context, string, string, string) error {
	return nil
}

func newTestService(t *testing.T, store subscriptionStore) *Service {
	t.Helper()
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	return NewService(keys, store)
}

func validInput(t *testing.T, endpoint string) SubscriptionInput {
	t.Helper()
	browser := newBrowserStandIn(t)
	var input SubscriptionInput
	input.Endpoint = endpoint
	input.Keys.P256DH = browser.p256dh
	input.Keys.Auth = browser.auth
	return input
}

func TestValidateEndpointAcceptsOnlyHTTPSOrigins(t *testing.T) {
	for _, endpoint := range []string{
		"https://fcm.googleapis.com/fcm/send/abc",
		"https://updates.push.services.mozilla.com/wpush/v2/xyz",
	} {
		if err := validateEndpoint(endpoint); err != nil {
			t.Fatalf("validateEndpoint(%q) = %v, want accepted", endpoint, err)
		}
	}
	for _, endpoint := range []string{
		"",
		"http://push.example.net/send",
		"https://",
		"https://user:secret@push.example.net/send",
		"ftp://push.example.net/send",
		"https://push.example.net/" + strings.Repeat("a", maxEndpointBytes),
		"https://push.example.net/send",
		"https://10.0.0.5/send",
		"https://fcm.googleapis.com:8443/fcm/send/abc",
		"https://fcm.googleapis.com.attacker.net/send",
	} {
		if err := validateEndpoint(endpoint); !errors.Is(err, ErrInvalidSubscription) {
			t.Fatalf("validateEndpoint(%q) = %v, want ErrInvalidSubscription", endpoint, err)
		}
	}
}

func TestClassifyEndpointNamesThePushServiceWithoutKeepingTheEndpoint(t *testing.T) {
	cases := map[string]string{
		"https://fcm.googleapis.com/fcm/send/abc":                "google",
		"https://updates.push.services.mozilla.com/wpush/v2/xyz": "mozilla",
		"https://web.push.apple.com/QabcDEF":                     "apple",
		"https://wns2-db5p.notify.windows.com/w/?token=abc":      "windows",
		"https://push.example.net/send/abc":                      "other",
	}
	for endpoint, want := range cases {
		if got := classifyEndpoint(endpoint); got != want {
			t.Fatalf("classifyEndpoint(%q) = %q, want %q", endpoint, got, want)
		}
	}
}

func TestSubscribeStoresAValidSubscriptionForTheCallersOwnScope(t *testing.T) {
	store := &recordingSubscriptionStore{}
	service := newTestService(t, store)

	id, err := service.Subscribe(context.Background(), "org-1", "user-1", validInput(t, "https://fcm.googleapis.com/fcm/send/abc"))
	if err != nil {
		t.Fatalf("Subscribe() error = %v", err)
	}
	if id != "wps_stored" {
		t.Fatalf("id = %q, want the stored id", id)
	}
	if len(store.upserts) != 1 || store.upserts[0] != "org-1|user-1|google|https://fcm.googleapis.com/fcm/send/abc" {
		t.Fatalf("stored = %v, want one row for org-1 and user-1", store.upserts)
	}
}

func TestSubscribeRefusesInvalidInputBeforeAnythingIsStored(t *testing.T) {
	store := &recordingSubscriptionStore{}
	service := newTestService(t, store)

	badEndpoint := validInput(t, "http://push.example.net/send")
	if _, err := service.Subscribe(context.Background(), "org-1", "user-1", badEndpoint); !errors.Is(err, ErrInvalidSubscription) {
		t.Fatalf("Subscribe(http endpoint) error = %v, want ErrInvalidSubscription", err)
	}

	badKeys := validInput(t, "https://fcm.googleapis.com/fcm/send/abc")
	badKeys.Keys.Auth = "AAAA"
	if _, err := service.Subscribe(context.Background(), "org-1", "user-1", badKeys); !errors.Is(err, ErrInvalidSubscription) {
		t.Fatalf("Subscribe(short auth) error = %v, want ErrInvalidSubscription", err)
	}

	if len(store.upserts) != 0 {
		t.Fatalf("refused subscriptions reached storage: %v", store.upserts)
	}
}

func TestUnsubscribeReportsAnUnknownSubscription(t *testing.T) {
	service := newTestService(t, &recordingSubscriptionStore{})

	if err := service.Unsubscribe(context.Background(), "org-1", "user-1", "wps_missing"); !errors.Is(err, ErrSubscriptionNotFound) {
		t.Fatalf("Unsubscribe(missing) error = %v, want ErrSubscriptionNotFound", err)
	}
	if err := service.Unsubscribe(context.Background(), "org-1", "user-1", "wps_mine"); err != nil {
		t.Fatalf("Unsubscribe(own) error = %v", err)
	}
}
