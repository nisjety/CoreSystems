package mailtask

import (
	"context"
	"errors"
	"strings"
	"sync"
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

// scriptedGateway returns errs[i] on call i (nil once errs is exhausted) and
// records every request it receives. Unlike spyGateway it is safe for
// concurrent use, which the concurrent-delivery tests need. A non-nil entered
// channel gets one value per call that reached Ingest, a non-nil release
// channel holds every call inside Ingest until it is closed, and delay makes
// each call linger, so a test can hold one delivery in flight deterministically.
type scriptedGateway struct {
	mu      sync.Mutex
	errs    []error
	calls   []GatewayIngestRequest
	entered chan struct{}
	release chan struct{}
	delay   time.Duration
}

func (s *scriptedGateway) Ingest(_ context.Context, req GatewayIngestRequest) error {
	s.mu.Lock()
	index := len(s.calls)
	s.calls = append(s.calls, req)
	var err error
	if index < len(s.errs) {
		err = s.errs[index]
	}
	entered, release, delay := s.entered, s.release, s.delay
	s.mu.Unlock()
	if entered != nil {
		entered <- struct{}{}
	}
	if release != nil {
		<-release
	}
	if delay > 0 {
		time.Sleep(delay)
	}
	return err
}

func (s *scriptedGateway) callCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.calls)
}

// scriptedResolver returns errs[i] on call i, and otherwise looks the alias up
// in byAlias. It counts every call so a test can assert whether a replay
// re-resolved the alias, and it is safe for concurrent use.
type scriptedResolver struct {
	mu      sync.Mutex
	errs    []error
	byAlias map[string]ResolvedAlias
	calls   int
}

func (r *scriptedResolver) Resolve(_ context.Context, alias, _ string) (ResolvedAlias, bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	index := r.calls
	r.calls++
	if index < len(r.errs) && r.errs[index] != nil {
		return ResolvedAlias{}, false, r.errs[index]
	}
	resolved, ok := r.byAlias[alias]
	return resolved, ok, nil
}

func (r *scriptedResolver) callCount() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.calls
}

func ownerAliasResolver() *scriptedResolver {
	return &scriptedResolver{byAlias: map[string]ResolvedAlias{
		"abc123def456": {OrgID: "org-1", UserID: "user-1", VerifiedEmail: "owner@example.com"},
	}}
}

