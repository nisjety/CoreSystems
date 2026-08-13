package egress

import (
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strconv"
	"testing"
	"time"
)

// TestSafeClientDialsTheVettedAddress proves the address ResolveAndVet
// approves is the address the transport actually connects to, not a second,
// independently resolved one. The stub resolver maps a hostname that looks
// like any other remote name to a real local listener's address; because a
// fake TLD like ".example.test" has no real DNS answer, a second, real
// lookup performed anywhere downstream would fail outright rather than
// quietly substituting a different address — so success here is only
// possible if the pinned address from ResolveAndVet was the one dialed.
func TestSafeClientDialsTheVettedAddress(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen: %v", err)
	}
	defer listener.Close()
	server := &http.Server{Handler: http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})}
	go server.Serve(listener)
	defer server.Close()

	tcpAddr := listener.Addr().(*net.TCPAddr)
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"pinned.example.test": {netip.MustParseAddr(tcpAddr.IP.String())},
	}}
	// The listener binds to loopback, which default policy blocks; allowlist
	// the hostname explicitly, matching WithAllowlist's intended use: naming
	// one confirmed-legitimate destination rather than relaxing the policy.
	guard := New(WithResolver(stub), WithAllowlist([]string{"pinned.example.test"}, nil))
	client := SafeClient(ClientConfig{Guard: guard, ConnectTimeout: 2 * time.Second, RequestTimeout: 2 * time.Second})

	resp, err := client.Get("http://pinned.example.test:" + strconv.Itoa(tcpAddr.Port) + "/")
	if err != nil {
		t.Fatalf("expected the request to reach the pinned address, got %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusNoContent {
		t.Fatalf("expected 204 from the pinned server, got %d", resp.StatusCode)
	}
	if stub.calls == 0 {
		t.Fatal("expected the stub resolver to have been consulted")
	}
}

// TestSafeClientBlocksRebindToPrivateAddress is the end-to-end version of the
// rebinding proof: a hostname resolves to loopback, and a real SafeClient
// (not just ResolveAndVet in isolation) must fail the request closed rather
// than connect.
func TestSafeClientBlocksRebindToPrivateAddress(t *testing.T) {
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"rebind.example.test": {netip.MustParseAddr("127.0.0.1")},
	}}
	client := SafeClient(ClientConfig{
		Guard:          New(WithResolver(stub)),
		ConnectTimeout: time.Second,
		RequestTimeout: time.Second,
	})
	_, err := client.Get("http://rebind.example.test:9/")
	if err == nil {
		t.Fatal("expected the rebind attempt to fail")
	}
	if !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected an ErrBlocked-wrapped error, got %v", err)
	}
}

// TestSafeClientDefaultRefusesRedirects proves the default (AllowRedirects:
// false) configuration never follows a redirect at all: the call-site
// survey behind this package found no provider call that needs one
// followed, so the safest default is not to negotiate the question per
// request.
func TestSafeClientDefaultRefusesRedirects(t *testing.T) {
	blockedTarget := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Error("the redirect target must never be dialed when redirects are disabled")
	}))
	defer blockedTarget.Close()

	redirector := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, blockedTarget.URL, http.StatusFound)
	}))
	defer redirector.Close()

	// Both httptest servers bind to loopback; allowlist it so the test
	// exercises redirect-following behavior specifically, not vetting.
	guard := New(WithAllowlist([]string{"127.0.0.1"}, nil))
	client := SafeClient(ClientConfig{Guard: guard, ConnectTimeout: time.Second, RequestTimeout: 2 * time.Second})

	resp, err := client.Get(redirector.URL)
	if err != nil {
		t.Fatalf("expected the un-followed redirect response itself, not an error: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusFound {
		t.Fatalf("expected the 302 to be returned un-followed, got %d", resp.StatusCode)
	}
}

// TestSafeClientRevetsEveryRedirectHop proves that when a call site opts
// into following redirects, a hop landing on a blocked destination is
// refused rather than silently followed — "a redirect test proving an
// off-vet redirect is refused."
func TestSafeClientRevetsEveryRedirectHop(t *testing.T) {
	target := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Error("a redirect hop that fails re-vetting must never be dialed")
	}))
	defer target.Close()
	targetPort := target.Listener.Addr().(*net.TCPAddr).Port

	redirector := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "http://blocked-hop.example.test:"+strconv.Itoa(targetPort)+"/", http.StatusFound)
	}))
	defer redirector.Close()
	redirectorIP := redirector.Listener.Addr().(*net.TCPAddr).IP.String()

	stub := &stubResolver{answers: map[string][]netip.Addr{
		// The redirect Location's host resolves to loopback via the stub —
		// standing in for a provider response or DNS answer steering the
		// hop somewhere it should not go.
		"blocked-hop.example.test": {netip.MustParseAddr("127.0.0.1")},
	}}
	// Allowlist ONLY the redirector's own address (needed to reach the test
	// rig at all); "blocked-hop.example.test" is deliberately left off, so
	// its resolution to loopback is refused by CheckRedirect.
	guard := New(WithResolver(stub), WithAllowlist([]string{redirectorIP}, nil))
	client := SafeClient(ClientConfig{
		Guard:          guard,
		AllowRedirects: true,
		ConnectTimeout: time.Second,
		RequestTimeout: 2 * time.Second,
	})

	_, err := client.Get(redirector.URL)
	if !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected the redirect hop to be refused with ErrBlocked, got %v", err)
	}
}

// TestSafeClientRevetAllowsALegitimateRedirectHop is the positive
// counterpart: with AllowRedirects enabled, a hop to a permitted destination
// is followed normally.
func TestSafeClientRevetAllowsALegitimateRedirectHop(t *testing.T) {
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	defer target.Close()

	redirector := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusFound)
	}))
	defer redirector.Close()

	guard := New(WithAllowlist([]string{"127.0.0.1"}, nil))
	client := SafeClient(ClientConfig{
		Guard:          guard,
		AllowRedirects: true,
		ConnectTimeout: time.Second,
		RequestTimeout: 2 * time.Second,
	})

	resp, err := client.Get(redirector.URL)
	if err != nil {
		t.Fatalf("expected the legitimate redirect to be followed, got %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusNoContent {
		t.Fatalf("expected 204 from the redirect target, got %d", resp.StatusCode)
	}
}

func TestSafeClientAppliesDefaultsWhenConfigIsZeroValue(t *testing.T) {
	client := SafeClient(ClientConfig{Guard: New()})
	if client.Timeout != defaultRequestTimeout {
		t.Fatalf("expected default request timeout %s, got %s", defaultRequestTimeout, client.Timeout)
	}
	if client.CheckRedirect == nil {
		t.Fatal("expected a CheckRedirect to be set even with a zero-value config")
	}
}
