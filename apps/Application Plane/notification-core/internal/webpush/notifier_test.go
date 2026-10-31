package webpush

import (
	"context"
	"encoding/base64"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/notification"
)

// fakeSubscriptionStore records every lookup and write, keyed by organization and user, so a test can show which
// recipient the notifier reached.
type fakeSubscriptionStore struct {
	mu            sync.Mutex
	subscriptions map[string][]Subscription
	lookups       []string
	revoked       []string
	delivered     []string
}

func recipientKey(organizationID, userID string) string {
	return organizationID + "|" + userID
}

func (f *fakeSubscriptionStore) Upsert(context.Context, string, string, Subscription, string) (string, error) {
	return "", errors.New("the notifier never stores subscriptions")
}

func (f *fakeSubscriptionStore) ListActive(_ context.Context, organizationID, userID string, _ int) ([]Subscription, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	key := recipientKey(organizationID, userID)
	f.lookups = append(f.lookups, key)
	return append([]Subscription(nil), f.subscriptions[key]...), nil
}

func (f *fakeSubscriptionStore) Revoke(_ context.Context, organizationID, userID, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.revoked = append(f.revoked, recipientKey(organizationID, userID)+"|"+id)
	return nil
}

func (f *fakeSubscriptionStore) MarkDelivered(_ context.Context, organizationID, userID, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.delivered = append(f.delivered, recipientKey(organizationID, userID)+"|"+id)
	return nil
}

// pathServer answers each path with the status its name asks for, so one test server stands in for several push
// services. Each path is recorded once it has been hit.
func pathServer(t *testing.T) (*httptest.Server, *sync.Mutex, map[string]int) {
	t.Helper()
	var mu sync.Mutex
	hits := map[string]int{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		hits[r.URL.Path]++
		mu.Unlock()
		if strings.HasSuffix(r.URL.Path, "gone") {
			w.WriteHeader(http.StatusGone)
			return
		}
		w.WriteHeader(http.StatusCreated)
	}))
	t.Cleanup(server.Close)
	return server, &mu, hits
}

func TestFanOutWakesOnlyTheRecipientsOwnSubscriptions(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	server, mu, hits := pathServer(t)
	browser := newBrowserStandIn(t)
	endpoint := func(path string) Subscription {
		return Subscription{ID: "wps_" + strings.Trim(path, "/"), Endpoint: server.URL + path, P256DH: browser.p256dh, Auth: browser.auth}
	}
	store := &fakeSubscriptionStore{subscriptions: map[string][]Subscription{
		recipientKey("org-1", "user-1"): {endpoint("/a")},
		recipientKey("org-1", "user-2"): {endpoint("/b")},
		recipientKey("org-2", "user-1"): {endpoint("/c")},
		recipientKey("org-1", "user-3"): {endpoint("/gone")},
	}}
	notifier := NewNotifier(store, NewSender(keys, "mailto:ops@example.test", server.Client()))

	notifier.fanOut(context.Background(), notification.PushNotice{
		OrganizationID: "org-1",
		RecipientID:    "user-1",
		Type:           "modelplane.needs_approval",
		ThreadID:       "thread-9",
	})

	mu.Lock()
	defer mu.Unlock()
	if hits["/a"] != 1 || hits["/b"] != 0 || hits["/c"] != 0 || hits["/gone"] != 0 {
		t.Fatalf("push hits = %v, want only the recipient's own endpoint", hits)
	}
	if len(store.lookups) != 1 || store.lookups[0] != recipientKey("org-1", "user-1") {
		t.Fatalf("lookups = %v, want one lookup scoped to org-1 and user-1", store.lookups)
	}
	if len(store.delivered) != 1 || store.delivered[0] != recipientKey("org-1", "user-1")+"|wps_a" {
		t.Fatalf("delivered = %v, want the success recorded for the recipient only", store.delivered)
	}
}

func TestFanOutRevokesAGoneEndpointForTheSameRecipient(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	server, _, _ := pathServer(t)
	browser := newBrowserStandIn(t)
	store := &fakeSubscriptionStore{subscriptions: map[string][]Subscription{
		recipientKey("org-1", "user-3"): {{ID: "wps_gone", Endpoint: server.URL + "/gone", P256DH: browser.p256dh, Auth: browser.auth}},
	}}
	notifier := NewNotifier(store, NewSender(keys, "mailto:ops@example.test", server.Client()))

	notifier.fanOut(context.Background(), notification.PushNotice{OrganizationID: "org-1", RecipientID: "user-3", Type: "modelplane.plan_ready"})

	if len(store.revoked) != 1 || store.revoked[0] != recipientKey("org-1", "user-3")+"|wps_gone" {
		t.Fatalf("revoked = %v, want the gone endpoint revoked under its own recipient", store.revoked)
	}
	if len(store.delivered) != 0 {
		t.Fatalf("delivered = %v, want no success for a gone endpoint", store.delivered)
	}
}

func TestTopicForIsAStableBase64URLPrefixOfTheThreadHash(t *testing.T) {
	if got := topicFor(""); got != "" {
		t.Fatalf("topicFor(\"\") = %q, want no topic", got)
	}
	topic := topicFor("thread-9")
	if len(topic) != 32 {
		t.Fatalf("topic length = %d, want 32 (the Web Push limit)", len(topic))
	}
	if _, err := base64.RawURLEncoding.DecodeString(topic); err != nil {
		t.Fatalf("topic %q is not base64url: %v", topic, err)
	}
	if topic != topicFor("thread-9") {
		t.Fatal("topic is not stable for one thread")
	}
	if topic == topicFor("thread-10") {
		t.Fatal("two threads share a topic")
	}
	if strings.Contains(topic, "thread-9") {
		t.Fatal("topic carries the thread id in clear")
	}
}
