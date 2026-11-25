package clients

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
)

// fixtureServiceAPIKey is a low-entropy fake of org-core's service API key.
const fixtureServiceAPIKey = "fixture-org-core-service-api-key-0000000000000000"

// fakeTokens mints one recognisable fake token per organisation and records each mint, in order. blank makes
// it return an empty token, and err makes it fail.
type fakeTokens struct {
	err    error
	blank  bool
	minted []string
}

func (f *fakeTokens) Token(_ context.Context, orgID string) (string, error) {
	f.minted = append(f.minted, orgID)
	if f.err != nil {
		return "", f.err
	}
	if f.blank {
		return "", nil
	}
	return "fixture-token-for-" + orgID, nil
}

// receiptServer stands in for session-core's SessionRetentionService. It answers GetThreadRetention from one
// canned response, or refuses, and records what each call carried.
type receiptServer struct {
	mpv1.UnimplementedSessionRetentionServiceServer
	response       *mpv1.GetThreadRetentionResponse
	refuse         error
	calls          int
	orgs           []string
	threads        []string
	auths          []string
	deadlineWithin bool
}

func (s *receiptServer) GetThreadRetention(ctx context.Context, in *mpv1.GetThreadRetentionRequest) (*mpv1.GetThreadRetentionResponse, error) {
	s.calls++
	s.orgs = append(s.orgs, in.GetOrgId())
	s.threads = append(s.threads, in.GetThreadId())
	auth := ""
	if md, ok := metadata.FromIncomingContext(ctx); ok {
		if values := md.Get("authorization"); len(values) == 1 {
			auth = values[0]
		}
	}
	s.auths = append(s.auths, auth)
	if deadline, ok := ctx.Deadline(); ok {
		s.deadlineWithin = time.Until(deadline) <= threadReceiptTimeout
	}
	if s.refuse != nil {
		return nil, s.refuse
	}
	return s.response, nil
}

// startReceiptServer serves the receipt server over an in-memory listener and returns a client connection
// to it, so the generated binding and its metadata run over real gRPC without a network socket.
func startReceiptServer(t *testing.T, server *receiptServer) *grpc.ClientConn {
	t.Helper()
	listener := bufconn.Listen(1 << 20)
	grpcServer := grpc.NewServer()
	mpv1.RegisterSessionRetentionServiceServer(grpcServer, server)
	go func() { _ = grpcServer.Serve(listener) }()
	t.Cleanup(grpcServer.Stop)

	conn, err := grpc.NewClient("passthrough:///session-core-bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return listener.DialContext(ctx)
		}),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		t.Fatalf("dial the in-memory session-core: %v", err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	return conn
}

// bindingClient is the production binding wired to the in-memory session-core and the given token source.
func bindingClient(t *testing.T, server *receiptServer, tokens TokenSource) *SessionThreadRetentionClient {
	t.Helper()
	conn := startReceiptServer(t, server)
	source := grpcThreadReceiptSource{client: mpv1.NewSessionRetentionServiceClient(conn), tokens: tokens}
	return newSessionThreadRetentionClient(source, conn.Close)
}

// TestGRPCBindingSendsTheMintedTokenAndTheIdsOnly pins the wire: the token minted for the organisation read
// travels as the authorization metadata with the Bearer scheme, the two trimmed ids are the only request
// fields, and the read carries the receipt timeout.
func TestGRPCBindingSendsTheMintedTokenAndTheIdsOnly(t *testing.T) {
	server := &receiptServer{response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: true}}
	tokens := &fakeTokens{}
	client := bindingClient(t, server, tokens)

	nothing, err := client.NothingRetained(context.Background(), " org-1 ", " thr-1 ")
	if err != nil || !nothing {
		t.Fatalf("a zero-retention receipt over gRPC: nothing=%v err=%v", nothing, err)
	}
	if server.calls != 1 || server.orgs[0] != "org-1" || server.threads[0] != "thr-1" {
		t.Fatalf("the request carries the trimmed ids once: calls=%d", server.calls)
	}
	if !slices.Equal(tokens.minted, []string{"org-1"}) {
		t.Fatalf("the read takes one token, for its own organisation: minted=%v", tokens.minted)
	}
	if server.auths[0] != "Bearer fixture-token-for-org-1" {
		t.Fatalf("the minted token travels as the authorization metadata with the Bearer scheme, got %q", server.auths[0])
	}
	if !server.deadlineWithin {
		t.Fatal("a receipt read carries the receipt timeout, so a stalled session-core cannot hold a hold open")
	}
}

