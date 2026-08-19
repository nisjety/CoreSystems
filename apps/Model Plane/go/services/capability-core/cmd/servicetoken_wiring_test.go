package main

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// mintSpy is a stand-in for auth-core's internal-token endpoint that records the
// org each mint was requested for, so tests can prove per-tenant minting rather
// than only that "a token appeared".
type mintSpy struct {
	server *httptest.Server

	mu   sync.Mutex
	orgs []string
}

func newMintSpy(t *testing.T) *mintSpy {
	t.Helper()
	spy := &mintSpy{}
	spy.server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			OrgID string `json:"orgId"`
		}
		_ = json.NewDecoder(r.Body).Decode(&body)

		spy.mu.Lock()
		spy.orgs = append(spy.orgs, body.OrgID)
		serial := len(spy.orgs)
		spy.mu.Unlock()

		audience := r.URL.Path[len("/api/") : len(r.URL.Path)-len("/internal-token")]
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token":            "minted-" + audience + "-" + body.OrgID + "-" + strconvItoa(serial),
			"expiresInSeconds": 300,
			"audience":         audience,
			"issuer":           "https://auth.example.test/api/convex-auth",
			"expiresAt":        time.Now().Add(300 * time.Second).UTC().Format(time.RFC3339),
		})
	}))
	t.Cleanup(spy.server.Close)
	return spy
}

func (m *mintSpy) minted() []string {
	m.mu.Lock()
	defer m.mu.Unlock()
	return append([]string(nil), m.orgs...)
}

func strconvItoa(n int) string {
	if n == 0 {
		return "0"
	}
	var out []byte
	for n > 0 {
		out = append([]byte{byte('0' + n%10)}, out...)
		n /= 10
	}
	return string(out)
}

// recordingInvoker captures the credential presented on each attempt and can be
// told to reject a number of leading attempts with Unauthenticated.
type recordingInvoker struct {
	rejectFirstN int
	bearers      []string
}

func (r *recordingInvoker) invoke(
	ctx context.Context, _ string, _, _ any, _ *grpc.ClientConn, _ ...grpc.CallOption,
) error {
	bearer := ""
	if md, ok := metadata.FromOutgoingContext(ctx); ok {
		if values := md.Get("authorization"); len(values) > 0 {
			// More than one value means the retry appended to an already-credentialed
			// context instead of the original one.
			if len(values) > 1 {
				return status.Errorf(codes.Internal,
					"multiple authorization values presented: %v", values)
			}
			bearer = values[0]
		}
	}
	r.bearers = append(r.bearers, bearer)
	if len(r.bearers) <= r.rejectFirstN {
		return status.Error(codes.Unauthenticated, "verified caller credential required")
	}
	return nil
}

func (r *recordingInvoker) attempts() int { return len(r.bearers) }

// mintingCredential builds a session-core credential in minting mode pointed at
// spy, with the static override explicitly cleared.
func mintingCredential(t *testing.T, spy *mintSpy) *backendCredential {
	t.Helper()
	t.Setenv("SESSION_CORE_SERVICE_TOKEN", "")
	t.Setenv(authCoreURLEnv, spy.server.URL)
	t.Setenv(serviceIDEnv, "capability-core")
	t.Setenv(serviceCredentialEnv, "principal-secret")

	credential := newBackendCredential(
		"session-core", "SESSION_CORE_SERVICE_TOKEN", sessionCoreScopes, sessionCoreTokenReason)
	if credential.minter == nil {
		t.Fatal("expected minting mode, got no minter")
	}
	if credential.static {
		t.Fatal("expected minting mode, got static mode")
	}
	return credential
}

func TestEnvOverrideSkipsMintingEntirely(t *testing.T) {
	spy := newMintSpy(t)
	t.Setenv(authCoreURLEnv, spy.server.URL)
	t.Setenv(serviceIDEnv, "capability-core")
	t.Setenv(serviceCredentialEnv, "principal-secret")
	// Both a static token AND a mintable credential are present: the static one
	// must win, verbatim, so a break-glass paste is never silently overridden.
	t.Setenv("SESSION_CORE_SERVICE_TOKEN", "static-break-glass-token")

	credential := newBackendCredential(
		"session-core", "SESSION_CORE_SERVICE_TOKEN", sessionCoreScopes, sessionCoreTokenReason)
	if !credential.static {
		t.Fatal("a set *_SERVICE_TOKEN must select static mode")
	}
	if credential.minter != nil {
		t.Fatal("static mode must not build a minter")
	}

	invoker := &recordingInvoker{}
	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListConversation",
		&mpv1.ListConversationRequest{OrgId: "org-a", ThreadId: "t-1"}, nil, nil, invoker.invoke)
	if err != nil {
		t.Fatalf("intercept(): %v", err)
	}

	if len(invoker.bearers) != 1 || invoker.bearers[0] != "Bearer static-break-glass-token" {
		t.Fatalf("static token not presented verbatim: %v", invoker.bearers)
	}
	if got := spy.minted(); len(got) != 0 {
		t.Fatalf("static mode still minted: %v", got)
	}
}

