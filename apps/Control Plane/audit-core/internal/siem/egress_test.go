package siem

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
)

func TestBlockedAddressTable(t *testing.T) {
	blocked := []string{
		"127.0.0.1", "127.255.255.254", "0.0.0.0", "0.1.2.3",
		"10.0.0.1", "10.255.255.255", "172.16.0.1", "172.31.255.255", "192.168.1.1",
		"169.254.169.254", "169.254.0.1",
		"100.64.0.1", "100.127.255.254",
		"198.18.0.1", "192.0.0.8",
		"224.0.0.1", "239.255.255.250", "240.0.0.1", "255.255.255.255",
		"::1", "::", "fc00::1", "fd12:3456::1", "fe80::1", "fec0::1", "ff02::1", "100::1",
		// IPv4 carried inside IPv6: judged by the address it carries.
		"::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:169.254.169.254", "::ffff:100.64.0.1",
		"64:ff9b::a00:1", "64:ff9b::7f00:1", "2002:7f00:1::1", "2002:a9fe:a9fe::1",
		"2001::1",
	}
	for _, raw := range blocked {
		addr := netip.MustParseAddr(raw)
		if !IsBlockedAddress(addr) {
			t.Errorf("%s must be blocked", raw)
		}
	}
	allowed := []string{
		"8.8.8.8", "1.1.1.1", "93.184.216.34", "172.15.255.255", "172.32.0.1",
		"100.63.255.255", "100.128.0.1", "169.253.255.255", "192.0.1.1", "198.17.255.255", "198.20.0.1",
		"2606:4700:4700::1111", "2001:4860:4860::8888", "::ffff:8.8.8.8", "64:ff9b::808:808", "2002:808:808::1",
	}
	for _, raw := range allowed {
		addr := netip.MustParseAddr(raw)
		if IsBlockedAddress(addr) {
			t.Errorf("%s must be allowed", raw)
		}
	}
}

func TestValidateURLRefusesWhatTheGuardOwns(t *testing.T) {
	good := []string{"https://siem.example.com/ingest", "https://hec.example.com:8088/services/collector/event", "HTTPS://Example.com/a?b=c"}
	for _, raw := range good {
		if _, err := ValidateURL(raw); err != nil {
			t.Errorf("%s refused: %v", raw, err)
		}
	}
	cases := map[string]error{
		"http://siem.example.com/ingest":      ErrURLScheme,
		"ftp://siem.example.com/":             ErrURLScheme,
		"//siem.example.com/ingest":           ErrURLScheme,
		"https://user:pass@siem.example.com/": ErrURLUserinfo,
		"https://user@siem.example.com/":      ErrURLUserinfo,
		"https:///path":                       ErrURLHost,
		"https://:8443/path":                  ErrURLHost,
		"https://exa mple.com/":               ErrURLHost,
		"":                                    ErrURLScheme,
		"https://siem.example.com/\x7f":       ErrURLHost,
	}
	for raw, want := range cases {
		if _, err := ValidateURL(raw); !errors.Is(err, want) {
			t.Errorf("ValidateURL(%q) = %v, want %v", raw, err, want)
		}
	}
}

// fakeResolver answers from a script, so a test can make one name resolve to
// a public address first and a private one next (DNS rebinding).
type fakeResolver struct {
	mu      sync.Mutex
	answers map[string][][]netip.Addr
	calls   map[string]int
}

func (f *fakeResolver) LookupNetIP(_ context.Context, _, host string) ([]netip.Addr, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.calls == nil {
		f.calls = map[string]int{}
	}
	sequence, ok := f.answers[host]
	if !ok || len(sequence) == 0 {
		return nil, errors.New("no such host")
	}
	i := f.calls[host]
	f.calls[host]++
	if i >= len(sequence) {
		i = len(sequence) - 1
	}
	return sequence[i], nil
}

func addrs(values ...string) []netip.Addr {
	out := make([]netip.Addr, len(values))
	for i, v := range values {
		out[i] = netip.MustParseAddr(v)
	}
	return out
}

// dialRecorder counts connection attempts and where they went, and connects
// every one to a local receiver so a test can tell "the guard refused" from
// "nothing was listening".
type dialRecorder struct {
	target string
	dialed []string
	mu     sync.Mutex
}

