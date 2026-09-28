package mailtask

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

var errResolverUnavailableForTest = errors.New("mail task alias resolver returned HTTP 503")

type fakeResolver struct {
	byAlias map[string]ResolvedAlias
	// lastProviderMessageID records the last providerMessageID this fake was
	// called with, so a test can assert handler.go forwards the parsed
	// Postmark event's own field rather than an empty/wrong value.
	lastProviderMessageID string
}

func (f *fakeResolver) Resolve(_ context.Context, alias, providerMessageID string) (ResolvedAlias, bool, error) {
	f.lastProviderMessageID = providerMessageID
	resolved, ok := f.byAlias[alias]
	return resolved, ok, nil
}

// spyGateway is a GatewayIngestClient that records every call, so tests can
// assert it was called exactly once (or not at all) -- the idempotency and
// draft-only-pipeline assertions this handler's own tests need.
type spyGateway struct {
	calls []GatewayIngestRequest
	err   error
}

func (s *spyGateway) Ingest(_ context.Context, req GatewayIngestRequest) error {
	s.calls = append(s.calls, req)
	return s.err
}

func testFixtureBody(t *testing.T, authResultsHeader, fromEmail string) []byte {
	t.Helper()
	body := postmarkFixture(t, authResultsHeader, nil)
	if fromEmail != "owner@example.com" {
		body = []byte(strings.ReplaceAll(string(body), "owner@example.com", fromEmail))
	}
	return body
}

func newTestHandler(resolver AliasResolver, gateway GatewayIngestClient) *Handler {
	return &Handler{
		Provider:        "postmark",
		WebhookSecret:   "the-secret",
		ReceivingDomain: "tasks.verevon.ai",
		Resolver:        resolver,
		Extractor:       nil,
		Gateway:         gateway,
		Replay:          NewReplayGuard(time.Hour),
		Now:             func() time.Time { return time.Date(2026, 9, 28, 12, 0, 0, 0, time.UTC) },
	}
}

func TestHandleInboundPostmarkIsDisabledWithoutConfiguration(t *testing.T) {
	h := &Handler{} // Provider/WebhookSecret unset
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "anything"), postmarkFixture(t, "spf=pass; dkim=pass", nil))
	if result.Reason != ReasonDisabled {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonDisabled)
	}
}

func TestHandleInboundPostmarkRejectsMissingWebhookAuth(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)

	result := h.HandleInboundPostmark(context.Background(), "", postmarkFixture(t, "spf=pass; dkim=pass", nil))
	if result.Reason != ReasonUnauthorized {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonUnauthorized)
	}
	if len(gateway.calls) != 0 {
		t.Fatal("gateway must not be called when webhook auth fails")
	}
}

func TestHandleInboundPostmarkRejectsWhenAuthResultIsNotPass(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)

	body := postmarkFixture(t, "spf=fail; dkim=pass", nil)
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if result.Reason != ReasonSenderNotVerified {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonSenderNotVerified)
	}
	if len(gateway.calls) != 0 {
		t.Fatal("gateway must not be called when SPF/DKIM does not pass")
	}
}

func TestHandleInboundPostmarkRejectsWhenFromDoesNotMatchTheVerifiedEmail(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)

	body := testFixtureBody(t, "spf=pass; dkim=pass", "attacker@example.com")
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if result.Reason != ReasonSenderNotVerified {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonSenderNotVerified)
	}
	if len(gateway.calls) != 0 {
		t.Fatal("gateway must not be called when From does not match the alias owner's verified email")
	}
}

// erroringResolver simulates a resolver-level failure (e.g. user-core
// unreachable), distinct from a resolver that correctly reports "no such
// alias".
type erroringResolver struct{ err error }

func (e *erroringResolver) Resolve(context.Context, string, string) (ResolvedAlias, bool, error) {
	return ResolvedAlias{}, false, e.err
}

// TestHandleInboundPostmarkTreatsAResolverFailureAsRetryableNotAsNotFound is
// the regression test for the bug fixed alongside HTTPAliasResolver's own
// status-code handling: a transient resolver failure must reach the HTTP
// layer as something retryable, never collapse into the same outcome as a
// genuine "no such alias" (which the spec requires to answer 200 and never
// be retried).
func TestHandleInboundPostmarkTreatsAResolverFailureAsRetryableNotAsNotFound(t *testing.T) {
	gateway := &spyGateway{}
	h := newTestHandler(&erroringResolver{err: errResolverUnavailableForTest}, gateway)

	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if result.Reason != ReasonAliasResolverUnavailable {
		t.Fatalf("Reason = %q, want %q (not %q, which the HTTP layer answers 200 to and never retries)",
			result.Reason, ReasonAliasResolverUnavailable, ReasonAliasNotFound)
	}
	if result.Err == nil {
		t.Fatal("Err = nil, want the resolver's own error preserved for the log line")
	}
	if len(gateway.calls) != 0 {
		t.Fatal("gateway must not be called when the resolver itself failed")
	}
}

func TestHandleInboundPostmarkRejectsUnknownAlias(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)

	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if result.Reason != ReasonAliasNotFound {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonAliasNotFound)
	}
}

func TestHandleInboundPostmarkAcceptsTheMatchingCase(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)

	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if result.Reason != ReasonAccepted {
		t.Fatalf("Reason = %q, Err = %v, want %q", result.Reason, result.Err, ReasonAccepted)
	}
	if len(gateway.calls) != 1 {
		t.Fatalf("gateway calls = %d, want exactly 1", len(gateway.calls))
	}
	call := gateway.calls[0]
	if call.OrgID != "org-1" || call.UserID != "user-1" || call.ProviderMessageID != "provider-msg-1" {
		t.Fatalf("gateway call = %+v", call)
	}
}

func TestHandleInboundPostmarkReplayOfTheSameProviderMessageIDIsANoOp(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)

	first := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if first.Reason != ReasonAccepted {
		t.Fatalf("first delivery Reason = %q, want %q", first.Reason, ReasonAccepted)
	}

	second := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if second.Reason != ReasonDuplicate {
		t.Fatalf("replay Reason = %q, want %q", second.Reason, ReasonDuplicate)
	}

	if len(gateway.calls) != 1 {
		t.Fatalf("gateway calls = %d, want exactly 1 (replay must not call it again)", len(gateway.calls))
	}
}

func TestHandleInboundPostmarkRejectsWrongReceivingDomain(t *testing.T) {
	resolver := &fakeResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
	gateway := &spyGateway{}
	h := newTestHandler(resolver, gateway)
	h.ReceivingDomain = "other-domain.example.com"

	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	result := h.HandleInboundPostmark(context.Background(), basicAuthHeader("postmark", "the-secret"), body)
	if result.Reason != ReasonWrongDomain {
		t.Fatalf("Reason = %q, want %q", result.Reason, ReasonWrongDomain)
	}
	if len(gateway.calls) != 0 {
		t.Fatal("gateway must not be called for a mismatched receiving domain")
	}
}
