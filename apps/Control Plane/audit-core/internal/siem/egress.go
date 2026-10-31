// Package siem delivers an org's audit chain to the destinations that org
// configured in org-core (build plan WS06-S8). The exporter is a separate
// process from the audit-core server so that the one component that dials
// customer-chosen addresses holds no ingest credential and no signing key.
//
// This file is the egress guard. A destination URL is customer input, so every
// connection the exporter makes goes through it: https only, no userinfo, the
// resolved addresses checked at dial time (a DNS answer can change between a
// check and a connect), no redirects, ten seconds, and a bounded body.
package siem

import (
	"bytes"
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"time"
)

const (
	// RequestTimeout bounds one delivery: dial, TLS, request and response.
	RequestTimeout = 10 * time.Second
	// MaxBodyBytes is the largest request body the exporter sends. A batch of
	// 500 rows is about 900 KiB; the packer in the runner stops earlier when
	// the rows are larger than that.
	MaxBodyBytes = 1 << 20
	// maxResponseBytes is how much of a receiver's answer is read before the
	// connection is dropped. The body is never logged or stored.
	maxResponseBytes = 64 << 10
)

// Egress errors are a closed vocabulary of failure causes. They carry no URL,
// address or response text, because net/http errors embed the URL and the
// exporter's log lines must not.
var (
	ErrURLScheme      = errors.New("destination url must use https")
	ErrURLUserinfo    = errors.New("destination url must not carry credentials")
	ErrURLHost        = errors.New("destination url has no usable host")
	ErrBlockedAddress = errors.New("destination resolves to a blocked address")
	ErrResolve        = errors.New("destination host did not resolve")
	ErrRedirect       = errors.New("destination answered with a redirect")
	ErrBodyTooLarge   = errors.New("request body exceeds the egress cap")
)

// blockedPrefixes are the ranges a customer destination may never reach: the
// host's own loopback, private and link-local space (the cloud metadata address
// 169.254.169.254 is inside 169.254.0.0/16), carrier-grade NAT, the IPv6 unique
// local range, and the other non-routable blocks. Addresses are unmapped and
// tunnelling prefixes are checked through their embedded IPv4 address first,
// so ::ffff:127.0.0.1 and 64:ff9b::a00:1 cannot smuggle a private target past
// the table.
var blockedPrefixes = mustPrefixes(
	"0.0.0.0/8",      // "this network"
	"10.0.0.0/8",     // RFC 1918
	"100.64.0.0/10",  // CGNAT
	"127.0.0.0/8",    // loopback
	"169.254.0.0/16", // link-local, includes the metadata address
	"172.16.0.0/12",  // RFC 1918
	"192.0.0.0/24",   // IETF protocol assignments
	"192.168.0.0/16", // RFC 1918
	"198.18.0.0/15",  // benchmarking
	"224.0.0.0/4",    // multicast
	"240.0.0.0/4",    // reserved, includes the broadcast address
	"::/128",         // unspecified
	"::1/128",        // loopback
	"100::/64",       // discard-only
	"fc00::/7",       // unique local
	"fe80::/10",      // link-local
	"fec0::/10",      // deprecated site-local
	"ff00::/8",       // multicast
)

func mustPrefixes(values ...string) []netip.Prefix {
	prefixes := make([]netip.Prefix, len(values))
	for i, value := range values {
		prefixes[i] = netip.MustParsePrefix(value)
	}
	return prefixes
}

var (
	nat64Prefix  = netip.MustParsePrefix("64:ff9b::/96")
	sixToFour    = netip.MustParsePrefix("2002::/16")
	teredoPrefix = netip.MustParsePrefix("2001::/32")
)

// IsBlockedAddress reports whether the guard refuses to connect to addr.
func IsBlockedAddress(addr netip.Addr) bool {
	addr = addr.Unmap().WithZone("")
	if addr.Is6() {
		// An IPv6 form that carries an IPv4 address is judged by that address.
		raw := addr.As16()
		switch {
		case nat64Prefix.Contains(addr):
			return IsBlockedAddress(netip.AddrFrom4([4]byte(raw[12:16])))
		case sixToFour.Contains(addr):
			return IsBlockedAddress(netip.AddrFrom4([4]byte(raw[2:6])))
		case teredoPrefix.Contains(addr):
			// Teredo is a tunnel to an arbitrary relay, never a customer SIEM.
			return true
		}
	}
	for _, prefix := range blockedPrefixes {
		if prefix.Contains(addr) {
			return true
		}
	}
	return false
}

// ValidateURL checks the parts of a destination URL that do not need the
// network. The dial-time check in NewHTTPClient is the real SSRF defence: this
// only refuses a destination early with a precise cause.
func ValidateURL(raw string) (*url.URL, error) {
	parsed, err := url.Parse(raw)
	if err != nil {
		return nil, ErrURLHost
	}
	switch {
	case !strings.EqualFold(parsed.Scheme, "https"):
		return nil, ErrURLScheme
	case parsed.User != nil:
		return nil, ErrURLUserinfo
	case parsed.Hostname() == "":
		return nil, ErrURLHost
	}
	return parsed, nil
}

// Resolver is the slice of net.Resolver the guard needs; tests replace it.
type Resolver interface {
	LookupNetIP(ctx context.Context, network, host string) ([]netip.Addr, error)
}