func (d *dialRecorder) dial(ctx context.Context, network, address string) (net.Conn, error) {
	d.mu.Lock()
	d.dialed = append(d.dialed, address)
	d.mu.Unlock()
	return (&net.Dialer{}).DialContext(ctx, network, d.target)
}

func tlsReceiver(t *testing.T, handler http.Handler) (*httptest.Server, *tls.Config) {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	pool := x509.NewCertPool()
	pool.AddCert(server.Certificate())
	return server, &tls.Config{RootCAs: pool}
}

func TestEgressBlocksEveryPrivateResolutionBeforeConnecting(t *testing.T) {
	receiver, tlsConfig := tlsReceiver(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(200) }))
	cases := map[string][]netip.Addr{
		"loopback.example.com": addrs("127.0.0.1"),
		"rfc1918.example.com":  addrs("10.1.2.3"),
		"metadata.example.com": addrs("169.254.169.254"),
		"cgnat.example.com":    addrs("100.64.0.9"),
		"ula.example.com":      addrs("fd00::5"),
		"mapped.example.com":   addrs("::ffff:192.168.0.1"),
		// One public and one private answer: the private one must still block,
		// or a connect retry would reach it.
		"mixed.example.com": addrs("93.184.216.34", "10.0.0.8"),
	}
	resolver := &fakeResolver{answers: map[string][][]netip.Addr{}}
	for host, answer := range cases {
		resolver.answers[host] = [][]netip.Addr{answer}
	}
	recorder := &dialRecorder{target: strings.TrimPrefix(receiver.URL, "https://")}
	client := NewHTTPClient(Egress{Resolver: resolver, Dialer: recorder.dial, TLSConfig: tlsConfig})
	for host := range cases {
		status, err := Post(context.Background(), client, "https://"+host+"/ingest", nil, []byte("x"))
		if !errors.Is(err, ErrBlockedAddress) || status != 0 {
			t.Errorf("%s: status=%d err=%v, want ErrBlockedAddress", host, status, err)
		}
	}
	if len(recorder.dialed) != 0 {
		t.Fatalf("guard let a dial through to %v", recorder.dialed)
	}
}

func TestEgressBlocksIPLiteralHosts(t *testing.T) {
	recorder := &dialRecorder{target: "127.0.0.1:1"}
	client := NewHTTPClient(Egress{Dialer: recorder.dial})
	for _, target := range []string{"https://127.0.0.1/", "https://[::1]/", "https://169.254.169.254/latest/meta-data", "https://[::ffff:10.0.0.1]/"} {
		if _, err := Post(context.Background(), client, target, nil, nil); !errors.Is(err, ErrBlockedAddress) {
			t.Errorf("%s: %v, want ErrBlockedAddress", target, err)
		}
	}
	if len(recorder.dialed) != 0 {
		t.Fatalf("dialed %v", recorder.dialed)
	}
}

// A name that is public when the destination is saved and private when the
// exporter connects must be caught at connect time: the check is in the dialer,
// on the addresses it is about to use.
func TestEgressChecksAtDialTimeAgainstDNSRebinding(t *testing.T) {
	receiver, tlsConfig := tlsReceiver(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(200) }))
	resolver := &fakeResolver{answers: map[string][][]netip.Addr{
		"rebind.example.com": {addrs("93.184.216.34"), addrs("127.0.0.1")},
	}}
	recorder := &dialRecorder{target: strings.TrimPrefix(receiver.URL, "https://")}
	client := NewHTTPClient(Egress{Resolver: resolver, Dialer: recorder.dial, TLSConfig: tlsConfig})

	status, err := Post(context.Background(), client, "https://rebind.example.com/ingest", nil, []byte("x"))
	if err != nil || status != 200 {
		t.Fatalf("first delivery (public answer): status=%d err=%v", status, err)
	}
	if len(recorder.dialed) != 1 || recorder.dialed[0] != "93.184.216.34:443" {
		t.Fatalf("dialed %v, want the checked public address", recorder.dialed)
	}
	if _, err := Post(context.Background(), client, "https://rebind.example.com/ingest", nil, []byte("x")); !errors.Is(err, ErrBlockedAddress) {
		t.Fatalf("second delivery (answer flipped to loopback): %v, want ErrBlockedAddress", err)
	}
	if len(recorder.dialed) != 1 {
		t.Fatalf("the flipped answer was dialed: %v", recorder.dialed)
	}
}

