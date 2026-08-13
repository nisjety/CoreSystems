// Package egress provides a general SSRF / DNS-rebinding guard for
// integration-corev2's outbound HTTP calls.
//
// integration-corev2 is the service CoreSystem's CLAUDE.md names as owner of
// provider-action outbound HTTP. Some destinations it dials are built from
// data the service does not fully control: a Shopify shop domain persisted
// in a connection's providerContext at connect time, or a pagination URL a
// provider's own API handed back in a JSON response (Meta's `paging.next`,
// Microsoft Graph's `@odata.nextLink`). Go's net/http resolves DNS itself
// with no pinning — validating a URL and then letting the transport
// re-resolve the same hostname independently, later, leaves a gap a DNS
// answer can change between the check and the connect ("DNS rebinding"),
// landing the connection on a private/loopback/metadata address the check
// never saw.
//
// This package closes that gap with a single-resolution design:
// ResolveAndVet resolves a host exactly once and vets every address it gets
// back; SafeClient's Transport.DialContext is the only place a connection is
// actually made, and it dials one of those already-vetted addresses
// directly rather than handing the hostname back to the transport for a
// second, unchecked lookup. Because resolution and dialing happen
// back-to-back inside one function call, there is no separate "pin now,
// look up later" registry to maintain the way Quarry-v2's Rust dns_guard.rs
// needs one (reqwest's custom-resolver hook fires independently and later,
// so it has to pre-register a pin for that future call to find). In Go, the
// address ResolveAndVet returns is the address net.Dialer is handed, so the
// checked address and the connected address are provably the same value.
package egress

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/netip"
	"os"
	"strings"
	"sync"
)

// ErrBlocked wraps every rejection ResolveAndVet produces, so callers (and
// tests) can distinguish "policy refused this destination" from an ordinary
// DNS or network failure via errors.Is(err, ErrBlocked).
var ErrBlocked = errors.New("egress: destination blocked by policy")

// blockedHostnames are refused before any DNS lookup runs, so a resolver
// that would otherwise answer for them — a locally configured stub, a
// hosts-file entry, a captive-portal DNS — never gets the chance.
// metadata.google.internal and metadata.goog are GCP's metadata-server
// names; 169.254.169.254 is the literal cloud-metadata address shared by
// AWS/GCP/Azure (also caught later by the link-local IP check below, but a
// name-level block is cheaper and fails before a lookup is even attempted).
//
// This check runs unconditionally, before the allowlist is consulted: a
// metadata hostname is never a legitimate destination, so nothing in
// WithAllowlist can carve out an exception for one.
var blockedHostnames = map[string]struct{}{
	"metadata.google.internal": {},
	"metadata.goog":            {},
	"169.254.169.254":          {},
}

// imdsV6 is AWS's IMDSv6 address. Unlike its v4 counterpart it does not sit
// inside a broader link-local range netip recognizes, so it needs an
// explicit literal check; RFC4193 ULA (fc00::/7, which fd00::/8 is part of)
// already blocks it via Guard.vet's IsPrivate case below, but checking it
// first gives a clearer rejection reason and does not depend on that
// broader rule staying in place.
var imdsV6 = netip.MustParseAddr("fd00:ec2::254")

// cgnatPrefix is the RFC 6598 carrier-grade NAT block. net/netip has no
// dedicated predicate for it (unlike RFC1918/RFC4193, which IsPrivate
// covers).
var cgnatPrefix = netip.MustParsePrefix("100.64.0.0/10")

// thisNetworkV4 is 0.0.0.0/8 ("this network"), broader than Addr.
// IsUnspecified (which matches only the single 0.0.0.0 address). No
// legitimate outbound target lives anywhere in the rest of the block.
var thisNetworkV4 = netip.MustParsePrefix("0.0.0.0/8")

// reservedV4 is 240.0.0.0/4, historically reserved "Class E" space. It also
// contains 255.255.255.255 (the limited broadcast address), which net/netip
// has no dedicated predicate for either.
var reservedV4 = netip.MustParsePrefix("240.0.0.0/4")

// Resolver is satisfied by *net.Resolver. Tests substitute a stub so
// DNS-rebinding-style scenarios are deterministic and need no real network
// or real DNS infrastructure.
type Resolver interface {
	LookupNetIP(ctx context.Context, network, host string) ([]netip.Addr, error)
}

// Guard vets destination hosts before ResolveAndVet or SafeClient will
// connect to them. The zero value is ready to use with default policy
// (net.DefaultResolver, every private/special-use range blocked, no
// allowlist) — construct with New to override the resolver or add an
// allowlist entry.
type Guard struct {
	resolver  Resolver
	allowHost map[string]struct{}
	allowNet  []netip.Prefix
}

// Option configures a Guard built via New.
type Option func(*Guard)

// WithResolver overrides the resolver. Production code has no reason to use
// this; it exists so tests can substitute a deterministic stub instead of
// real DNS.
func WithResolver(r Resolver) Option {
	return func(g *Guard) { g.resolver = r }
}

// WithAllowlist declares hostnames (exact match, case-insensitive, trailing
// dot ignored) and/or CIDRs that may resolve into otherwise-blocked private
// space without being rejected. Reach for this only when a survey of the
// service's outbound call sites shows a real, legitimate private
// destination — the package default is to block private ranges, and an
// allowlist entry should name the one destination that needed the
// exception, not relax the policy generally. As of this package's
// introduction, integration-corev2's call sites have no such destination:
// every class of caller-, org-config-, or provider-data-influenced host it
// dials (Shopify shop domains, Meta/Microsoft Graph pagination URLs) is
// expected to resolve to public internet space, so both lists are expected
// to stay empty.
func WithAllowlist(hosts []string, cidrs []string) Option {
	return func(g *Guard) {
		if g.allowHost == nil {
			g.allowHost = map[string]struct{}{}
		}
		for _, h := range hosts {
			if normalized := normalizeHost(h); normalized != "" {
				g.allowHost[normalized] = struct{}{}
			}
		}
		for _, c := range cidrs {
			if prefix, err := netip.ParsePrefix(strings.TrimSpace(c)); err == nil {
				g.allowNet = append(g.allowNet, prefix)
			}
		}
	}
}

