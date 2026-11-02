package org

import (
	"context"
	"errors"
	"strings"
	"testing"
)

// WS05-S6: domain normalisation and the switch-off behaviour of the service. Pure logic, no database.

func TestNormalizeIdentityDomain(t *testing.T) {
	label63 := strings.Repeat("a", 63)
	cases := []struct {
		name   string
		in     string
		want   string // normalized form when valid
		reason string // closed reason when refused
	}{
		// valid forms
		{name: "plain", in: "example.com", want: "example.com"},
		{name: "uppercase", in: "ExAmPle.COM", want: "example.com"},
		{name: "trailing dot", in: "example.com.", want: "example.com"},
		{name: "surrounding space", in: "  example.com\t", want: "example.com"},
		{name: "subdomain", in: "mail.sub.example.co.uk", want: "mail.sub.example.co.uk"},
		{name: "digits and hyphens", in: "a-1.b2-c.example.org", want: "a-1.b2-c.example.org"},
		{name: "idn to punycode", in: "bücher.example", want: "xn--bcher-kva.example"},
		{name: "punycode in", in: "xn--bcher-kva.example", want: "xn--bcher-kva.example"},
		{name: "idn uppercase", in: "BÜCHER.Example", want: "xn--bcher-kva.example"},
		{name: "253 bytes exactly", in: strings.Join([]string{label63, label63, label63, strings.Repeat("a", 61)}, "."), want: strings.Join([]string{label63, label63, label63, strings.Repeat("a", 61)}, ".")},

		// empty
		{name: "empty", in: "", reason: "empty"},
		{name: "blank", in: "   ", reason: "empty"},
		{name: "lone dot", in: ".", reason: "empty"},

		// IP literals
		{name: "ipv4", in: "192.168.0.1", reason: "ip_literal"},
		{name: "ipv4 public", in: "8.8.8.8", reason: "ip_literal"},
		{name: "ipv6", in: "2001:db8::1", reason: "ip_literal"},
		{name: "ipv6 loopback", in: "::1", reason: "ip_literal"},
		{name: "ipv6 bracketed", in: "[::1]", reason: "ip_literal"},
		{name: "numeric last label", in: "example.123", reason: "ip_literal"},

		// shape
		{name: "single label", in: "localhost", reason: "malformed"},
		{name: "bare tld", in: "com", reason: "malformed"},
		{name: "scheme", in: "https://example.com", reason: "malformed"},
		{name: "path", in: "example.com/login", reason: "malformed"},
		{name: "email", in: "user@example.com", reason: "malformed"},
		{name: "port", in: "example.com:443", reason: "malformed"},
		{name: "wildcard", in: "*.example.com", reason: "malformed"},
		{name: "underscore", in: "foo_bar.example.com", reason: "malformed"},
		{name: "inner space", in: "foo bar.example.com", reason: "malformed"},
		{name: "empty label", in: "a..example.com", reason: "malformed"},
		{name: "leading dot", in: ".example.com", reason: "malformed"},
		{name: "leading hyphen", in: "-a.example.com", reason: "malformed"},
		{name: "trailing hyphen", in: "a-.example.com", reason: "malformed"},
		{name: "label over 63", in: strings.Repeat("a", 64) + ".example.com", reason: "malformed"},
		{name: "control char", in: "exa\nmple.com", reason: "malformed"},

		// length
		{name: "over 253", in: strings.Join([]string{label63, label63, label63, label63}, "."), reason: "too_long"},
		{name: "absurdly long", in: strings.Repeat("a", 2000) + ".com", reason: "too_long"},

		// suffixes and free mail
		{name: "public suffix", in: "co.uk", reason: "public_suffix"},
		{name: "private public suffix", in: "github.io", reason: "public_suffix"},
		{name: "free mail", in: "gmail.com", reason: "free_mail"},
		{name: "free mail uppercase", in: "GMAIL.COM", reason: "free_mail"},
		{name: "apple relay", in: "privaterelay.appleid.com", reason: "free_mail"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got, err := NormalizeIdentityDomain(tc.in)
			if tc.reason == "" {
				if err != nil {
					t.Fatalf("NormalizeIdentityDomain(%q) refused: %v", tc.in, err)
				}
				if got != tc.want {
					t.Fatalf("NormalizeIdentityDomain(%q) = %q, want %q", tc.in, got, tc.want)
				}
				return
			}
			var invalid *DomainValidationError
			if !errors.As(err, &invalid) {
				t.Fatalf("NormalizeIdentityDomain(%q) = %q, %v; want a DomainValidationError", tc.in, got, err)
			}
			if invalid.Reason != tc.reason {
				t.Fatalf("NormalizeIdentityDomain(%q) reason = %q, want %q", tc.in, invalid.Reason, tc.reason)
			}
			if got != "" {
				t.Fatalf("a refused domain must return no value, got %q", got)
			}
		})
	}
}