// J1: a delivery that failed downstream was answered 503 so Postmark would
// retry it. The retry must be processed, not dropped as a duplicate of the
// attempt that failed.
func TestHandleInboundPostmarkRetryAfterAGatewayFailureIsReprocessedNotDropped(t *testing.T) {
	gateway := &scriptedGateway{errs: []error{errors.New("mail task gateway ingest returned HTTP 503")}}
	h := newTestHandler(ownerAliasResolver(), gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	first := h.HandleInboundPostmark(context.Background(), auth, body)
	if first.Reason != ReasonGatewayUnavailable {
		t.Fatalf("first delivery Reason = %q, want %q", first.Reason, ReasonGatewayUnavailable)
	}

	retry := h.HandleInboundPostmark(context.Background(), auth, body)
	if retry.Reason != ReasonAccepted {
		t.Fatalf("Postmark retry Reason = %q, Err = %v, want %q (a failed delivery must never be recorded as a duplicate)",
			retry.Reason, retry.Err, ReasonAccepted)
	}
	if got := gateway.callCount(); got != 2 {
		t.Fatalf("gateway calls = %d, want 2 (the retry must reach Gateway.Ingest again)", got)
	}

	// Once the retry was accepted the message is done: a further replay is a
	// duplicate and does not reach the gateway a third time.
	again := h.HandleInboundPostmark(context.Background(), auth, body)
	if again.Reason != ReasonDuplicate {
		t.Fatalf("replay after acceptance Reason = %q, want %q", again.Reason, ReasonDuplicate)
	}
	if got := gateway.callCount(); got != 2 {
		t.Fatalf("gateway calls = %d, want still 2 after the replay", got)
	}
}

func TestHandleInboundPostmarkRetryAfterAMissingGatewayIsReprocessedNotDropped(t *testing.T) {
	h := newTestHandler(ownerAliasResolver(), nil)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	first := h.HandleInboundPostmark(context.Background(), auth, body)
	if first.Reason != ReasonGatewayUnavailable {
		t.Fatalf("first delivery Reason = %q, want %q", first.Reason, ReasonGatewayUnavailable)
	}

	gateway := &scriptedGateway{}
	h.Gateway = gateway
	retry := h.HandleInboundPostmark(context.Background(), auth, body)
	if retry.Reason != ReasonAccepted {
		t.Fatalf("retry Reason = %q, want %q", retry.Reason, ReasonAccepted)
	}
	if got := gateway.callCount(); got != 1 {
		t.Fatalf("gateway calls = %d, want 1", got)
	}
}

func TestHandleInboundPostmarkRetryAfterAResolverFailureIsReprocessedNotDropped(t *testing.T) {
	resolver := ownerAliasResolver()
	resolver.errs = []error{errResolverUnavailableForTest}
	gateway := &scriptedGateway{}
	h := newTestHandler(resolver, gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	first := h.HandleInboundPostmark(context.Background(), auth, body)
	if first.Reason != ReasonAliasResolverUnavailable {
		t.Fatalf("first delivery Reason = %q, want %q", first.Reason, ReasonAliasResolverUnavailable)
	}

	retry := h.HandleInboundPostmark(context.Background(), auth, body)
	if retry.Reason != ReasonAccepted {
		t.Fatalf("Postmark retry Reason = %q, Err = %v, want %q (a failed delivery must never be recorded as a duplicate)",
			retry.Reason, retry.Err, ReasonAccepted)
	}
	if got := resolver.callCount(); got != 2 {
		t.Fatalf("resolver calls = %d, want 2", got)
	}
	if got := gateway.callCount(); got != 1 {
		t.Fatalf("gateway calls = %d, want 1", got)
	}
}

// A dropped forgery is answered 200, so Postmark never retries it. A replay of
// the same MessageID is a duplicate and must not re-resolve the alias.
func TestHandleInboundPostmarkReplayOfARejectedSenderIsADuplicateWithoutReResolving(t *testing.T) {
	resolver := ownerAliasResolver()
	gateway := &scriptedGateway{}
	h := newTestHandler(resolver, gateway)
	body := testFixtureBody(t, "spf=pass; dkim=pass", "attacker@example.com")
	auth := basicAuthHeader("postmark", "the-secret")

	first := h.HandleInboundPostmark(context.Background(), auth, body)
	if first.Reason != ReasonSenderNotVerified {
		t.Fatalf("first delivery Reason = %q, want %q", first.Reason, ReasonSenderNotVerified)
	}
	replay := h.HandleInboundPostmark(context.Background(), auth, body)
	if replay.Reason != ReasonDuplicate {
		t.Fatalf("replay Reason = %q, want %q", replay.Reason, ReasonDuplicate)
	}
	if got := resolver.callCount(); got != 1 {
		t.Fatalf("resolver calls = %d, want 1 (the replay must not re-resolve)", got)
	}
	if got := gateway.callCount(); got != 0 {
		t.Fatalf("gateway calls = %d, want 0", got)
	}
}

func TestHandleInboundPostmarkReplayOfAnUnknownAliasIsADuplicateWithoutReResolving(t *testing.T) {
	resolver := &scriptedResolver{byAlias: map[string]ResolvedAlias{}}
	gateway := &scriptedGateway{}
	h := newTestHandler(resolver, gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	first := h.HandleInboundPostmark(context.Background(), auth, body)
	if first.Reason != ReasonAliasNotFound {
		t.Fatalf("first delivery Reason = %q, want %q", first.Reason, ReasonAliasNotFound)
	}
	replay := h.HandleInboundPostmark(context.Background(), auth, body)
	if replay.Reason != ReasonDuplicate {
		t.Fatalf("replay Reason = %q, want %q", replay.Reason, ReasonDuplicate)
	}
	if got := resolver.callCount(); got != 1 {
		t.Fatalf("resolver calls = %d, want 1 (the replay must not re-resolve)", got)
	}
}

// A payload without a MessageID has nothing to deduplicate on. Two unrelated
// such messages must not collapse into one shared "empty id" key and have the
// second silently dropped as a duplicate of the first.
func TestHandleInboundPostmarkDoesNotDeduplicateMessagesWithoutAProviderMessageID(t *testing.T) {
	gateway := &scriptedGateway{}
	h := newTestHandler(ownerAliasResolver(), gateway)
	body := []byte(strings.Replace(string(postmarkFixture(t, "spf=pass; dkim=pass", nil)),
		`"MessageID":"provider-msg-1"`, `"MessageID":""`, 1))
	if !strings.Contains(string(body), `"MessageID":""`) {
		t.Fatal("fixture rewrite did not clear the MessageID")
	}
	auth := basicAuthHeader("postmark", "the-secret")

	for attempt := 1; attempt <= 2; attempt++ {
		result := h.HandleInboundPostmark(context.Background(), auth, body)
		if result.Reason != ReasonAccepted {
			t.Fatalf("delivery %d Reason = %q, want %q (no MessageID means nothing to dedupe on)", attempt, result.Reason, ReasonAccepted)
		}
	}
	if got := gateway.callCount(); got != 2 {
		t.Fatalf("gateway calls = %d, want 2", got)
	}
}

// heldGateway builds a scriptedGateway whose calls block inside Ingest until
// the returned unblock func runs (also registered as a cleanup, so a failing
// test never leaks a blocked goroutine).
func heldGateway(t *testing.T, errs ...error) (*scriptedGateway, func()) {
	t.Helper()
	gateway := &scriptedGateway{errs: errs, entered: make(chan struct{}, 16), release: make(chan struct{})}
	var once sync.Once
	unblock := func() { once.Do(func() { close(gateway.release) }) }
	t.Cleanup(unblock)
	return gateway, unblock
}

func waitForResult(t *testing.T, results <-chan Result, what string) Result {
	t.Helper()
	select {
	case result := <-results:
		return result
	case <-time.After(5 * time.Second):
		t.Fatalf("timed out waiting for %s", what)
		return Result{}
	}
}

// J1: two simultaneous deliveries of one MessageID must not both reach
// Gateway.Ingest. The loser is told to retry (never 200): the delivery in
// flight may still fail, and acknowledging the loser would tell Postmark the
// message was delivered.
func TestHandleInboundPostmarkBouncesASecondDeliveryWhileOneIsInFlight(t *testing.T) {
	gateway, unblock := heldGateway(t)
	h := newTestHandler(ownerAliasResolver(), gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	first := make(chan Result, 1)
	go func() { first <- h.HandleInboundPostmark(context.Background(), auth, body) }()
	select {
	case <-gateway.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("the first delivery never reached the gateway")
	}

	second := h.HandleInboundPostmark(context.Background(), auth, body)
	if second.Reason != ReasonDeliveryInFlight {
		t.Fatalf("second concurrent delivery Reason = %q, want %q", second.Reason, ReasonDeliveryInFlight)
	}
	if got := gateway.callCount(); got != 1 {
		t.Fatalf("gateway calls = %d, want exactly 1 while the first delivery is in flight", got)
	}

	unblock()
	if got := waitForResult(t, first, "the first delivery"); got.Reason != ReasonAccepted {
		t.Fatalf("first delivery Reason = %q, Err = %v, want %q", got.Reason, got.Err, ReasonAccepted)
	}

	third := h.HandleInboundPostmark(context.Background(), auth, body)
	if third.Reason != ReasonDuplicate {
		t.Fatalf("delivery after the first was accepted Reason = %q, want %q", third.Reason, ReasonDuplicate)
	}
	if got := gateway.callCount(); got != 1 {
		t.Fatalf("gateway calls = %d, want exactly 1 overall", got)
	}
}

// The in-flight claim must be cleared on failure, otherwise the retry that the
// bounced delivery was told to make would itself be bounced forever.
func TestHandleInboundPostmarkClearsTheInFlightClaimWhenTheDeliveryFails(t *testing.T) {
	gateway, unblock := heldGateway(t, errors.New("mail task gateway ingest returned HTTP 503"))
	h := newTestHandler(ownerAliasResolver(), gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	first := make(chan Result, 1)
	go func() { first <- h.HandleInboundPostmark(context.Background(), auth, body) }()
	select {
	case <-gateway.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("the first delivery never reached the gateway")
	}
	if second := h.HandleInboundPostmark(context.Background(), auth, body); second.Reason != ReasonDeliveryInFlight {
		t.Fatalf("second concurrent delivery Reason = %q, want %q", second.Reason, ReasonDeliveryInFlight)
	}

	unblock()
	if got := waitForResult(t, first, "the first delivery"); got.Reason != ReasonGatewayUnavailable {
		t.Fatalf("first delivery Reason = %q, want %q", got.Reason, ReasonGatewayUnavailable)
	}

	retry := h.HandleInboundPostmark(context.Background(), auth, body)
	if retry.Reason != ReasonAccepted {
		t.Fatalf("retry after the failed delivery Reason = %q, Err = %v, want %q", retry.Reason, retry.Err, ReasonAccepted)
	}
	if got := gateway.callCount(); got != 2 {
		t.Fatalf("gateway calls = %d, want 2 (the failed attempt and the retry)", got)
	}
}

// Run with -race: many simultaneous deliveries of one MessageID, exactly one
// of which may win and reach Gateway.Ingest.
func TestHandleInboundPostmarkConcurrentDeliveriesOfOneMessageIDReachTheGatewayOnce(t *testing.T) {
	const deliveries = 32
	gateway := &scriptedGateway{delay: 20 * time.Millisecond}
	h := newTestHandler(ownerAliasResolver(), gateway)
	body := postmarkFixture(t, "spf=pass; dkim=pass", nil)
	auth := basicAuthHeader("postmark", "the-secret")

	var (
		wg      sync.WaitGroup
		mu      sync.Mutex
		reasons = map[Reason]int{}
		start   = make(chan struct{})
	)
	for i := 0; i < deliveries; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			result := h.HandleInboundPostmark(context.Background(), auth, body)
			mu.Lock()
			reasons[result.Reason]++
			mu.Unlock()
		}()
	}
	close(start)
	wg.Wait()

	if got := gateway.callCount(); got != 1 {
		t.Fatalf("gateway calls = %d, want exactly 1 (outcomes: %v)", got, reasons)
	}
	if reasons[ReasonAccepted] != 1 {
		t.Fatalf("accepted = %d, want exactly 1 (outcomes: %v)", reasons[ReasonAccepted], reasons)
	}
	if other := deliveries - 1 - reasons[ReasonDuplicate] - reasons[ReasonDeliveryInFlight]; other != 0 {
		t.Fatalf("%d deliveries ended in an unexpected outcome (outcomes: %v)", other, reasons)
	}
}
