package egress

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"testing"
)

// stubResolver lets tests control DNS answers precisely instead of hitting a
// real resolver, which is what makes rebinding-style scenarios reproducible:
// a hostname can be made to answer with any address, including one no real
// authoritative server for that name would ever legitimately return.
//
// Hosts not present in answers fall back to parsing the host as an IP
// literal (mirroring what a real resolver does for literals with no lookup
// at all) so tests can dial real local listeners (which httptest and
// net.Listen always address by literal IP) without needing an entry for
// every one of them.
type stubResolver struct {
	answers map[string][]netip.Addr
	calls   int
}

func (s *stubResolver) LookupNetIP(_ context.Context, _, host string) ([]netip.Addr, error) {
	s.calls++
	normalized := normalizeHost(host)
	if addrs, ok := s.answers[normalized]; ok {
		return addrs, nil
	}
	if addr, err := netip.ParseAddr(normalized); err == nil {
		return []netip.Addr{addr}, nil
	}
	return nil, &net.DNSError{Err: "no such host", Name: host, IsNotFound: true}
}

func TestGuardVetBlocksEveryReservedRange(t *testing.T) {
	g := New()
	tests := []struct {
		name    string
		addr    string
		blocked bool
	}{
		{"loopback v4", "127.0.0.1", true},
		{"loopback v6", "::1", true},
		{"unspecified v4", "0.0.0.0", true},
		{"unspecified v6", "::", true},
		{"this-network v4 (rest of 0.0.0.0/8)", "0.8.8.8", true},
		{"link-local v4", "169.254.1.1", true},
		{"aws imdsv4 metadata address", "169.254.169.254", true},
		{"link-local v6", "fe80::1", true},
		{"multicast v4", "224.0.0.1", true},
		{"multicast v6", "ff02::1", true},
		{"reserved class-e v4", "240.0.0.1", true},
		{"limited broadcast", "255.255.255.255", true},
		{"aws imdsv6 metadata address", "fd00:ec2::254", true},
		{"rfc1918 10/8", "10.1.2.3", true},
		{"rfc1918 172.16/12", "172.16.5.5", true},
		{"rfc1918 192.168/16", "192.168.1.1", true},
		{"rfc4193 ula", "fc00::1", true},
		{"cgnat 100.64/10", "100.64.0.1", true},
		{"cgnat upper edge", "100.127.255.255", true},
		{"public v4 (test-net-3, not in this blocklist)", "203.0.113.10", false},
		{"public v6", "2606:4700:4700::1111", false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			addr := netip.MustParseAddr(tt.addr)
			reason := g.vet(addr)
			if tt.blocked && reason == "" {
				t.Fatalf("expected %s to be blocked, but vet() allowed it", tt.addr)
			}
			if !tt.blocked && reason != "" {
				t.Fatalf("expected %s to be allowed, but vet() blocked it: %s", tt.addr, reason)
			}
		})
	}
}

func TestGuardVetNormalizesIPv4MappedIPv6(t *testing.T) {
	g := New()
	// ::ffff:10.0.0.1 must be judged by the IPv4 rules for 10.0.0.1
	// (private), or wrapping a blocked v4 address in its v6-mapped form
	// would bypass every IPv4-specific predicate (IsPrivate,
	// IsLinkLocalUnicast, ...) since none of them match on the v6 form.
	mappedPrivate := netip.MustParseAddr("::ffff:10.0.0.1")
	if reason := g.vet(mappedPrivate); reason == "" {
		t.Fatal("expected ::ffff:10.0.0.1 (mapped RFC1918 address) to be blocked")
	}
	mappedMetadata := netip.MustParseAddr("::ffff:169.254.169.254")
	if reason := g.vet(mappedMetadata); reason == "" {
		t.Fatal("expected ::ffff:169.254.169.254 (mapped metadata address) to be blocked")
	}
	mappedPublic := netip.MustParseAddr("::ffff:203.0.113.5")
	if reason := g.vet(mappedPublic); reason != "" {
		t.Fatalf("expected ::ffff:203.0.113.5 (mapped public address) to be allowed, got: %s", reason)
	}
}

func TestResolveAndVetBlocksMetadataHostnamesPreDNS(t *testing.T) {
	// The stub would happily answer with a public address for these names;
	// proving the block still fires (and that the stub is never even
	// consulted) demonstrates the check runs before DNS, not because DNS
	// happened to fail.
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"metadata.google.internal": {netip.MustParseAddr("8.8.8.8")},
		"metadata.goog":            {netip.MustParseAddr("8.8.8.8")},
		"169.254.169.254":          {netip.MustParseAddr("169.254.169.254")},
	}}
	g := New(WithResolver(stub))
	for _, host := range []string{"metadata.google.internal", "metadata.goog", "169.254.169.254", "METADATA.GOOGLE.INTERNAL.", "metadata.goog."} {
		if _, err := g.ResolveAndVet(context.Background(), host); !errors.Is(err, ErrBlocked) {
			t.Fatalf("expected %s to be blocked pre-DNS, got %v", host, err)
		}
	}
	if stub.calls != 0 {
		t.Fatalf("expected the resolver never to be consulted for a pre-DNS-blocked hostname, got %d calls", stub.calls)
	}
}

