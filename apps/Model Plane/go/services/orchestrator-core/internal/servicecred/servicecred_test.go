package servicecred

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// fakeMinter records what it was asked for so tests assert the tenant the
// interceptor derived, not just that some token was attached.
type fakeMinter struct {
	mu       sync.Mutex
	audience string
	// minted is what the fake hands back; mintedSeq overrides it call-by-call
	// for the re-mint test. Named to avoid reading as a credential assignment.
	minted      string
	mintedSeq   []string
	err         error
	orgs        []string
	invalidated []string
}

func (f *fakeMinter) Token(_ context.Context, orgID string) (string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.orgs = append(f.orgs, orgID)
	if f.err != nil {
		return "", f.err
	}
	if len(f.mintedSeq) > 0 {
		next := f.mintedSeq[0]
		f.mintedSeq = f.mintedSeq[1:]
		return next, nil
	}
	return f.minted, nil
}

func (f *fakeMinter) Invalidate(orgID string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.invalidated = append(f.invalidated, orgID)
}

func (f *fakeMinter) Audience() string { return f.audience }

// orgRequest stands in for a generated request message carrying org_id.
type orgRequest struct{ org string }

func (o orgRequest) GetOrgId() string { return o.org }

// orglessRequest stands in for capability-core's skill RPCs, whose protos have
// no org field at all.
type orglessRequest struct{}

// capturingInvoker records the outbound metadata each call carried.
type capturingInvoker struct {
	calls []metadata.MD
	// errs is returned per call index, so a test can make the first attempt
	// fail Unauthenticated and the second succeed.
	errs []error
}

func (c *capturingInvoker) invoke(
	ctx context.Context, _ string, _, _ any, _ *grpc.ClientConn, _ ...grpc.CallOption,
) error {
	md, _ := metadata.FromOutgoingContext(ctx)
	c.calls = append(c.calls, md.Copy())
	if len(c.errs) > 0 {
		err := c.errs[0]
		c.errs = c.errs[1:]
		return err
	}
	return nil
}

func (c *capturingInvoker) authorizationAt(i int) string {
	if i >= len(c.calls) {
		return ""
	}
	values := c.calls[i].Get(metadataAuthorization)
	if len(values) == 0 {
		return ""
	}
	return values[0]
}

// The proxy path must win: when a real user's bearer is present, the callee has
// to see that user's authority, never orchestrator-core's service identity.
// Minting here would silently escalate a user request to service privileges.
func TestUnaryInterceptor_ForwardsInboundCredentialInsteadOfMinting(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "service-token"}
	inv := &capturingInvoker{}
	ctx := metadata.NewIncomingContext(
		context.Background(),
		metadata.Pairs(metadataAuthorization, "Bearer user-token"),
	)

	err := UnaryInterceptor(minter, nil)(
		ctx, "/mp.v1.SessionCore/StartRun", orgRequest{org: "org-1"}, nil, nil, inv.invoke)
	if err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if got := inv.authorizationAt(0); got != "Bearer user-token" {
		t.Fatalf("authorization = %q, want the forwarded user bearer", got)
	}
	if len(minter.orgs) != 0 {
		t.Fatalf("minted %v tokens on the proxy path; must forward instead", minter.orgs)
	}
}

// Inbound metadata that carries no bearer is not a forwardable credential.
// Treating it as one would send an uncredentialed call while looking like the
// proxy path succeeded.
func TestUnaryInterceptor_MintsWhenInboundMetadataHasNoBearer(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "service-token"}
	inv := &capturingInvoker{}
	ctx := metadata.NewIncomingContext(
		context.Background(), metadata.Pairs("x-request-id", "abc"))

	if err := UnaryInterceptor(minter, nil)(
		ctx, "/mp.v1.SessionCore/StartRun", orgRequest{org: "org-1"}, nil, nil, inv.invoke,
	); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if got := inv.authorizationAt(0); got != "Bearer service-token" {
		t.Fatalf("authorization = %q, want a minted bearer", got)
	}
}

// An empty authorization value is equally not a credential.
func TestUnaryInterceptor_MintsWhenInboundBearerIsBlank(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "service-token"}
	inv := &capturingInvoker{}
	ctx := metadata.NewIncomingContext(
		context.Background(), metadata.Pairs(metadataAuthorization, "   "))

	if err := UnaryInterceptor(minter, nil)(
		ctx, "/mp.v1.SessionCore/StartRun", orgRequest{org: "org-1"}, nil, nil, inv.invoke,
	); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if got := inv.authorizationAt(0); got != "Bearer service-token" {
		t.Fatalf("authorization = %q, want a minted bearer", got)
	}
}