func TestEgressRefusesPlainHTTPWithoutDialing(t *testing.T) {
	recorder := &dialRecorder{target: "127.0.0.1:1"}
	client := NewHTTPClient(Egress{AllowPrivate: true, Dialer: recorder.dial})
	if _, err := Post(context.Background(), client, "http://siem.example.com/ingest", nil, nil); !errors.Is(err, ErrURLScheme) {
		t.Fatalf("http destination: %v, want ErrURLScheme", err)
	}
	if len(recorder.dialed) != 0 {
		t.Fatalf("dialed %v", recorder.dialed)
	}
}

func TestEgressFollowsNoRedirectAndNeverReachesThePrivateTarget(t *testing.T) {
	var privateHits atomic.Int32
	private := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		privateHits.Add(1)
		w.WriteHeader(200)
	}))
	t.Cleanup(private.Close)
	receiver, tlsConfig := tlsReceiver(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, private.URL+"/steal", http.StatusFound)
	}))
	resolver := &fakeResolver{answers: map[string][][]netip.Addr{"siem.example.com": {addrs("93.184.216.34")}}}
	recorder := &dialRecorder{target: strings.TrimPrefix(receiver.URL, "https://")}
	client := NewHTTPClient(Egress{Resolver: resolver, Dialer: recorder.dial, TLSConfig: tlsConfig})

	status, err := Post(context.Background(), client, "https://siem.example.com/ingest", nil, []byte("x"))
	if !errors.Is(err, ErrRedirect) || status != http.StatusFound {
		t.Fatalf("status=%d err=%v, want a refused 302", status, err)
	}
	if privateHits.Load() != 0 || len(recorder.dialed) != 1 {
		t.Fatalf("redirect was followed: private hits %d, dials %v", privateHits.Load(), recorder.dialed)
	}
}

func TestEgressAllowPrivateLiftsOnlyTheAddressCheck(t *testing.T) {
	var gotAuth, gotBody string
	receiver, tlsConfig := tlsReceiver(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		buf := make([]byte, 16)
		n, _ := r.Body.Read(buf)
		gotBody = string(buf[:n])
		w.WriteHeader(204)
	}))
	recorder := &dialRecorder{target: strings.TrimPrefix(receiver.URL, "https://")}
	client := NewHTTPClient(Egress{AllowPrivate: true, Dialer: recorder.dial, TLSConfig: tlsConfig})
	header := http.Header{}
	header.Set("Authorization", "Splunk t")
	status, err := Post(context.Background(), client, "https://127.0.0.1:9/ingest", header, []byte("hello"))
	if err != nil || status != 204 || gotAuth != "Splunk t" || gotBody != "hello" {
		t.Fatalf("status=%d err=%v auth=%q body=%q", status, err, gotAuth, gotBody)
	}
}

func TestPostRefusesABodyOverTheCapWithoutSending(t *testing.T) {
	recorder := &dialRecorder{target: "127.0.0.1:1"}
	client := NewHTTPClient(Egress{AllowPrivate: true, Dialer: recorder.dial})
	if _, err := Post(context.Background(), client, "https://siem.example.com/", nil, make([]byte, MaxBodyBytes+1)); !errors.Is(err, ErrBodyTooLarge) {
		t.Fatalf("oversize body: %v", err)
	}
	if len(recorder.dialed) != 0 {
		t.Fatalf("dialed %v", recorder.dialed)
	}
}

func TestFailureCodesNeverCarryTheURL(t *testing.T) {
	receiver, tlsConfig := tlsReceiver(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {}))
	// Nothing resolves: the transport error would normally embed the URL.
	client := NewHTTPClient(Egress{Resolver: &fakeResolver{}, TLSConfig: tlsConfig})
	_, err := Post(context.Background(), client, "https://secret-host.example.com/path?token=SECRETQUERY", nil, []byte("x"))
	if err == nil {
		t.Fatal("want a failure")
	}
	for _, text := range []string{err.Error(), FailureCode(err)} {
		for _, leak := range []string{"secret-host", "SECRETQUERY", "token=", receiver.URL} {
			if strings.Contains(text, leak) {
				t.Fatalf("failure %q leaks %q", text, leak)
			}
		}
	}
	if FailureCode(err) != "dns_failed" {
		t.Fatalf("code = %q", FailureCode(err))
	}
}