func TestMissingCredentialLeavesBackendDisabledWithoutPanic(t *testing.T) {
	// Neither a static token nor a service-principal credential. This must be a
	// warning and a pass-through, never a panic and never a startup failure.
	t.Setenv("SESSION_CORE_SERVICE_TOKEN", "")
	t.Setenv(serviceCredentialEnv, "")
	t.Setenv(authCoreURLEnv, "")
	t.Setenv(serviceIDEnv, "")

	credential := newBackendCredential(
		"session-core", "SESSION_CORE_SERVICE_TOKEN", sessionCoreScopes, sessionCoreTokenReason)
	if credential.minter != nil || credential.static {
		t.Fatalf("expected the no-credential mode, got static=%v minter=%v",
			credential.static, credential.minter != nil)
	}

	invoker := &recordingInvoker{}
	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListConversation",
		&mpv1.ListConversationRequest{OrgId: "org-a"}, nil, nil, invoker.invoke)
	if err != nil {
		t.Fatalf("intercept() must not fail locally: %v", err)
	}
	if len(invoker.bearers) != 1 || invoker.bearers[0] != "" {
		t.Fatalf("expected one unauthenticated pass-through, got %v", invoker.bearers)
	}
}

func TestStartLearningConsumerStaysDisabledWithoutBackends(t *testing.T) {
	// The consumer's disabled path must remain reachable and non-panicking: this
	// is what "the learning review stays off rather than crashing" relies on.
	t.Setenv("SESSION_CORE_ADDR", "")
	t.Setenv("INFERENCE_CORE_ADDR", "")

	session, inference, runs := dialBackends()
	if session != nil || inference != nil || runs != nil {
		t.Fatal("unset backend addrs must yield no clients")
	}
	// nil clients → consumer disabled, no goroutine, no panic.
	startLearningConsumer(context.Background(), nil, session, inference)
}

func TestMintedTokenCarriesTheRequestOwnOrg(t *testing.T) {
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	invoker := &recordingInvoker{}
	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/UpsertAgentSkill",
		&mpv1.UpsertAgentSkillRequest{OrgId: "org-b", Name: "s"}, nil, nil, invoker.invoke)
	if err != nil {
		t.Fatalf("intercept(): %v", err)
	}

	minted := spy.minted()
	if len(minted) != 1 || minted[0] != "org-b" {
		t.Fatalf("token was not minted for the request's own org: %v", minted)
	}
	if len(invoker.bearers) != 1 || invoker.bearers[0] != "Bearer minted-session-core-org-b-1" {
		t.Fatalf("minted token not presented: %v", invoker.bearers)
	}
}

func TestMintedTokensAreLazyAndPerOrg(t *testing.T) {
	// Two runs from two different tenants: each gets its own token, and a repeat
	// for the same tenant reuses the cached one. This is the property a single
	// startup-minted token cannot have.
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	for _, org := range []string{"org-a", "org-b", "org-a"} {
		invoker := &recordingInvoker{}
		err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListAgentSkills",
			&mpv1.ListAgentSkillsRequest{OrgId: org}, nil, nil, invoker.invoke)
		if err != nil {
			t.Fatalf("intercept(%s): %v", org, err)
		}
	}

	minted := spy.minted()
	if len(minted) != 2 || minted[0] != "org-a" || minted[1] != "org-b" {
		t.Fatalf("expected exactly one lazy mint per org, got %v", minted)
	}
}

func TestOrglessRpcFallsBackToTheVerifiedCallerOrg(t *testing.T) {
	// /commands delegation: ListModels and CompactNow have no org_id on the wire,
	// so the caller's verified principal supplies the tenant.
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	ctx := contextWithVerifiedPrincipal(t, "org-from-principal")
	invoker := &recordingInvoker{}
	err := credential.intercept(ctx, "/mp.v1.SessionCore/CompactNow",
		&mpv1.CompactNowRequest{Toon: true}, nil, nil, invoker.invoke)
	if err != nil {
		t.Fatalf("intercept(): %v", err)
	}

	minted := spy.minted()
	if len(minted) != 1 || minted[0] != "org-from-principal" {
		t.Fatalf("org-less RPC did not use the verified caller org: %v", minted)
	}
}