// The activity path: no inbound metadata at all, org taken off the request.
func TestUnaryInterceptor_MintsForRequestOrgOnActivityPath(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "service-token"}
	inv := &capturingInvoker{}

	if err := UnaryInterceptor(minter, nil)(
		context.Background(), "/mp.v1.SessionCore/StartRun",
		orgRequest{org: "org-42"}, nil, nil, inv.invoke,
	); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if got := inv.authorizationAt(0); got != "Bearer service-token" {
		t.Fatalf("authorization = %q, want a minted bearer", got)
	}
	if len(minter.orgs) != 1 || minter.orgs[0] != "org-42" {
		t.Fatalf("minted for %v, want [org-42] read off the request", minter.orgs)
	}
}

// capability-core's org-less RPCs get their tenant from WithOrg.
func TestUnaryInterceptor_UsesWithOrgWhenRequestHasNoOrgField(t *testing.T) {
	minter := &fakeMinter{audience: AudienceCapabilityCore, minted: "cap-token"}
	inv := &capturingInvoker{}
	ctx := WithOrg(context.Background(), "org-7")

	if err := UnaryInterceptor(minter, nil)(
		ctx, "/mp.v1.CapabilityCore/PromoteSkill", orglessRequest{}, nil, nil, inv.invoke,
	); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if len(minter.orgs) != 1 || minter.orgs[0] != "org-7" {
		t.Fatalf("minted for %v, want [org-7] from WithOrg", minter.orgs)
	}
}

// WithOrg takes precedence, so a caller can be explicit even for a message type
// that has an org field but left it empty.
func TestUnaryInterceptor_WithOrgWinsOverRequestOrg(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "t"}
	inv := &capturingInvoker{}
	ctx := WithOrg(context.Background(), "explicit-org")

	if err := UnaryInterceptor(minter, nil)(
		ctx, "/m", orgRequest{org: "message-org"}, nil, nil, inv.invoke,
	); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if minter.orgs[0] != "explicit-org" {
		t.Fatalf("minted for %q, want the explicit WithOrg tenant", minter.orgs[0])
	}
}

// No org is a caller bug and must surface as such. Defaulting the tenant would
// mint a token that authenticates and is then refused on every call, which reads
// as a baffling permission error far from its cause.
func TestUnaryInterceptor_RefusesWhenNoOrgIsResolvable(t *testing.T) {
	minter := &fakeMinter{audience: AudienceCapabilityCore, minted: "t"}
	inv := &capturingInvoker{}

	err := UnaryInterceptor(minter, nil)(
		context.Background(), "/mp.v1.CapabilityCore/PromoteSkill",
		orglessRequest{}, nil, nil, inv.invoke)
	if status.Code(err) != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument", status.Code(err))
	}
	if !strings.Contains(err.Error(), AudienceCapabilityCore) {
		t.Fatalf("error %q should name the audience", err)
	}
	if len(inv.calls) != 0 {
		t.Fatal("call was invoked despite having no tenant to mint for")
	}
}

// An unconfigured deployment must keep the behavior it had before minting
// existed: forward whatever is there. Failing instead would take down the
// working proxy path to punish an unconfigured activity path.
func TestUnaryInterceptor_NilMinterFallsBackToForwarding(t *testing.T) {
	inv := &capturingInvoker{}
	ctx := metadata.NewIncomingContext(
		context.Background(), metadata.Pairs(metadataAuthorization, "Bearer user-token"))

	if err := UnaryInterceptor(nil, nil)(
		ctx, "/m", orgRequest{org: "org-1"}, nil, nil, inv.invoke); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if got := inv.authorizationAt(0); got != "Bearer user-token" {
		t.Fatalf("authorization = %q, want the inbound bearer forwarded", got)
	}
}