// TestGRPCBindingMintsPerOrganisationAndNeverReusesATokenAcrossOrgs pins that every read asks for the token of
// the organisation it reads, and that the token which travels is that organisation's, so no token crosses
// organisations.
func TestGRPCBindingMintsPerOrganisationAndNeverReusesATokenAcrossOrgs(t *testing.T) {
	server := &receiptServer{response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: true}}
	tokens := &fakeTokens{}
	client := bindingClient(t, server, tokens)

	for _, org := range []string{"org-x", "org-y", "org-x"} {
		if _, err := client.NothingRetained(context.Background(), org, "thr-1"); err != nil {
			t.Fatalf("a read for %s: %v", org, err)
		}
	}
	if !slices.Equal(tokens.minted, []string{"org-x", "org-y", "org-x"}) {
		t.Fatalf("each read mints for its own organisation: minted=%v", tokens.minted)
	}
	if !slices.Equal(server.orgs, []string{"org-x", "org-y", "org-x"}) {
		t.Fatalf("each call names its own organisation: got %v", server.orgs)
	}
	wantAuths := []string{"Bearer fixture-token-for-org-x", "Bearer fixture-token-for-org-y", "Bearer fixture-token-for-org-x"}
	if !slices.Equal(server.auths, wantAuths) {
		t.Fatalf("each read sends its own organisation's token: got %v", server.auths)
	}
}

// TestGRPCBindingNeverCallsSessionCoreWithoutAToken pins that a token which cannot be minted, or comes back
// empty, stops the read before session-core is called, and the answer is an error, never nothing-retained.
func TestGRPCBindingNeverCallsSessionCoreWithoutAToken(t *testing.T) {
	server := &receiptServer{response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: true}}
	tokens := &fakeTokens{err: errors.New("auth-core refused the mint")}
	client := bindingClient(t, server, tokens)

	got, err := client.NothingRetained(context.Background(), "org-1", "thr-1")
	if err == nil || got {
		t.Fatalf("a token failure is an error, never a nothing-retained answer: got=%v err=%v", got, err)
	}
	tokens.err, tokens.blank = nil, true
	got, err = client.NothingRetained(context.Background(), "org-1", "thr-1")
	if err == nil || got {
		t.Fatalf("an empty token is an error, never a nothing-retained answer: got=%v err=%v", got, err)
	}
	if server.calls != 0 {
		t.Fatalf("no call reaches session-core without a token, got %d", server.calls)
	}
}

// TestGRPCBindingMapsEveryReceipt pins the answer over gRPC: a thread session-core does not hold is not
// proven empty, and only a found receipt that says nothing was retained can refuse a hold.
func TestGRPCBindingMapsEveryReceipt(t *testing.T) {
	cases := []struct {
		name     string
		response *mpv1.GetThreadRetentionResponse
		want     bool
	}{
		{name: "not found", response: &mpv1.GetThreadRetentionResponse{Found: false, NothingRetained: true}, want: false},
		{name: "found and nothing retained", response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: true}, want: true},
		{name: "found and retained", response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: false}, want: false},
	}
	server := &receiptServer{}
	client := bindingClient(t, server, &fakeTokens{})
	for _, tc := range cases {
		server.response = tc.response
		got, err := client.NothingRetained(context.Background(), "org-1", "thr-1")
		if err != nil || got != tc.want {
			t.Fatalf("%s: got %v err=%v want %v", tc.name, got, err, tc.want)
		}
	}
}

// TestGRPCBindingReturnsARefusalAndNeverGuesses pins the refusal: session-core's permission error comes back as
// an error, so the hold is placed, and a call without ids never reaches session-core.
func TestGRPCBindingReturnsARefusalAndNeverGuesses(t *testing.T) {
	server := &receiptServer{
		response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: true},
		refuse:   status.Error(codes.PermissionDenied, "only org-core may read thread retention"),
	}
	client := bindingClient(t, server, &fakeTokens{})

	got, err := client.NothingRetained(context.Background(), "org-1", "thr-1")
	if err == nil || got || status.Code(err) != codes.PermissionDenied {
		t.Fatalf("a refused read is an error, never a nothing-retained answer: got=%v err=%v", got, err)
	}
	if _, err := client.NothingRetained(context.Background(), "org-1", "  "); err == nil {
		t.Fatal("a call without a thread is refused before it reaches session-core")
	}
	if server.calls != 1 {
		t.Fatalf("only the one call with ids reaches session-core, got %d", server.calls)
	}
}

