package http

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	stdhttp "net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/config"
	"github.com/I-Dacosta/AquatiqCMS/apps/notification-core/internal/webpush"
)

// fakePushSubscriptions keeps subscriptions per organization and user, so a test can show that one user's
// endpoint answers as not found to another user.
type fakePushSubscriptions struct {
	mu    sync.Mutex
	owned map[string]string
}

func newFakePushSubscriptions() *fakePushSubscriptions {
	return &fakePushSubscriptions{owned: map[string]string{}}
}

func (f *fakePushSubscriptions) PublicKey() string {
	return "BTestApplicationServerKey"
}

func (f *fakePushSubscriptions) Subscribe(_ context.Context, organizationID, userID string, input webpush.SubscriptionInput) (string, error) {
	if !strings.HasPrefix(input.Endpoint, "https://") {
		return "", webpush.ErrInvalidSubscription
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	id := "wps_" + strconv.Itoa(len(f.owned)+1)
	f.owned[id] = organizationID + "|" + userID
	return id, nil
}

func (f *fakePushSubscriptions) Unsubscribe(_ context.Context, organizationID, userID, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.owned[id] != organizationID+"|"+userID {
		return webpush.ErrSubscriptionNotFound
	}
	delete(f.owned, id)
	return nil
}

type allowAllRecipients struct{}

func (allowAllRecipients) ResolveUser(context.Context, string, string) (string, error) {
	return "provider-subscriber", nil
}

func pushServer(t *testing.T, push PushSubscriptions, recipients RecipientAuthorizer) *httptest.Server {
	t.Helper()
	handler := NewHandler(&config.Config{ServiceName: "notification-core"}, HandlerDeps{Push: push, Recipients: recipients})
	server := httptest.NewServer(newRouter(handler, newTestVerifier(t)))
	t.Cleanup(server.Close)
	return server
}

// signedPushRequest is a gateway-delegated request for one user, the only shape the BFF sends.
func signedPushRequest(t *testing.T, method, url string, body []byte, userID string) *stdhttp.Request {
	t.Helper()
	request, err := stdhttp.NewRequest(method, url, bytes.NewReader(body))
	if err != nil {
		t.Fatalf("NewRequest() error = %v", err)
	}
	request.Header.Set("Content-Type", "application/json")
	signTestRequest(t, request, body, testDelegationSecret, userID, "org-1", "member")
	return request
}

func doRequest(t *testing.T, request *stdhttp.Request) (int, []byte) {
	t.Helper()
	response, err := stdhttp.DefaultClient.Do(request)
	if err != nil {
		t.Fatalf("%s %s error = %v", request.Method, request.URL.Path, err)
	}
	defer response.Body.Close()
	payload, _ := io.ReadAll(response.Body)
	return response.StatusCode, payload
}

func TestPushConfigAnswersDisabledWhileWebPushIsOff(t *testing.T) {
	server := pushServer(t, nil, allowAllRecipients{})

	status, payload := doRequest(t, signedPushRequest(t, stdhttp.MethodGet, server.URL+"/push/config", nil, "user-1"))
	if status != stdhttp.StatusOK {
		t.Fatalf("GET /push/config status = %d, want 200", status)
	}
	var body map[string]any
	if err := json.Unmarshal(payload, &body); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if body["enabled"] != false {
		t.Fatalf("body = %s, want enabled false", payload)
	}
	if _, ok := body["public_key"]; ok {
		t.Fatalf("body = %s, want no key while disabled", payload)
	}
}

func TestPushConfigPublishesOnlyThePublicKey(t *testing.T) {
	server := pushServer(t, newFakePushSubscriptions(), allowAllRecipients{})

	status, payload := doRequest(t, signedPushRequest(t, stdhttp.MethodGet, server.URL+"/push/config", nil, "user-1"))
	if status != stdhttp.StatusOK {
		t.Fatalf("GET /push/config status = %d, want 200", status)
	}
	var body struct {
		Enabled   bool   `json:"enabled"`
		PublicKey string `json:"public_key"`
	}
	if err := json.Unmarshal(payload, &body); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if !body.Enabled || body.PublicKey != "BTestApplicationServerKey" {
		t.Fatalf("body = %s, want the enabled public key", payload)
	}
}

func TestPushSubscriptionRoutesAreUnavailableWhileWebPushIsOff(t *testing.T) {
	server := pushServer(t, nil, allowAllRecipients{})
	body := []byte(`{"endpoint":"https://push.example.net/send/abc","keys":{"p256dh":"x","auth":"y"}}`)

	status, _ := doRequest(t, signedPushRequest(t, stdhttp.MethodPost, server.URL+"/push/subscriptions", body, "user-1"))
	if status != stdhttp.StatusServiceUnavailable {
		t.Fatalf("POST /push/subscriptions status = %d, want 503 while disabled", status)
	}
	status, _ = doRequest(t, signedPushRequest(t, stdhttp.MethodDelete, server.URL+"/push/subscriptions/wps_1", nil, "user-1"))
	if status != stdhttp.StatusServiceUnavailable {
		t.Fatalf("DELETE /push/subscriptions/{id} status = %d, want 503 while disabled", status)
	}
}

func TestPushSubscriptionIsStoredForTheDelegatedUserAndRefusedWhenInvalid(t *testing.T) {
	push := newFakePushSubscriptions()
	server := pushServer(t, push, allowAllRecipients{})

	valid := []byte(`{"endpoint":"https://push.example.net/send/abc","keys":{"p256dh":"x","auth":"y"}}`)
	status, payload := doRequest(t, signedPushRequest(t, stdhttp.MethodPost, server.URL+"/push/subscriptions", valid, "user-1"))
	if status != stdhttp.StatusCreated {
		t.Fatalf("POST /push/subscriptions status = %d, want 201", status)
	}
	if !strings.Contains(string(payload), `"id":"wps_1"`) {
		t.Fatalf("body = %s, want the stored id", payload)
	}
	if push.owned["wps_1"] != "org-1|user-1" {
		t.Fatalf("stored for %q, want the delegated organization and user", push.owned["wps_1"])
	}

	invalid := []byte(`{"endpoint":"http://push.example.net/send/abc","keys":{"p256dh":"x","auth":"y"}}`)
	status, _ = doRequest(t, signedPushRequest(t, stdhttp.MethodPost, server.URL+"/push/subscriptions", invalid, "user-1"))
	if status != stdhttp.StatusBadRequest {
		t.Fatalf("POST /push/subscriptions with an http endpoint status = %d, want 400", status)
	}
}

func TestPushSubscriptionCannotBeRevokedByAnotherUser(t *testing.T) {
	push := newFakePushSubscriptions()
	server := pushServer(t, push, allowAllRecipients{})
	body := []byte(`{"endpoint":"https://push.example.net/send/abc","keys":{"p256dh":"x","auth":"y"}}`)
	if status, _ := doRequest(t, signedPushRequest(t, stdhttp.MethodPost, server.URL+"/push/subscriptions", body, "user-1")); status != stdhttp.StatusCreated {
		t.Fatalf("setup POST status = %d, want 201", status)
	}

	status, _ := doRequest(t, signedPushRequest(t, stdhttp.MethodDelete, server.URL+"/push/subscriptions/wps_1", nil, "user-2"))
	if status != stdhttp.StatusNotFound {
		t.Fatalf("another user's DELETE status = %d, want 404", status)
	}
	status, _ = doRequest(t, signedPushRequest(t, stdhttp.MethodDelete, server.URL+"/push/subscriptions/wps_1", nil, "user-1"))
	if status != stdhttp.StatusNoContent {
		t.Fatalf("the owner's DELETE status = %d, want 204", status)
	}
}

func TestPushRoutesRequireAnActiveMembership(t *testing.T) {
	server := pushServer(t, newFakePushSubscriptions(), deniedRecipientAuthorizer{})

	status, _ := doRequest(t, signedPushRequest(t, stdhttp.MethodGet, server.URL+"/push/config", nil, "user-1"))
	if status != stdhttp.StatusForbidden {
		t.Fatalf("GET /push/config without membership status = %d, want 403", status)
	}
}