// Egress configures the guarded HTTP client.
type Egress struct {
	// AllowPrivate lifts the address check (AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS,
	// refused in production by the binary). The https-only and no-redirect rules
	// stay on.
	AllowPrivate bool
	// Resolver defaults to net.DefaultResolver.
	Resolver Resolver
	// Dialer defaults to a plain net.Dialer. Tests substitute it to reach a
	// local receiver without opening the guard.
	Dialer func(ctx context.Context, network, address string) (net.Conn, error)
	// TLSConfig is for tests (a private root); production uses the system roots.
	TLSConfig *tls.Config
}

// NewHTTPClient returns a client that can only deliver to addresses the guard
// accepts, never follows a redirect, and gives up after RequestTimeout.
func NewHTTPClient(e Egress) *http.Client {
	resolver := e.Resolver
	if resolver == nil {
		resolver = net.DefaultResolver
	}
	dial := e.Dialer
	if dial == nil {
		dial = (&net.Dialer{Timeout: RequestTimeout}).DialContext
	}
	transport := &http.Transport{
		// The proxy variables would send the request to a host the guard never
		// resolved, so the guard would be checking the wrong address.
		Proxy:                 nil,
		TLSClientConfig:       e.TLSConfig,
		TLSHandshakeTimeout:   RequestTimeout,
		ResponseHeaderTimeout: RequestTimeout,
		DisableKeepAlives:     true,
		DialContext: func(ctx context.Context, network, address string) (net.Conn, error) {
			host, port, err := net.SplitHostPort(address)
			if err != nil {
				return nil, ErrURLHost
			}
			addrs, err := resolveAll(ctx, resolver, host)
			if err != nil {
				return nil, err
			}
			if !e.AllowPrivate {
				for _, addr := range addrs {
					if IsBlockedAddress(addr) {
						return nil, ErrBlockedAddress
					}
				}
			}
			// Connect to the address that was checked, not to the name: a second
			// lookup could answer differently.
			var lastErr error
			for _, addr := range addrs {
				conn, err := dial(ctx, network, net.JoinHostPort(addr.String(), port))
				if err == nil {
					return conn, nil
				}
				lastErr = err
			}
			return nil, lastErr
		},
	}
	return &http.Client{
		Transport: transport,
		Timeout:   RequestTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
}

// resolveAll returns every address of host. Every one is checked, not just the
// first: a name that answers with one public and one private address would
// otherwise reach the private one on a connect retry.
func resolveAll(ctx context.Context, resolver Resolver, host string) ([]netip.Addr, error) {
	if addr, err := netip.ParseAddr(host); err == nil {
		return []netip.Addr{addr}, nil
	}
	addrs, err := resolver.LookupNetIP(ctx, "ip", host)
	if err != nil || len(addrs) == 0 {
		return nil, ErrResolve
	}
	return addrs, nil
}

// Post sends body to destination with the given headers and reports only the
// HTTP status. Any 3xx is returned as ErrRedirect (the client does not follow
// it) so a redirect to a private address is never even attempted. The response
// body is read up to a cap and discarded.
func Post(ctx context.Context, client *http.Client, destination string, header http.Header, body []byte) (int, error) {
	if len(body) > MaxBodyBytes {
		return 0, ErrBodyTooLarge
	}
	if _, err := ValidateURL(destination); err != nil {
		return 0, err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, destination, bytes.NewReader(body))
	if err != nil {
		return 0, ErrURLHost
	}
	for name, values := range header {
		for _, value := range values {
			request.Header.Add(name, value)
		}
	}
	response, err := client.Do(request)
	if err != nil {
		return 0, classifyTransportError(err)
	}
	defer response.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxResponseBytes))
	if response.StatusCode >= 300 && response.StatusCode < 400 {
		return response.StatusCode, ErrRedirect
	}
	return response.StatusCode, nil
}

// classifyTransportError strips a transport error down to its cause. The
// original error embeds the request URL, which must not reach a log.
func classifyTransportError(err error) error {
	for _, known := range []error{ErrBlockedAddress, ErrResolve, ErrURLHost} {
		if errors.Is(err, known) {
			return known
		}
	}
	var timeout interface{ Timeout() bool }
	if errors.As(err, &timeout) && timeout.Timeout() || errors.Is(err, context.DeadlineExceeded) {
		return errTimeout
	}
	if errors.Is(err, context.Canceled) {
		return context.Canceled
	}
	return errTransport
}

var (
	errTimeout   = errors.New("destination did not answer in time")
	errTransport = errors.New("destination connection failed")
)

// FailureCode maps a delivery error to the token stored as last_error and used
// as a metric and log value: a fixed vocabulary, so no URL, address or response
// text can ride along.
func FailureCode(err error) string {
	switch {
	case err == nil:
		return ""
	case errors.Is(err, ErrBlockedAddress):
		return "egress_blocked"
	case errors.Is(err, ErrURLScheme), errors.Is(err, ErrURLUserinfo), errors.Is(err, ErrURLHost):
		return "url_invalid"
	case errors.Is(err, ErrResolve):
		return "dns_failed"
	case errors.Is(err, ErrRedirect):
		return "redirect_refused"
	case errors.Is(err, ErrBodyTooLarge):
		return "body_too_large"
	case errors.Is(err, errTimeout):
		return "timeout"
	case errors.Is(err, context.Canceled):
		return "canceled"
	case errors.Is(err, errTransport):
		return "transport"
	default:
		return "error"
	}
}

// statusFailure is the failure token for a non-2xx answer.
func statusFailure(status int) string {
	return fmt.Sprintf("http_%d", status)
}
