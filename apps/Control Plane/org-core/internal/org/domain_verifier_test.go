package org

import (
	"context"
	"errors"
	"net"
	"testing"
	"time"
)

// WS05-S6: the DNS proof check with a fake resolver. No network.

type fakeTXTResolver struct {
	records map[string][]string
	err     error
	asked   []string
	// block makes the lookup wait for its context, to prove the timeout bound.
	block bool
	// deadline records whether the lookup context carried a deadline.
	hadDeadline bool
}

func (f *fakeTXTResolver) LookupTXT(ctx context.Context, name string) ([]string, error) {
	f.asked = append(f.asked, name)
	_, f.hadDeadline = ctx.Deadline()
	if f.block {
		<-ctx.Done()
		return nil, ctx.Err()
	}
	if f.err != nil {
		return nil, f.err
	}
	return f.records[name], nil
}

const testToken = "tokentokentokentokentoken0123456789"

func TestCheckDomainChallenge(t *testing.T) {
	name := DomainChallengeName("example.com")
	good := DomainChallengeValue(testToken)
	cases := []struct {
		name       string
		resolver   *fakeTXTResolver
		wantOK     bool
		wantReason string
	}{
		{"match", &fakeTXTResolver{records: map[string][]string{name: {good}}}, true, ""},
		{"match among others", &fakeTXTResolver{records: map[string][]string{name: {"v=spf1 -all", "other", good}}}, true, ""},
		{"match with surrounding space", &fakeTXTResolver{records: map[string][]string{name: {"  " + good + " "}}}, true, ""},
		{"mismatch wrong token", &fakeTXTResolver{records: map[string][]string{name: {DomainChallengeValue("nottherightone")}}}, false, DomainReasonTXTMismatch},
		{"mismatch missing prefix", &fakeTXTResolver{records: map[string][]string{name: {testToken}}}, false, DomainReasonTXTMismatch},
		{"mismatch prefix of token", &fakeTXTResolver{records: map[string][]string{name: {good[:len(good)-1]}}}, false, DomainReasonTXTMismatch},
		{"mismatch token with suffix", &fakeTXTResolver{records: map[string][]string{name: {good + "x"}}}, false, DomainReasonTXTMismatch},
		{"mismatch only unrelated records", &fakeTXTResolver{records: map[string][]string{name: {"v=spf1 -all"}}}, false, DomainReasonTXTMismatch},
		{"empty answer", &fakeTXTResolver{records: map[string][]string{}}, false, DomainReasonTXTNotFound},
		{"record on another name", &fakeTXTResolver{records: map[string][]string{"_verevon-challenge.other.com": {good}}}, false, DomainReasonTXTNotFound},
		{"dns not found", &fakeTXTResolver{err: &net.DNSError{Err: "no such host", Name: name, IsNotFound: true}}, false, DomainReasonTXTNotFound},
		{"dns server failure", &fakeTXTResolver{err: &net.DNSError{Err: "server misbehaving", Name: name, IsTemporary: true}}, false, DomainReasonDNSUnavailable},
		{"dns timeout", &fakeTXTResolver{err: &net.DNSError{Err: "i/o timeout", Name: name, IsTimeout: true}}, false, DomainReasonDNSUnavailable},
		{"other error", &fakeTXTResolver{err: errors.New("connection refused")}, false, DomainReasonDNSUnavailable},
		{"wrapped not-found is still not found", &fakeTXTResolver{err: errors.Join(errors.New("lookup"), &net.DNSError{IsNotFound: true})}, false, DomainReasonTXTNotFound},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			ok, reason := checkDomainChallenge(context.Background(), tc.resolver, "example.com", testToken)
			if ok != tc.wantOK || reason != tc.wantReason {
				t.Fatalf("got (%v, %q), want (%v, %q)", ok, reason, tc.wantOK, tc.wantReason)
			}
			if len(tc.resolver.asked) != 1 || tc.resolver.asked[0] != name {
				t.Fatalf("resolver asked %v, want exactly %q", tc.resolver.asked, name)
			}
			if !tc.resolver.hadDeadline {
				t.Fatal("every lookup must run under a deadline")
			}
		})
	}
}

func TestCheckDomainChallengeBoundsASlowResolver(t *testing.T) {
	resolver := &fakeTXTResolver{block: true}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started := time.Now()
	// Cancel the parent soon so the test does not wait the full DomainDNSTimeout.
	go func() { time.Sleep(50 * time.Millisecond); cancel() }()
	ok, reason := checkDomainChallenge(ctx, resolver, "example.com", testToken)
	if ok || reason != DomainReasonDNSUnavailable {
		t.Fatalf("a stuck resolver must read as dns_unavailable, got (%v, %q)", ok, reason)
	}
	if time.Since(started) > 2*time.Second {
		t.Fatal("the lookup did not honour its context")
	}
}

func TestDomainTimeoutConstantsAreSane(t *testing.T) {
	if DomainDNSTimeout <= 0 || DomainDNSTimeout > 10*time.Second {
		t.Fatalf("DomainDNSTimeout = %v", DomainDNSTimeout)
	}
	if DomainChallengeTTL != 7*24*time.Hour {
		t.Fatalf("DomainChallengeTTL = %v", DomainChallengeTTL)
	}
	if MaxIdentityDomainsPerOrg != 10 {
		t.Fatalf("MaxIdentityDomainsPerOrg = %d", MaxIdentityDomainsPerOrg)
	}
}

func TestNewDomainTXTResolverCleansTheServerList(t *testing.T) {
	// Constructing must not dial anything; blank entries are dropped, ports defaulted.
	if NewDomainTXTResolver(nil) == nil || NewDomainTXTResolver([]string{"", "  ", "1.1.1.1", "9.9.9.9:5353"}) == nil {
		t.Fatal("constructor returned nil")
	}
}
