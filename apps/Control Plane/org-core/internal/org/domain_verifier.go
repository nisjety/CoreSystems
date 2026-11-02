package org

import (
	"context"
	"crypto/subtle"
	"errors"
	"net"
	"strings"
	"time"
)

const (
	// DomainChallengeLabel is the DNS label an admin publishes the challenge under.
	DomainChallengeLabel = "_verevon-challenge"
	// DomainChallengeValuePrefix precedes the token in the TXT value.
	DomainChallengeValuePrefix = "verevon-site-verification="

	// DomainDNSTimeout bounds one verification lookup.
	DomainDNSTimeout = 4 * time.Second
)

// Reasons a verification attempt did not prove the domain. They are the
// closed set the API returns in data.reason.
const (
	DomainReasonTXTNotFound     = "txt_not_found"
	DomainReasonTXTMismatch     = "txt_mismatch"
	DomainReasonDNSUnavailable  = "dns_unavailable"
	DomainReasonChallengeExpire = "challenge_expired"
)

// DomainTXTResolver looks up TXT records. The production implementation is a
// pure-Go net.Resolver; tests supply a fake. There is deliberately no HTTP
// method on it: a domain is proven by DNS alone, so verification has no SSRF
// surface.
type DomainTXTResolver interface {
	LookupTXT(ctx context.Context, name string) ([]string, error)
}

// NewDomainTXTResolver returns the pure-Go resolver. With servers empty the
// system resolvers are used; otherwise each "host:port" (port defaults to 53)
// is tried in order for every lookup.
func NewDomainTXTResolver(servers []string) DomainTXTResolver {
	resolver := &net.Resolver{PreferGo: true}
	cleaned := make([]string, 0, len(servers))
	for _, server := range servers {
		server = strings.TrimSpace(server)
		if server == "" {
			continue
		}
		if _, _, err := net.SplitHostPort(server); err != nil {
			server = net.JoinHostPort(server, "53")
		}
		cleaned = append(cleaned, server)
	}
	if len(cleaned) > 0 {
		resolver.Dial = func(ctx context.Context, network, _ string) (net.Conn, error) {
			dialer := net.Dialer{Timeout: DomainDNSTimeout}
			var lastErr error
			for _, address := range cleaned {
				conn, err := dialer.DialContext(ctx, network, address)
				if err == nil {
					return conn, nil
				}
				lastErr = err
			}
			return nil, lastErr
		}
	}
	return netDomainResolver{resolver: resolver}
}

type netDomainResolver struct{ resolver *net.Resolver }

func (r netDomainResolver) LookupTXT(ctx context.Context, name string) ([]string, error) {
	return r.resolver.LookupTXT(ctx, name)
}

// DomainChallengeName is the record name for a normalized domain.
func DomainChallengeName(domain string) string {
	return DomainChallengeLabel + "." + domain
}

// DomainChallengeValue is the record value for a token.
func DomainChallengeValue(token string) string {
	return DomainChallengeValuePrefix + token
}

// checkDomainChallenge resolves the challenge record and compares it, in
// constant time, to the expected value. It returns ok=true when one record
// matches, otherwise the reason. A resolver failure other than "no such
// record" is dns_unavailable so a transient outage is never read as a
// mismatch.
func checkDomainChallenge(ctx context.Context, resolver DomainTXTResolver, domain, token string) (ok bool, reason string) {
	lookupCtx, cancel := context.WithTimeout(ctx, DomainDNSTimeout)
	defer cancel()
	records, err := resolver.LookupTXT(lookupCtx, DomainChallengeName(domain))
	if err != nil {
		var dnsErr *net.DNSError
		if errors.As(err, &dnsErr) && dnsErr.IsNotFound {
			return false, DomainReasonTXTNotFound
		}
		return false, DomainReasonDNSUnavailable
	}
	if len(records) == 0 {
		return false, DomainReasonTXTNotFound
	}
	want := []byte(DomainChallengeValue(token))
	matched := 0
	for _, record := range records {
		got := []byte(strings.TrimSpace(record))
		if len(got) == len(want) && subtle.ConstantTimeCompare(got, want) == 1 {
			matched = 1
		}
	}
	if matched == 1 {
		return true, ""
	}
	return false, DomainReasonTXTMismatch
}