// A rejected cached token is re-minted exactly once. This covers a credential
// rotated underneath a live process — the refresh margin cannot see that coming.
func TestUnaryInterceptor_RemintsOnceOnUnauthenticated(t *testing.T) {
	minter := &fakeMinter{
		audience:  AudienceSessionCore,
		mintedSeq: []string{"stale-token", "fresh-token"},
	}
	inv := &capturingInvoker{errs: []error{status.Error(codes.Unauthenticated, "expired")}}

	if err := UnaryInterceptor(minter, nil)(
		context.Background(), "/m", orgRequest{org: "org-1"}, nil, nil, inv.invoke,
	); err != nil {
		t.Fatalf("interceptor returned %v after re-mint", err)
	}
	if len(inv.calls) != 2 {
		t.Fatalf("invoked %d times, want 2 (original + one retry)", len(inv.calls))
	}
	if got := inv.authorizationAt(0); got != "Bearer stale-token" {
		t.Fatalf("first attempt carried %q", got)
	}
	if got := inv.authorizationAt(1); got != "Bearer fresh-token" {
		t.Fatalf("retry carried %q, want the re-minted token", got)
	}
	if len(minter.invalidated) != 1 || minter.invalidated[0] != "org-1" {
		t.Fatalf("invalidated %v, want the stale entry dropped once", minter.invalidated)
	}
}

// A second Unauthenticated is a real authorization answer, not a stale token, so
// it is returned rather than retried forever.
func TestUnaryInterceptor_DoesNotRetryTwice(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "t"}
	inv := &capturingInvoker{errs: []error{
		status.Error(codes.Unauthenticated, "nope"),
		status.Error(codes.Unauthenticated, "still nope"),
	}}

	err := UnaryInterceptor(minter, nil)(
		context.Background(), "/m", orgRequest{org: "org-1"}, nil, nil, inv.invoke)
	if status.Code(err) != codes.Unauthenticated {
		t.Fatalf("code = %v, want the second Unauthenticated returned", status.Code(err))
	}
	if len(inv.calls) != 2 {
		t.Fatalf("invoked %d times, want exactly 2", len(inv.calls))
	}
}

// A non-auth error is returned untouched — no re-mint, no retry. Retrying a
// PermissionDenied or an Internal would double every side effect.
func TestUnaryInterceptor_DoesNotRetryNonAuthErrors(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "t"}
	inv := &capturingInvoker{errs: []error{status.Error(codes.PermissionDenied, "denied")}}

	err := UnaryInterceptor(minter, nil)(
		context.Background(), "/m", orgRequest{org: "org-1"}, nil, nil, inv.invoke)
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", status.Code(err))
	}
	if len(inv.calls) != 1 {
		t.Fatalf("invoked %d times, want 1", len(inv.calls))
	}
	if len(minter.invalidated) != 0 {
		t.Fatal("invalidated the token for a non-auth failure")
	}
}

// A mint failure must not be reported as a transport problem: the activities
// treat Unavailable as "skip, non-fatal", so surfacing a broken credential as
// Unavailable would make every run silently succeed having done nothing.
func TestUnaryInterceptor_MintFailureIsUnauthenticatedNotUnavailable(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, err: errors.New("issuer down")}
	inv := &capturingInvoker{}

	err := UnaryInterceptor(minter, nil)(
		context.Background(), "/m", orgRequest{org: "org-1"}, nil, nil, inv.invoke)
	if status.Code(err) != codes.Unauthenticated {
		t.Fatalf("code = %v, want Unauthenticated", status.Code(err))
	}
	if len(inv.calls) != 0 {
		t.Fatal("called the sibling without a credential")
	}
}

// The retry must replace the bearer, not append a second one.
func TestWithBearer_ReplacesRatherThanStacks(t *testing.T) {
	ctx := withBearer(context.Background(), "first")
	ctx = withBearer(ctx, "second")
	md, _ := metadata.FromOutgoingContext(ctx)
	values := md.Get(metadataAuthorization)
	if len(values) != 1 || values[0] != "Bearer second" {
		t.Fatalf("authorization = %v, want exactly [Bearer second]", values)
	}
}

func TestWithOrg_IgnoresBlank(t *testing.T) {
	if got := OrgFrom(WithOrg(context.Background(), "  ")); got != "" {
		t.Fatalf("OrgFrom = %q, want empty for a blank org", got)
	}
	if got := OrgFrom(WithOrg(context.Background(), " org-9 ")); got != "org-9" {
		t.Fatalf("OrgFrom = %q, want trimmed org-9", got)
	}
}

// execution-core must NOT have a minter: its ExecuteStep requires the caller's
// own identity to equal the request's user, which a service token can never
// satisfy. Wiring one would only produce confusing failures, and loosening
// execution-core to accept one is a policy decision, not a wiring fix.
func TestDefaultScopes_HasNoExecutionCoreAudience(t *testing.T) {
	if _, ok := DefaultScopes["execution-core"]; ok {
		t.Fatal("execution-core must not have default mint scopes")
	}
}