func TestNormalizeIdentityDomainIsIdempotent(t *testing.T) {
	for _, in := range []string{"Example.COM.", "bücher.example", "sub.Example.org"} {
		once, err := NormalizeIdentityDomain(in)
		if err != nil {
			t.Fatal(err)
		}
		twice, err := NormalizeIdentityDomain(once)
		if err != nil || twice != once {
			t.Fatalf("normalising %q again: %q, %v", once, twice, err)
		}
	}
}

func TestDomainValidationErrorCarriesTheReason(t *testing.T) {
	err := &DomainValidationError{Reason: "free_mail"}
	if !strings.Contains(err.Error(), "free_mail") {
		t.Fatalf("error text %q must carry the reason", err.Error())
	}
}

func TestDomainChallengeShape(t *testing.T) {
	if got := DomainChallengeName("example.com"); got != "_verevon-challenge.example.com" {
		t.Fatalf("challenge name = %q", got)
	}
	if got := DomainChallengeValue("tok"); got != "verevon-site-verification=tok" {
		t.Fatalf("challenge value = %q", got)
	}
	if domainChallenge("example.com", "", nil) != nil {
		t.Fatal("no token and no expiry means no challenge")
	}
}

func TestNewDomainTokenIsUnpredictable(t *testing.T) {
	seen := map[string]bool{}
	for i := 0; i < 50; i++ {
		token, err := newDomainToken()
		if err != nil {
			t.Fatal(err)
		}
		if len(token) < 32 {
			t.Fatalf("token %q is too short to be a 192-bit secret", token)
		}
		if seen[token] {
			t.Fatalf("token repeated: %q", token)
		}
		seen[token] = true
	}
}

// A service that never had EnableDomainVerification called refuses every
// domain method before it touches the repository (nil here), so a deployment
// with the flag off behaves exactly as before.
func TestDomainMethodsRefuseUntilVerificationIsEnabled(t *testing.T) {
	service := &Service{}
	ctx := context.Background()
	if service.DomainVerificationEnabled() {
		t.Fatal("verification must default to off")
	}
	checks := map[string]error{}
	_, checks["list"] = service.ListIdentityDomains(ctx, "org-1")
	_, checks["add"] = service.AddIdentityDomain(ctx, "org-1", "u", "admin", "example.com")
	_, checks["verify"] = service.VerifyIdentityDomain(ctx, "org-1", "u", "admin", "example.com")
	checks["revoke"] = service.RevokeIdentityDomain(ctx, "org-1", "u", "admin", "example.com")
	_, checks["lookup"] = service.LookupVerifiedDomainOwner(ctx, "example.com")
	for name, err := range checks {
		if !errors.Is(err, ErrDomainVerificationUnavailable) {
			t.Fatalf("%s: err = %v, want ErrDomainVerificationUnavailable", name, err)
		}
	}

	service.EnableDomainVerification(&fakeTXTResolver{})
	if !service.DomainVerificationEnabled() {
		t.Fatal("EnableDomainVerification must switch the feature on")
	}
}

// Once enabled, a bad domain is refused as a validation error before the repository is reached.
func TestEnabledDomainMethodsValidateBeforeTheRepository(t *testing.T) {
	service := &Service{}
	service.EnableDomainVerification(&fakeTXTResolver{})
	ctx := context.Background()
	var invalid *DomainValidationError
	if _, err := service.AddIdentityDomain(ctx, "org-1", "u", "admin", "gmail.com"); !errors.As(err, &invalid) || invalid.Reason != "free_mail" {
		t.Fatalf("add: %v", err)
	}
	if _, err := service.VerifyIdentityDomain(ctx, "org-1", "u", "admin", "10.0.0.1"); !errors.As(err, &invalid) || invalid.Reason != "ip_literal" {
		t.Fatalf("verify: %v", err)
	}
	if err := service.RevokeIdentityDomain(ctx, "org-1", "u", "admin", "co.uk"); !errors.As(err, &invalid) || invalid.Reason != "public_suffix" {
		t.Fatalf("revoke: %v", err)
	}
	if _, err := service.LookupVerifiedDomainOwner(ctx, ""); !errors.As(err, &invalid) || invalid.Reason != "empty" {
		t.Fatalf("lookup: %v", err)
	}
}