func TestOrglessRpcWithoutPrincipalIsRefusedNotGuessed(t *testing.T) {
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	invoker := &recordingInvoker{}
	err := credential.intercept(context.Background(), "/mp.v1.InferenceCore/ListModels",
		&mpv1.ListModelsRequest{}, nil, nil, invoker.invoke)

	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("expected FailedPrecondition for an untenanted RPC, got %v", err)
	}
	if invoker.attempts() != 0 {
		t.Fatalf("an untenanted RPC was still sent: %d attempts", invoker.attempts())
	}
	if got := spy.minted(); len(got) != 0 {
		t.Fatalf("an untenanted RPC still minted: %v", got)
	}
}

func TestUnauthenticatedTriggersExactlyOneRemintAndRetry(t *testing.T) {
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	// Prime the cache so the first attempt uses a token the provider believes is
	// live — the situation the 401 backstop exists for.
	warmup := &recordingInvoker{}
	if err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListAgentSkills",
		&mpv1.ListAgentSkillsRequest{OrgId: "org-a"}, nil, nil, warmup.invoke); err != nil {
		t.Fatalf("warmup intercept(): %v", err)
	}
	if len(spy.minted()) != 1 {
		t.Fatalf("warmup should mint once, got %v", spy.minted())
	}

	invoker := &recordingInvoker{rejectFirstN: 1}
	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListAgentSkills",
		&mpv1.ListAgentSkillsRequest{OrgId: "org-a"}, nil, nil, invoker.invoke)
	if err != nil {
		t.Fatalf("the retry should have succeeded: %v", err)
	}

	if invoker.attempts() != 2 {
		t.Fatalf("expected exactly one retry (2 attempts), got %d", invoker.attempts())
	}
	if minted := spy.minted(); len(minted) != 2 || minted[1] != "org-a" {
		t.Fatalf("expected exactly one forced re-mint for org-a, got %v", minted)
	}
	// The retry presented a *different* credential — a retry with the same
	// rejected token would be pointless.
	if invoker.bearers[0] == invoker.bearers[1] {
		t.Fatalf("retry reused the rejected credential: %v", invoker.bearers)
	}
}

func TestUnauthenticatedIsRetriedOnlyOnce(t *testing.T) {
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	// A backend that rejects everything must not produce an unbounded
	// mint/retry loop.
	invoker := &recordingInvoker{rejectFirstN: 100}
	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListAgentSkills",
		&mpv1.ListAgentSkillsRequest{OrgId: "org-a"}, nil, nil, invoker.invoke)

	if status.Code(err) != codes.Unauthenticated {
		t.Fatalf("a persistently rejected call must surface Unauthenticated, got %v", err)
	}
	if invoker.attempts() != 2 {
		t.Fatalf("expected exactly 2 attempts (original + one retry), got %d", invoker.attempts())
	}
	if minted := spy.minted(); len(minted) != 2 {
		t.Fatalf("expected exactly 2 mints (initial + one re-mint), got %d: %v", len(minted), minted)
	}
}

func TestNonAuthErrorsAreNotRetried(t *testing.T) {
	spy := newMintSpy(t)
	credential := mintingCredential(t, spy)

	failing := func(context.Context, string, any, any, *grpc.ClientConn, ...grpc.CallOption) error {
		return status.Error(codes.NotFound, "thread missing")
	}
	calls := 0
	counted := func(
		ctx context.Context, method string, req, reply any, cc *grpc.ClientConn, opts ...grpc.CallOption,
	) error {
		calls++
		return failing(ctx, method, req, reply, cc, opts...)
	}

	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListConversation",
		&mpv1.ListConversationRequest{OrgId: "org-a"}, nil, nil, counted)
	if status.Code(err) != codes.NotFound {
		t.Fatalf("expected the backend error to pass through, got %v", err)
	}
	if calls != 1 {
		t.Fatalf("a non-auth error must not be retried, got %d calls", calls)
	}
	if len(spy.minted()) != 1 {
		t.Fatalf("a non-auth error must not force a re-mint: %v", spy.minted())
	}
}