// TestResolveAndVetCatchesRebinding is the DNS-rebinding proof: the hostname
// looks like any other external name, but the (stubbed) resolver answers
// with a loopback address, exactly what an attacker controlling DNS for
// that name can do. ResolveAndVet must refuse it based on the resolved
// address, not the appearance of the name.
func TestResolveAndVetCatchesRebinding(t *testing.T) {
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"rebind.example.test": {netip.MustParseAddr("127.0.0.1")},
	}}
	g := New(WithResolver(stub))
	if _, err := g.ResolveAndVet(context.Background(), "rebind.example.test"); !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected rebinding to loopback to be blocked, got %v", err)
	}
}

func TestResolveAndVetFailsClosedOnPartiallyBadAnswer(t *testing.T) {
	// One legitimate-looking address plus one private address in the same
	// answer is exactly the rebinding pattern this guard defends against; a
	// resolver willing to answer that way for one lookup cannot be trusted
	// to keep answering consistently for the next one, so the whole lookup
	// fails rather than silently using only the address that passed.
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"mixed.example.test": {netip.MustParseAddr("203.0.113.1"), netip.MustParseAddr("10.0.0.1")},
	}}
	g := New(WithResolver(stub))
	if _, err := g.ResolveAndVet(context.Background(), "mixed.example.test"); !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected a mixed public/private answer to be blocked entirely, got %v", err)
	}
}

func TestResolveAndVetAllowlistedHostSkipsIPVetting(t *testing.T) {
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"internal.example.test": {netip.MustParseAddr("10.1.2.3")},
	}}
	g := New(WithResolver(stub), WithAllowlist([]string{"Internal.Example.Test."}, nil))
	addrs, err := g.ResolveAndVet(context.Background(), "internal.example.test")
	if err != nil {
		t.Fatalf("expected the allowlisted host to resolve, got %v", err)
	}
	if len(addrs) != 1 || addrs[0].String() != "10.1.2.3" {
		t.Fatalf("expected the allowlisted private address to be returned, got %v", addrs)
	}
}

func TestResolveAndVetAllowlistedCIDR(t *testing.T) {
	stub := &stubResolver{answers: map[string][]netip.Addr{
		"cidr.example.test":  {netip.MustParseAddr("10.9.9.9")},
		"other.example.test": {netip.MustParseAddr("10.1.1.1")},
	}}
	g := New(WithResolver(stub), WithAllowlist(nil, []string{"10.9.0.0/16"}))
	if _, err := g.ResolveAndVet(context.Background(), "cidr.example.test"); err != nil {
		t.Fatalf("expected the CIDR-allowlisted address to resolve, got %v", err)
	}
	// A different private address NOT covered by the allowlisted CIDR must
	// still be blocked — the allowlist is scoped to the declared range, not
	// private space in general.
	if _, err := g.ResolveAndVet(context.Background(), "other.example.test"); !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected an address outside the allowlisted CIDR to still be blocked, got %v", err)
	}
}

func TestResolveAndVetAllowlistNeverCoversMetadataHostnames(t *testing.T) {
	// Allowlisting a metadata hostname by name should not be possible: the
	// pre-DNS hostname block runs unconditionally, ahead of the allowlist
	// check.
	g := New(WithAllowlist([]string{"metadata.google.internal"}, nil))
	if _, err := g.ResolveAndVet(context.Background(), "metadata.google.internal"); !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected metadata.google.internal to stay blocked even when allowlisted by name, got %v", err)
	}
}

func TestResolveAndVetRejectsEmptyHost(t *testing.T) {
	g := New()
	if _, err := g.ResolveAndVet(context.Background(), "   "); !errors.Is(err, ErrBlocked) {
		t.Fatalf("expected an empty host to be blocked, got %v", err)
	}
}

func TestResolveAndVetPropagatesResolverFailure(t *testing.T) {
	g := New(WithResolver(&stubResolver{answers: map[string][]netip.Addr{}}))
	_, err := g.ResolveAndVet(context.Background(), "unresolvable.example.test")
	if err == nil {
		t.Fatal("expected a resolver failure to surface as an error")
	}
	if errors.Is(err, ErrBlocked) {
		t.Fatal("a DNS failure is not a policy rejection and must not be reported as one")
	}
}
