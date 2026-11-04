package webpush

import (
	"context"
	"crypto/ecdh"
	"crypto/rand"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// browserStandIn is a receiver with a real key pair and auth secret, as a browser would hold them after
// PushManager.subscribe().
type browserStandIn struct {
	key        *ecdh.PrivateKey
	authSecret []byte
	p256dh     string
	auth       string
}

func newBrowserStandIn(t *testing.T) browserStandIn {
	t.Helper()
	key, err := ecdh.P256().GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey() error = %v", err)
	}
	authSecret := make([]byte, 16)
	if _, err := rand.Read(authSecret); err != nil {
		t.Fatalf("rand.Read() error = %v", err)
	}
	return browserStandIn{
		key:        key,
		authSecret: authSecret,
		p256dh:     encodeBase64URL(key.PublicKey().Bytes()),
		auth:       encodeBase64URL(authSecret),
	}
}

type capturedPush struct {
	header http.Header
	body   []byte
}

func newCapturingServer(t *testing.T, status int) (*httptest.Server, <-chan capturedPush) {
	t.Helper()
	captured := make(chan capturedPush, 4)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		captured <- capturedPush{header: r.Header.Clone(), body: body}
		w.WriteHeader(status)
	}))
	t.Cleanup(server.Close)
	return server, captured
}

func TestSendPostsAnEncryptedWakeUpWithVAPIDHeaders(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	browser := newBrowserStandIn(t)
	server, captured := newCapturingServer(t, http.StatusCreated)

	sender := NewSender(keys, "mailto:ops@example.test", server.Client())
	subscription := Subscription{ID: "wps_1", Endpoint: server.URL + "/send/abc", P256DH: browser.p256dh, Auth: browser.auth}
	if err := sender.Send(context.Background(), subscription, "thread-topic-0123456789abcdefghij"); err != nil {
		t.Fatalf("Send() error = %v", err)
	}

	push := <-captured
	if got := push.header.Get("Content-Encoding"); got != "aes128gcm" {
		t.Fatalf("Content-Encoding = %q, want aes128gcm", got)
	}
	if got := push.header.Get("Urgency"); got != "high" {
		t.Fatalf("Urgency = %q, want high", got)
	}
	if got := push.header.Get("TTL"); got != "3600" {
		t.Fatalf("TTL = %q, want 3600", got)
	}
	if got := push.header.Get("Topic"); got != "thread-topic-0123456789abcdefghij" {
		t.Fatalf("Topic = %q", got)
	}
	authorization := push.header.Get("Authorization")
	if !strings.HasPrefix(authorization, "vapid t=") || !strings.HasSuffix(authorization, ", k="+keys.PublicKey()) {
		t.Fatalf("Authorization = %q, want a vapid token carrying the public key", authorization)
	}
	if got := openWakeUp(t, browser.key, browser.authSecret, push.body); string(got) != wakeUpPayload {
		t.Fatalf("browser read %q, want the constant wake-up %q", got, wakeUpPayload)
	}
}

func TestSendReportsAGoneEndpointAsRevocable(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	browser := newBrowserStandIn(t)

	for _, status := range []int{http.StatusGone, http.StatusNotFound} {
		server, _ := newCapturingServer(t, status)
		sender := NewSender(keys, "mailto:ops@example.test", server.Client())
		subscription := Subscription{ID: "wps_gone", Endpoint: server.URL + "/gone", P256DH: browser.p256dh, Auth: browser.auth}
		if err := sender.Send(context.Background(), subscription, ""); !errors.Is(err, ErrSubscriptionGone) {
			t.Fatalf("status %d: Send() error = %v, want ErrSubscriptionGone", status, err)
		}
	}
}

func TestSendNeverEchoesTheEndpointInItsErrors(t *testing.T) {
	publicKey, privateKey := newTestVAPID(t)
	keys, err := LoadVAPID(publicKey, privateKey)
	if err != nil {
		t.Fatalf("LoadVAPID() error = %v", err)
	}
	browser := newBrowserStandIn(t)

	server, _ := newCapturingServer(t, http.StatusInternalServerError)
	endpoint := server.URL + "/secret-capability"
	sender := NewSender(keys, "mailto:ops@example.test", server.Client())
	err = sender.Send(context.Background(), Subscription{ID: "wps_5xx", Endpoint: endpoint, P256DH: browser.p256dh, Auth: browser.auth}, "")
	if err == nil || errors.Is(err, ErrSubscriptionGone) {
		t.Fatalf("Send() error = %v, want a retryable failure", err)
	}
	if strings.Contains(err.Error(), "secret-capability") {
		t.Fatalf("error %q carries the endpoint", err)
	}

	server.Close()
	err = sender.Send(context.Background(), Subscription{ID: "wps_down", Endpoint: endpoint, P256DH: browser.p256dh, Auth: browser.auth}, "")
	if err == nil {
		t.Fatal("Send() to a closed server error = nil")
	}
	if strings.Contains(err.Error(), "secret-capability") || strings.Contains(err.Error(), server.URL) {
		t.Fatalf("transport error %q carries the endpoint", err)
	}
}