// session-core refuses any ZDR credential for a non-`:read` scope, so StartRun
// needs a write scope AND a persistent (non-ZDR) principal. Pinning the write
// scope here keeps a well-meaning narrowing to `session:read` from silently
// breaking every durable run's first activity.
func TestDefaultScopes_SessionCoreRequestsWriteScope(t *testing.T) {
	scopes := DefaultScopes[AudienceSessionCore]
	found := false
	for _, s := range scopes {
		if s == "session:write" {
			found = true
		}
		if strings.HasSuffix(s, ":read") {
			t.Fatalf("session-core scope %q is read-only; StartRun is a write", s)
		}
	}
	if !found {
		t.Fatalf("session-core scopes = %v, want session:write", scopes)
	}
}

func TestMinters_GetOnNilMapIsNil(t *testing.T) {
	var m Minters
	if m.Get(AudienceSessionCore) != nil {
		t.Fatal("nil Minters must report no minter")
	}
}

func TestNewMinters_RequiresIdentity(t *testing.T) {
	if _, err := NewMinters(Options{}, nil); err == nil {
		t.Fatal("expected an error when auth-core URL is unset")
	}
	_, err := NewMinters(Options{AuthCoreURL: "http://auth-core:3011"}, nil)
	if err == nil {
		t.Fatal("expected an error when the principal id and key are unset")
	}
}

func TestNewMinters_BuildsEveryAudience(t *testing.T) {
	minters, err := NewMinters(Options{
		AuthCoreURL: "http://auth-core:3011",
		ServiceID:   "orchestrator-core",
		Credential:  "secret",
	}, nil)
	if err != nil {
		t.Fatalf("NewMinters: %v", err)
	}
	for _, audience := range []string{
		AudienceSessionCore, AudienceInferenceCore,
		AudienceCapabilityCore, AudienceLettaBridge,
	} {
		minter := minters.Get(audience)
		if minter == nil {
			t.Fatalf("no minter for %s", audience)
		}
		if minter.Audience() != audience {
			t.Fatalf("minter audience = %q, want %q", minter.Audience(), audience)
		}
	}
}

// An audience mapped to an empty scope list is how an operator turns one
// audience off; it must not fall back to the default scopes.
func TestNewMinters_EmptyScopeOverrideDisablesAudience(t *testing.T) {
	minters, err := NewMinters(Options{
		AuthCoreURL:      "http://auth-core:3011",
		ServiceID:        "orchestrator-core",
		Credential:       "secret",
		ScopesByAudience: map[string][]string{AudienceLettaBridge: {}},
	}, nil)
	if err != nil {
		t.Fatalf("NewMinters: %v", err)
	}
	if minters.Get(AudienceLettaBridge) != nil {
		t.Fatal("letta-bridge should be disabled by an empty scope override")
	}
	if minters.Get(AudienceSessionCore) == nil {
		t.Fatal("disabling one audience must not disable the others")
	}
}

// Minting replaces the credential but must not drop non-credential inbound
// headers: losing trace context would make the activity path unobservable.
func TestUnaryInterceptor_MintingPreservesInboundTraceHeaders(t *testing.T) {
	minter := &fakeMinter{audience: AudienceSessionCore, minted: "service-token"}
	inv := &capturingInvoker{}
	ctx := metadata.NewIncomingContext(
		context.Background(), metadata.Pairs("x-request-id", "req-99"))

	if err := UnaryInterceptor(minter, nil)(
		ctx, "/m", orgRequest{org: "org-1"}, nil, nil, inv.invoke); err != nil {
		t.Fatalf("interceptor returned %v", err)
	}
	if got := inv.calls[0].Get("x-request-id"); len(got) != 1 || got[0] != "req-99" {
		t.Fatalf("x-request-id = %v, want it preserved alongside the minted bearer", got)
	}
	if got := inv.authorizationAt(0); got != "Bearer service-token" {
		t.Fatalf("authorization = %q", got)
	}
}

// The system-owner scope must be REQUESTED at mint time, not merely granted in
// auth-core's registry. The registry is only a ceiling; a mint request narrows to
// the scopes asked for, so omitting it here makes session-core answer
// "service scope required" on every durable run — with the registry looking
// correctly configured.
func TestDefaultScopes_SessionCoreRequestsTheSystemOwnerScope(t *testing.T) {
	scopes := DefaultScopes[AudienceSessionCore]
	found := false
	for _, s := range scopes {
		if s == "session:runs:system-owner" {
			found = true
		}
	}
	if !found {
		t.Fatalf("session-core scopes = %v, want session:runs:system-owner", scopes)
	}
}