func TestMintFailureSurfacesUnavailableAndDoesNotSendTheRpc(t *testing.T) {
	refusing := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"message":"Service principal is not authorized"}`))
	}))
	t.Cleanup(refusing.Close)

	t.Setenv("SESSION_CORE_SERVICE_TOKEN", "")
	t.Setenv(authCoreURLEnv, refusing.URL)
	t.Setenv(serviceIDEnv, "capability-core")
	t.Setenv(serviceCredentialEnv, "wrong-secret")

	credential := newBackendCredential(
		"session-core", "SESSION_CORE_SERVICE_TOKEN", sessionCoreScopes, sessionCoreTokenReason)

	invoker := &recordingInvoker{}
	err := credential.intercept(context.Background(), "/mp.v1.SessionCore/ListAgentSkills",
		&mpv1.ListAgentSkillsRequest{OrgId: "org-a"}, nil, nil, invoker.invoke)

	if status.Code(err) != codes.Unavailable {
		t.Fatalf("expected Unavailable when auth-core refuses, got %v", err)
	}
	if invoker.attempts() != 0 {
		t.Fatalf("an uncredentialed RPC was still sent: %d attempts", invoker.attempts())
	}
}

func TestRequestOrgReadsEveryTenantedLearningRpc(t *testing.T) {
	// The four learning-review RPCs carry org_id, which is exactly what makes
	// lazy per-run minting possible. The two /commands RPCs do not.
	tenanted := []any{
		&mpv1.ListConversationRequest{OrgId: "org-1"},
		&mpv1.ListAgentSkillsRequest{OrgId: "org-1"},
		&mpv1.UpsertAgentSkillRequest{OrgId: "org-1"},
		&mpv1.InferRequest{OrgId: "org-1"},
	}
	for _, req := range tenanted {
		if got := requestOrg(req); got != "org-1" {
			t.Fatalf("%T: requestOrg() = %q, want org-1", req, got)
		}
	}

	untenanted := []any{
		&mpv1.ListModelsRequest{},
		&mpv1.CompactNowRequest{Toon: true},
	}
	for _, req := range untenanted {
		if got := requestOrg(req); got != "" {
			t.Fatalf("%T: requestOrg() = %q, want empty", req, got)
		}
	}

	if got := requestOrg(&mpv1.ListAgentSkillsRequest{OrgId: "  "}); got != "" {
		t.Fatalf("a blank org_id must not be treated as a tenant, got %q", got)
	}
	if got := requestOrg("not a protobuf"); got != "" {
		t.Fatalf("a non-message request must yield no org, got %q", got)
	}
}

func TestEnvOrDefault(t *testing.T) {
	t.Setenv("CAPABILITY_CORE_TEST_VAR", "  ")
	if got := envOrDefault("CAPABILITY_CORE_TEST_VAR", "fallback"); got != "fallback" {
		t.Fatalf("blank value should fall back, got %q", got)
	}
	t.Setenv("CAPABILITY_CORE_TEST_VAR", " value ")
	if got := envOrDefault("CAPABILITY_CORE_TEST_VAR", "fallback"); got != "value" {
		t.Fatalf("value should be trimmed, got %q", got)
	}
}

// contextWithVerifiedPrincipal returns a context carrying a genuinely verified
// authctx principal for orgID.
//
// It runs a real authctx.Verifier over a real RS256 token rather than stuffing a
// value into the context, because the fallback path deliberately trusts ONLY a
// cryptographically verified principal — a fake that bypassed verification would
// not be testing the same thing.
func contextWithVerifiedPrincipal(t *testing.T, orgID string) context.Context {
	t.Helper()

	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	publicDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	publicPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER})

	const issuer = "https://auth.example.test/api/convex-auth"
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{"capability-core"},
		Issuer:       issuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatalf("NewVerifier(): %v", err)
	}

	now := time.Now()
	signed, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss":            issuer,
		"aud":            "capability-core",
		"sub":            "user-1",
		"iat":            now.Unix(),
		"nbf":            now.Add(-5 * time.Second).Unix(),
		"exp":            now.Add(5 * time.Minute).Unix(),
		"org_id":         orgID,
		"user_id":        "user-1",
		"principal_type": "user",
		"zdr":            false,
	}).SignedString(key)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}

	incoming := metadata.NewIncomingContext(context.Background(),
		metadata.Pairs("authorization", "Bearer "+signed))

	var captured context.Context
	interceptor := verifier.UnaryServerInterceptor(nil)
	if _, err := interceptor(
		incoming,
		&mpv1.ListModelsRequest{},
		&grpc.UnaryServerInfo{FullMethod: "/mp.v1.CapabilityCore/Test"},
		func(ctx context.Context, _ any) (any, error) { captured = ctx; return nil, nil },
	); err != nil {
		t.Fatalf("server interceptor rejected the test token: %v", err)
	}

	principal, ok := authctx.PrincipalFromContext(captured)
	if !ok || principal.OrganizationID != orgID {
		t.Fatalf("test context has no verified principal for %q: %+v", orgID, principal)
	}
	return captured
}