// New builds a Guard using net.DefaultResolver unless overridden. Pass
// options to substitute a resolver (tests) or declare an allowlist.
func New(opts ...Option) *Guard {
	g := &Guard{resolver: net.DefaultResolver}
	for _, opt := range opts {
		opt(g)
	}
	if g.resolver == nil {
		g.resolver = net.DefaultResolver
	}
	return g
}

var (
	defaultGuardOnce sync.Once
	defaultGuard     *Guard
)

// Default returns the process-wide Guard SafeClient uses when its
// ClientConfig leaves Guard nil. It is built once, from
// EGRESS_ALLOWLIST_HOSTS (comma-separated hostnames) and
// EGRESS_ALLOWLIST_CIDRS (comma-separated CIDRs) — see WithAllowlist's doc
// comment for when to actually populate them.
func Default() *Guard {
	defaultGuardOnce.Do(func() {
		hosts := splitCSV(os.Getenv("EGRESS_ALLOWLIST_HOSTS"))
		cidrs := splitCSV(os.Getenv("EGRESS_ALLOWLIST_CIDRS"))
		defaultGuard = New(WithAllowlist(hosts, cidrs))
	})
	return defaultGuard
}

func splitCSV(raw string) []string {
	var out []string
	for _, part := range strings.Split(raw, ",") {
		if trimmed := strings.TrimSpace(part); trimmed != "" {
			out = append(out, trimmed)
		}
	}
	return out
}

// ResolveAndVet resolves host exactly once and returns every address DNS
// gave back, having confirmed none of them falls inside a blocked range.
// Resolution and vetting happen in the same call so nothing between "check"
// and "use" can substitute a different answer (the DNS-rebinding TOCTOU).
//
// A partially-bad answer fails the whole lookup rather than silently
// filtering down to the addresses that passed: a resolver that returns one
// public and one private address for the same name is exhibiting exactly
// the rebinding behavior this guard exists to catch, and quietly dialing
// the public one would still normalize "sometimes answers with a private
// address" as tolerable for that name.
func (g *Guard) ResolveAndVet(ctx context.Context, host string) ([]netip.Addr, error) {
	normalized := normalizeHost(host)
	if normalized == "" {
		return nil, fmt.Errorf("%w: empty host", ErrBlocked)
	}
	if _, blocked := blockedHostnames[normalized]; blocked {
		return nil, fmt.Errorf("%w: %s is a cloud metadata hostname", ErrBlocked, host)
	}
	resolver := g.resolver
	if resolver == nil {
		resolver = net.DefaultResolver
	}
	addrs, err := resolver.LookupNetIP(ctx, "ip", normalized)
	if err != nil {
		return nil, fmt.Errorf("egress: resolve %s: %w", host, err)
	}
	if len(addrs) == 0 {
		return nil, fmt.Errorf("%w: %s resolved to no addresses", ErrBlocked, host)
	}
	if g.hostAllowed(normalized) {
		return addrs, nil
	}
	for _, addr := range addrs {
		if reason := g.vet(addr); reason != "" {
			return nil, fmt.Errorf("%w: %s resolved to %s (%s)", ErrBlocked, host, addr, reason)
		}
	}
	return addrs, nil
}

func (g *Guard) hostAllowed(normalizedHost string) bool {
	if g == nil || len(g.allowHost) == 0 {
		return false
	}
	_, ok := g.allowHost[normalizedHost]
	return ok
}

func (g *Guard) netAllowed(addr netip.Addr) bool {
	if g == nil {
		return false
	}
	for _, prefix := range g.allowNet {
		if prefix.Contains(addr) {
			return true
		}
	}
	return false
}

// vet returns a non-empty rejection reason for any address a general
// outbound HTTP client must not connect to, or "" if addr is fine.
//
// It normalizes IPv4-mapped IPv6 (::ffff:a.b.c.d) to plain IPv4 first:
// judged as IPv6, a mapped address matches none of netip's IPv4 predicates
// (IsPrivate, IsLinkLocalUnicast, ...), so skipping the unmap would let an
// attacker wrap a blocked v4 address in its v6-mapped form and sail
// through.
func (g *Guard) vet(addr netip.Addr) string {
	addr = addr.Unmap()
	if g.netAllowed(addr) {
		return ""
	}
	switch {
	case addr.IsUnspecified():
		return "unspecified"
	case addr.IsLoopback():
		return "loopback"
	case thisNetworkV4.Contains(addr):
		return `"this network" (0.0.0.0/8)`
	case addr.IsLinkLocalUnicast():
		return "link-local (includes the 169.254.169.254 cloud metadata address)"
	case addr.IsMulticast():
		return "multicast"
	case reservedV4.Contains(addr):
		return "reserved/broadcast (240.0.0.0/4)"
	case addr == imdsV6:
		return "AWS IMDSv6 (fd00:ec2::254)"
	case addr.IsPrivate():
		return "private (RFC1918/RFC4193)"
	case cgnatPrefix.Contains(addr):
		return "carrier-grade NAT (100.64.0.0/10)"
	default:
		return ""
	}
}

func normalizeHost(host string) string {
	host = strings.TrimSpace(host)
	host = strings.TrimSuffix(host, ".")
	host = strings.Trim(host, "[]")
	return strings.ToLower(host)
}
