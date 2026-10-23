package egress

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"time"
)

const (
	defaultConnectTimeout      = 5 * time.Second
	defaultTLSHandshakeTimeout = 5 * time.Second
	defaultRequestTimeout      = 20 * time.Second
	defaultMaxRedirects        = 5
)

// ClientConfig controls the http.Client SafeClient builds. The zero value is
// a safe, sensible default: the process-wide Default() Guard, a 5s connect
// timeout, a 5s TLS handshake timeout, a 20s overall request timeout, and
// redirects disabled outright.
type ClientConfig struct {
	// Guard is the policy every dial is checked against. Nil uses Default().
	Guard *Guard
	// ConnectTimeout bounds the TCP handshake. Defaults to 5s. Every
	// SafeClient always has one: this package exists partly because a
	// client without a connect timeout hung a whole test suite this week
	// (an unbounded connect to a half-open proxy that accepted the TCP
	// handshake but never completed anything above it).
	ConnectTimeout time.Duration
	// TLSHandshakeTimeout bounds the TLS handshake. Defaults to 5s.
	TLSHandshakeTimeout time.Duration
	// RequestTimeout is http.Client.Timeout, bounding the entire
	// request-to-response-body-read cycle. Defaults to 20s.
	RequestTimeout time.Duration
	// AllowRedirects opts into following redirects, re-vetting the target
	// host of every hop before it is followed. Defaults to false: the
	// call-site survey behind this package found no provider call that
	// needs a redirect followed, and refusing them outright is simpler to
	// reason about than "vetted, but only sometimes."
	AllowRedirects bool
	// MaxRedirects caps hops when AllowRedirects is true. Defaults to 5.
	MaxRedirects int
}

// SafeClient builds an *http.Client whose transport dials only addresses
// ResolveAndVet already approved for the request's hostname — the checked
// address and the connected address are the same value, because
// DialContext resolves, vets, and dials in a single call with no gap in
// between.
//
// TLS ServerName/SNI still comes from the original hostname even though the
// socket connects to a raw IP: DialContext returns a plain TCP connection,
// and http.Transport (not this package) performs the TLS handshake on top
// of it using the hostname it was asked to dial, which Transport derives
// from its own "addr" argument — the request's original host:port, never
// the IP this function substitutes internally. Using DialContext rather
// than DialTLSContext is what keeps the handshake (and its ServerName) the
// transport's job instead of this package's.
func SafeClient(cfg ClientConfig) *http.Client {
	guard := cfg.Guard
	if guard == nil {
		guard = Default()
	}
	connectTimeout := cfg.ConnectTimeout
	if connectTimeout <= 0 {
		connectTimeout = defaultConnectTimeout
	}
	tlsTimeout := cfg.TLSHandshakeTimeout
	if tlsTimeout <= 0 {
		tlsTimeout = defaultTLSHandshakeTimeout
	}
	requestTimeout := cfg.RequestTimeout
	if requestTimeout <= 0 {
		requestTimeout = defaultRequestTimeout
	}

	dialer := &net.Dialer{Timeout: connectTimeout}
	transport := &http.Transport{
		DialContext:           pinnedDialContext(guard, dialer),
		TLSHandshakeTimeout:   tlsTimeout,
		ForceAttemptHTTP2:     true,
		MaxIdleConns:          100,
		MaxIdleConnsPerHost:   10,
		IdleConnTimeout:       90 * time.Second,
		ExpectContinueTimeout: 1 * time.Second,
	}
	client := &http.Client{Transport: transport, Timeout: requestTimeout}

	if cfg.AllowRedirects {
		maxRedirects := cfg.MaxRedirects
		if maxRedirects <= 0 {
			maxRedirects = defaultMaxRedirects
		}
		client.CheckRedirect = revettingCheckRedirect(guard, maxRedirects)
	} else {
		client.CheckRedirect = func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		}
	}
	return client
}

// pinnedDialContext resolves and vets the host being dialed, then connects
// directly to one of the vetted addresses, trying each in turn the way an
// ordinary dial fails over between A/AAAA records. It never hands the
// hostname back to the standard resolver, so nothing downstream of this
// function performs a second, unchecked lookup — the address ResolveAndVet
// approved is the exact address net.Dialer connects to.
func pinnedDialContext(guard *Guard, dialer *net.Dialer) func(context.Context, string, string) (net.Conn, error) {
	return func(ctx context.Context, network, addr string) (net.Conn, error) {
		host, port, err := net.SplitHostPort(addr)
		if err != nil {
			return nil, fmt.Errorf("egress: split host:port %q: %w", addr, err)
		}
		addrs, err := guard.ResolveAndVet(ctx, host)
		if err != nil {
			return nil, err
		}
		var lastErr error
		for _, vetted := range addrs {
			conn, dialErr := dialer.DialContext(ctx, network, net.JoinHostPort(vetted.String(), port))
			if dialErr == nil {
				return conn, nil
			}
			lastErr = dialErr
		}
		return nil, fmt.Errorf("egress: dial vetted addresses for %s: %w", host, lastErr)
	}
}

// revettingCheckRedirect re-runs ResolveAndVet against a redirect target's
// host before http.Client is allowed to follow it. DialContext would catch
// an unvetted host regardless (a redirect to a different host always dials
// fresh, since the connection pool is keyed by host:port), but this makes
// "every hop is re-vetted" an explicit, independently testable property
// instead of an emergent side effect of Go's connection-pooling behavior.
func revettingCheckRedirect(guard *Guard, maxRedirects int) func(*http.Request, []*http.Request) error {
	return func(req *http.Request, via []*http.Request) error {
		if len(via) >= maxRedirects {
			return fmt.Errorf("egress: stopped after %d redirects", maxRedirects)
		}
		if _, err := guard.ResolveAndVet(req.Context(), req.URL.Hostname()); err != nil {
			return err
		}
		return nil
	}
}
