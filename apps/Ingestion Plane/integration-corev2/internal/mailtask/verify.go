package mailtask

import (
	"crypto/subtle"
	"encoding/base64"
	"strings"
)

// VerifyWebhookAuth checks the inbound Postmark webhook request's HTTP Basic
// credentials against the configured secret. Postmark's inbound webhook
// setup embeds "user:pass" in the target URL; Postmark's own servers send
// that back as a standard Authorization: Basic header on every delivery --
// there is no separate payload-signing scheme to check instead (see
// https://postmarkapp.com/support/article/1197-parse-an-inbound-email).
// An empty expectedSecret always fails closed (off by default, spec §7.1).
func VerifyWebhookAuth(authorizationHeader, expectedSecret string) bool {
	expectedSecret = strings.TrimSpace(expectedSecret)
	if expectedSecret == "" {
		return false
	}
	scheme, credentials, ok := strings.Cut(strings.TrimSpace(authorizationHeader), " ")
	if !ok || !strings.EqualFold(scheme, "Basic") {
		return false
	}
	decoded, err := base64.StdEncoding.DecodeString(strings.TrimSpace(credentials))
	if err != nil {
		return false
	}
	_, password, ok := strings.Cut(string(decoded), ":")
	if !ok {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(password), []byte(expectedSecret)) == 1
}

// VerifySender enforces the single non-optional anti-spoofing control (spec
// §6): the provider's own SPF+DKIM verdict must be "pass" AND the message's
// From address must case-fold-match the alias owner's OWN verified email
// address -- self-forward only, per the 2026-09-28 owner ruling. Both
// conditions are required; SPF/DKIM passing only proves the SENDING domain
// controls its own mail, not that the sender is this alias's owner (spec §6:
// "SPF/DKIM/DMARC pass alone is not sufficient").
func VerifySender(event InboundMailTaskEvent, resolvedVerifiedEmail string) bool {
	return senderRejectionCause(event, resolvedVerifiedEmail) == ""
}

// senderRejectionCause is VerifySender's decision with the reason attached: ""
// when the sender is verified, otherwise the first failing check (one of the
// senderCause* constants, which are content-free and safe to log or count).
// VerifySender is defined in terms of it, so the two can never disagree.
func senderRejectionCause(event InboundMailTaskEvent, resolvedVerifiedEmail string) string {
	if !strings.EqualFold(strings.TrimSpace(event.AuthResult), "pass") {
		return senderCauseAuthNotPass
	}
	resolvedVerifiedEmail = strings.TrimSpace(resolvedVerifiedEmail)
	if resolvedVerifiedEmail == "" {
		return senderCauseNoVerifiedEmail
	}
	if !strings.EqualFold(strings.TrimSpace(event.FromEmail), resolvedVerifiedEmail) {
		return senderCauseFromMismatch
	}
	return ""
}

// VerifyReceivingDomain checks the inbound message's To: domain against the
// configured receiving domain, as defense in depth alongside the provider's
// own domain-scoped inbound routing. An empty expectedDomain always fails
// closed (off by default).
func VerifyReceivingDomain(toDomain, expectedDomain string) bool {
	expectedDomain = strings.ToLower(strings.TrimSpace(expectedDomain))
	if expectedDomain == "" {
		return false
	}
	return strings.EqualFold(strings.TrimSpace(toDomain), expectedDomain)
}
