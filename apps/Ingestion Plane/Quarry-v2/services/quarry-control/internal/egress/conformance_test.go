package egress

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

// The tests in this file are the quarry-control conformance set for the
// webhook dispatcher and the catalog-watch client (E3.5-2). They reuse the
// stubResolver from guard_test.go and pin the three properties the two
// one-line anchors depend on: a registered webhook target that is a loopback
// or private address is refused before the request is sent, a hostname that
// rebinds to a private address is refused, and the fallback client keeps the
// 30 second request timeout with redirects not followed.

// TestConformanceWebhookLoopbackTargetRefused registers a live loopback
// server as a webhook target. The server is reachable, so only the guard
// stands between the dispatcher and the request. The request must fail with
// ErrBlocked and the server must never see it.
func TestConformanceWebhookLoopbackTargetRefused(t *testing.T) {
	var hits int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		atomic.AddInt32(&hits, 1)
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	client := SafeClient(ClientConfig{
		Guard:          New(WithResolver(&stubResolver{answers: map[string][]netip.Addr{}})),
		RequestTimeout: 30 * time.Second,
	})
	req, err := http.NewRequestWithContext(context.Background(), http.MethodPost, server.URL+"/hook", strings.NewReader(`{"event":"test"}`))
	if err != nil {
		t.Fatalf("build request: %v", err)
	}
	resp, err := client.Do(req)
	if resp != nil {
		resp.Body.Close()
	}
	if !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected a loopback webhook target to be refused with ErrBlocked, got %v", err)
	}
	if got := atomic.LoadInt32(&hits); got != 0 {
		t.Fatalf("expected the loopback server to see no request, it saw %d", got)
	}
}

// TestConformanceWebhookRebindingHostnameRefused is the tenant-registered
// hostname case: a name that looks public resolves to a private address, as
// a rebinding DNS answer would. The lookup happens, and the connection is
// refused on the resolved address.
func TestConformanceWebhookRebindingHostnameRefused(t *testing.T) {
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"hooks.tenant.example.test": {netip.MustParseAddr("10.20.30.40")},
	}}
	client := SafeClient(ClientConfig{
		Guard:          New(WithResolver(stub)),
		RequestTimeout: 30 * time.Second,
	})
	req, err := http.NewRequestWithContext(context.Background(), http.MethodPost, "https://hooks.tenant.example.test/in", strings.NewReader(`{}`))
	if err != nil {
		t.Fatalf("build request: %v", err)
	}
	resp, err := client.Do(req)
	if resp != nil {
		resp.Body.Close()
	}
	if !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected a hostname resolving to a private address to be refused with ErrBlocked, got %v", err)
	}
	if stub.calls == 0 {
		t.Fatal("expected the hostname to be resolved and vetted, the resolver was never consulted")
	}
}

// TestConformanceWebhookMetadataHostnameRefusedBeforeLookup covers the cloud
// metadata names a tenant could register as a webhook target. They are
// refused before the resolver runs, the same as the integration-core guard.
func TestConformanceWebhookMetadataHostnameRefusedBeforeLookup(t *testing.T) {
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"metadata.google.internal": {netip.MustParseAddr("8.8.8.8")},
	}}
	client := SafeClient(ClientConfig{
		Guard:          New(WithResolver(stub)),
		RequestTimeout: 30 * time.Second,
	})
	req, err := http.NewRequestWithContext(context.Background(), http.MethodPost, "http://metadata.google.internal/computeMetadata/v1/", strings.NewReader(`{}`))
	if err != nil {
		t.Fatalf("build request: %v", err)
	}
	resp, err := client.Do(req)
	if resp != nil {
		resp.Body.Close()
	}
	if !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected the metadata hostname to be refused with ErrBlocked, got %v", err)
	}
	if stub.calls != 0 {
		t.Fatalf("expected no lookup for a pre-DNS-blocked metadata hostname, got %d calls", stub.calls)
	}
}

// TestConformanceFallbackClientDefaults pins the shape of the client the
// dispatcher and catalog-watch fall back to when the caller passes nil: the
// 30 second request timeout those call sites used before, and redirects not
// followed, so a webhook target cannot bounce a delivery to another host.
func TestConformanceFallbackClientDefaults(t *testing.T) {
	client := SafeClient(ClientConfig{
		Guard:          New(),
		RequestTimeout: 30 * time.Second,
	})
	if client.Timeout != 30*time.Second {
		t.Fatalf("expected a 30s request timeout, got %s", client.Timeout)
	}
	if client.CheckRedirect == nil {
		t.Fatal("expected CheckRedirect to be set")
	}
	if err := client.CheckRedirect(nil, nil); !errors.Is(err, http.ErrUseLastResponse) {
		t.Fatalf("expected redirects to be refused by returning the last response, got %v", err)
	}
}