// TestConstructorDialsTheSessionCoreListenerInPlaintext runs the production constructor against a plaintext
// gRPC server on a loopback socket, as session-core serves on :9091 with no TLS. The read reaches the server
// and carries the token minted for its organisation.
func TestConstructorDialsTheSessionCoreListenerInPlaintext(t *testing.T) {
	server := &receiptServer{response: &mpv1.GetThreadRetentionResponse{Found: true, NothingRetained: true}}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("listen on loopback: %v", err)
	}
	grpcServer := grpc.NewServer()
	mpv1.RegisterSessionRetentionServiceServer(grpcServer, server)
	go func() { _ = grpcServer.Serve(listener) }()
	t.Cleanup(grpcServer.Stop)

	client, err := NewSessionThreadRetentionClient(listener.Addr().String(), &fakeTokens{})
	if err != nil || client == nil {
		t.Fatalf("the production constructor builds over a private address: client=%v err=%v", client, err)
	}
	t.Cleanup(func() { _ = client.Close() })

	nothing, err := client.NothingRetained(context.Background(), "org-1", "thr-1")
	if err != nil || !nothing {
		t.Fatalf("a plaintext read reaches a plaintext session-core listener: nothing=%v err=%v", nothing, err)
	}
	if server.calls != 1 || server.auths[0] != "Bearer fixture-token-for-org-1" {
		t.Fatalf("the read carries the token minted for its organisation: calls=%d", server.calls)
	}
}

// mintBody is the request body the token exchange posts. The mint test reads it back.
type mintBody struct {
	OrgID  string   `json:"orgId"`
	Scopes []string `json:"scopes"`
	Reason string   `json:"reason"`
}

// TestSessionCoreTokenSourceMintsForTheOrganisationOverAuthCore pins the mint contract: the exchange posts to
// Auth Core's session-core route with org-core's principal and service API key, asks for the organisation read
// and the one retention scope, and returns the token Auth Core issues.
func TestSessionCoreTokenSourceMintsForTheOrganisationOverAuthCore(t *testing.T) {
	var gotPath, gotServiceID, gotAPIKey string
	var gotBody mintBody
	authCore := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotServiceID = r.Header.Get("x-service-id")
		gotAPIKey = r.Header.Get("x-service-api-key")
		_ = json.NewDecoder(r.Body).Decode(&gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"token":"fixture-minted-token-for-org-1","expiresAt":"2030-01-01T00:00:00Z","expiresInSeconds":300,"issuer":"auth-core","audience":"session-core"}`))
	}))
	t.Cleanup(authCore.Close)

	tokens, err := NewSessionCoreTokenSource(authCore.URL, OrgCoreServiceID, fixtureServiceAPIKey)
	if err != nil {
		t.Fatalf("a complete configuration builds the token source: %v", err)
	}
	token, err := tokens.Token(context.Background(), "org-1")
	if err != nil || token != "fixture-minted-token-for-org-1" {
		t.Fatalf("the mint returns the token Auth Core issues: token=%q err=%v", token, err)
	}
	if gotPath != "/api/session-core/internal-token" {
		t.Fatalf("the mint goes to the session-core route, got %q", gotPath)
	}
	if gotServiceID != "org-core" || gotAPIKey != fixtureServiceAPIKey {
		t.Fatalf("the mint presents org-core's principal and service API key (principal %q)", gotServiceID)
	}
	if gotBody.OrgID != "org-1" || !slices.Equal(gotBody.Scopes, []string{"session:retention:read"}) || gotBody.Reason != "legal-hold thread receipt check" {
		t.Fatalf("the mint asks for the organisation and the one retention scope: %+v", gotBody)
	}
}

// TestSessionCoreTokenSourceRefusesAnUnsetField pins that no token source is built without its Auth Core URL,
// its service id or its service API key, and that no refusal carries the key.
func TestSessionCoreTokenSourceRefusesAnUnsetField(t *testing.T) {
	cases := map[string][3]string{
		"no Auth Core URL":   {"", OrgCoreServiceID, fixtureServiceAPIKey},
		"no service id":      {"http://auth-core:3011", "", fixtureServiceAPIKey},
		"no service API key": {"http://auth-core:3011", OrgCoreServiceID, ""},
	}
	for name, args := range cases {
		tokens, err := NewSessionCoreTokenSource(args[0], args[1], args[2])
		if tokens != nil || err == nil {
			t.Fatalf("%s: no token source is built without it: tokens=%v err=%v", name, tokens, err)
		}
		if strings.Contains(err.Error(), fixtureServiceAPIKey) {
			t.Fatalf("%s: the refusal must not carry the service API key", name)
		}
	}
}
