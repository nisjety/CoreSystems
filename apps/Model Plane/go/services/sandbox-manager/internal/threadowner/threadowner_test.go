package threadowner

import (
	"context"
	"errors"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// countingResolver answers from a fixed table and counts its calls.
type countingResolver struct {
	mu    sync.Mutex
	owns  bool
	err   error
	calls int
}

func (r *countingResolver) OwnsThread(context.Context, string, string, string) (bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.calls++
	return r.owns, r.err
}

func (r *countingResolver) set(owns bool, err error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.owns, r.err = owns, err
}

func (r *countingResolver) count() int {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.calls
}

type fakeClock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now
}

func (c *fakeClock) advance(d time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.now = c.now.Add(d)
}

func newClock() *fakeClock { return &fakeClock{now: time.Date(2026, 11, 20, 12, 0, 0, 0, time.UTC)} }

// TestCacheServesAYesForThirtySecondsThenAsksAgain: the positive cache holds for
// exactly its lifetime and a later call reaches session-core again.
func TestCacheServesAYesForThirtySecondsThenAsksAgain(t *testing.T) {
	t.Parallel()
	next := &countingResolver{owns: true}
	clock := newClock()
	cache, err := NewCache(next, DefaultCacheTTL)
	if err != nil {
		t.Fatal(err)
	}
	cache.WithClock(clock.Now)
	ctx := context.Background()

	for i := 0; i < 3; i++ {
		owns, err := cache.OwnsThread(ctx, "org-1", "user-1", "thread-1")
		if err != nil || !owns {
			t.Fatalf("call %d = (%v, %v), want a yes", i, owns, err)
		}
	}
	if next.count() != 1 {
		t.Fatalf("session-core was asked %d times inside the window, want 1", next.count())
	}

	clock.advance(DefaultCacheTTL - time.Second)
	if _, err := cache.OwnsThread(ctx, "org-1", "user-1", "thread-1"); err != nil {
		t.Fatal(err)
	}
	if next.count() != 1 {
		t.Fatalf("asked %d times one second before expiry, want still 1", next.count())
	}

	clock.advance(time.Second)
	if _, err := cache.OwnsThread(ctx, "org-1", "user-1", "thread-1"); err != nil {
		t.Fatal(err)
	}
	if next.count() != 2 {
		t.Fatalf("asked %d times after expiry, want 2", next.count())
	}
}

// TestCacheKeysOnOrgUserAndThread: a yes for one user, org or thread is never
// served to another.
func TestCacheKeysOnOrgUserAndThread(t *testing.T) {
	t.Parallel()
	next := &countingResolver{owns: true}
	cache, _ := NewCache(next, DefaultCacheTTL)
	ctx := context.Background()
	for _, k := range [][3]string{
		{"org-1", "user-1", "thread-1"},
		{"org-1", "user-2", "thread-1"},
		{"org-2", "user-1", "thread-1"},
		{"org-1", "user-1", "thread-2"},
	} {
		if _, err := cache.OwnsThread(ctx, k[0], k[1], k[2]); err != nil {
			t.Fatal(err)
		}
	}
	if next.count() != 4 {
		t.Fatalf("asked %d times for four distinct triples, want 4", next.count())
	}
}

// TestCacheNeverRemembersANoOrAnError: a refusal is asked again on the next call,
// and a yes that turns into a no is not served stale after its entry is dropped.
func TestCacheNeverRemembersANoOrAnError(t *testing.T) {
	t.Parallel()
	next := &countingResolver{owns: false}
	cache, _ := NewCache(next, DefaultCacheTTL)
	ctx := context.Background()

	if owns, err := cache.OwnsThread(ctx, "o", "u", "t"); owns || err != nil {
		t.Fatalf("no = (%v, %v)", owns, err)
	}
	next.set(false, errors.New("session-core down"))
	if owns, err := cache.OwnsThread(ctx, "o", "u", "t"); owns || err == nil {
		t.Fatalf("error = (%v, %v), want a refusal with an error", owns, err)
	}
	next.set(true, nil)
	if owns, err := cache.OwnsThread(ctx, "o", "u", "t"); !owns || err != nil {
		t.Fatalf("after recovery = (%v, %v), want a yes", owns, err)
	}
	if next.count() != 3 {
		t.Fatalf("asked %d times, want 3 (nothing cached before the yes)", next.count())
	}
}

// TestCacheIsBoundedByEntryCount: filling the cache past its bound resets it and
// still answers.
func TestCacheIsBoundedByEntryCount(t *testing.T) {
	t.Parallel()
	next := &countingResolver{owns: true}
	cache, _ := NewCache(next, DefaultCacheTTL)
	ctx := context.Background()
	for i := 0; i < maxCacheEntries+5; i++ {
		if _, err := cache.OwnsThread(ctx, "o", "u", "t"+strconv.Itoa(i)); err != nil {
			t.Fatal(err)
		}
	}
	cache.mu.Lock()
	size := len(cache.entries)
	cache.mu.Unlock()
	if size > maxCacheEntries {
		t.Fatalf("cache holds %d entries, bound is %d", size, maxCacheEntries)
	}
}

// fakeSessionCore records the request and bearer it saw.
type fakeSessionCore struct {
	authorized bool
	err        error
	gotReq     *mpv1.ResolveThreadOwnerRequest
	gotAuth    []string
}

func (f *fakeSessionCore) ResolveThreadOwner(ctx context.Context, in *mpv1.ResolveThreadOwnerRequest, _ ...grpc.CallOption) (*mpv1.ResolveThreadOwnerResponse, error) {
	f.gotReq = in
	md, _ := metadata.FromOutgoingContext(ctx)
	f.gotAuth = md.Get("authorization")
	if f.err != nil {
		return nil, f.err
	}
	return &mpv1.ResolveThreadOwnerResponse{Authorized: f.authorized}, nil
}

type fakeTokens struct {
	err  error
	orgs []string
}

func (f *fakeTokens) Token(_ context.Context, orgID string) (string, error) {
	f.orgs = append(f.orgs, orgID)
	if f.err != nil {
		return "", f.err
	}
	return "fixture-token-" + orgID, nil
}

// TestSessionCoreAsksResolveThreadOwnerWithTheOrgBoundServiceToken: the request
// carries the verified org, user and thread, and the bearer is the token minted
// for that org.
func TestSessionCoreAsksResolveThreadOwnerWithTheOrgBoundServiceToken(t *testing.T) {
	t.Parallel()
	client := &fakeSessionCore{authorized: true}
	tokens := &fakeTokens{}
	resolver, err := NewSessionCore(client, tokens)
	if err != nil {
		t.Fatal(err)
	}
	owns, err := resolver.OwnsThread(context.Background(), "org-1", "user-1", "thread-1")
	if err != nil || !owns {
		t.Fatalf("OwnsThread = (%v, %v), want a yes", owns, err)
	}
	if got := client.gotReq; got.GetOrgId() != "org-1" || got.GetUserId() != "user-1" || got.GetThreadId() != "thread-1" {
		t.Fatalf("request = %v", got)
	}
	if len(client.gotAuth) != 1 || client.gotAuth[0] != "Bearer fixture-token-org-1" {
		t.Fatalf("authorization = %v, want the org-1 bearer", client.gotAuth)
	}
	if len(tokens.orgs) != 1 || tokens.orgs[0] != "org-1" {
		t.Fatalf("token orgs = %v", tokens.orgs)
	}
}

// TestSessionCoreForeignUserIsADefiniteNo: session-core says authorized false.
func TestSessionCoreForeignUserIsADefiniteNo(t *testing.T) {
	t.Parallel()
	resolver, _ := NewSessionCore(&fakeSessionCore{authorized: false}, &fakeTokens{})
	owns, err := resolver.OwnsThread(context.Background(), "org-1", "user-2", "thread-1")
	if err != nil || owns {
		t.Fatalf("OwnsThread = (%v, %v), want a definite no", owns, err)
	}
}

// TestSessionCoreErrorsAreNeverAYes: an unreachable session-core, a refused token
// and a malformed request are all errors, and the error carries no token or
// session-core message.
func TestSessionCoreErrorsAreNeverAYes(t *testing.T) {
	t.Parallel()
	cases := map[string]struct {
		client *fakeSessionCore
		tokens *fakeTokens
		ids    [3]string
	}{
		"unreachable":   {&fakeSessionCore{err: status.Error(codes.Unavailable, "dial tcp session-core: refused secret-detail")}, &fakeTokens{}, [3]string{"o", "u", "t"}},
		"permission":    {&fakeSessionCore{err: status.Error(codes.PermissionDenied, "missing scope secret-detail")}, &fakeTokens{}, [3]string{"o", "u", "t"}},
		"token refused": {&fakeSessionCore{authorized: true}, &fakeTokens{err: errors.New("auth-core said 403")}, [3]string{"o", "u", "t"}},
		"empty thread":  {&fakeSessionCore{authorized: true}, &fakeTokens{}, [3]string{"o", "u", " "}},
		"empty user":    {&fakeSessionCore{authorized: true}, &fakeTokens{}, [3]string{"o", "", "t"}},
	}
	for name, tc := range cases {
		tc := tc
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			resolver, _ := NewSessionCore(tc.client, tc.tokens)
			owns, err := resolver.OwnsThread(context.Background(), tc.ids[0], tc.ids[1], tc.ids[2])
			if owns || err == nil {
				t.Fatalf("OwnsThread = (%v, %v), want an error and no yes", owns, err)
			}
			if strings.Contains(err.Error(), "secret-detail") {
				t.Fatalf("error carries the session-core message: %v", err)
			}
		})
	}
}

func TestNewSessionCoreRequiresItsParts(t *testing.T) {
	t.Parallel()
	if _, err := NewSessionCore(nil, &fakeTokens{}); err == nil {
		t.Fatal("a nil client was accepted")
	}
	if _, err := NewSessionCore(&fakeSessionCore{}, nil); err == nil {
		t.Fatal("a nil token source was accepted")
	}
	if _, err := NewCache(nil, time.Second); err == nil {
		t.Fatal("a nil resolver was accepted")
	}
}

// fakeTransport answers ThreadsExist from a table and records the call.
type fakeTransport struct {
	answers map[string]bool
	err     error
	bearer  string
	org     string
	ids     []string
}

func (f *fakeTransport) ThreadsExist(_ context.Context, bearer, orgID string, ids []string) (map[string]bool, error) {
	f.bearer, f.org, f.ids = bearer, orgID, ids
	return f.answers, f.err
}

func TestSessionCoreExistencePassesTheOrgBoundTokenAndReturnsExplicitAnswers(t *testing.T) {
	t.Parallel()
	transport := &fakeTransport{answers: map[string]bool{"t1": true, "t2": false}}
	reader, err := NewSessionCoreExistence(transport, &fakeTokens{})
	if err != nil {
		t.Fatal(err)
	}
	got, err := reader.ThreadsExist(context.Background(), "org-1", []string{"t1", "t2", "t3"})
	if err != nil {
		t.Fatal(err)
	}
	if !got["t1"] || got["t2"] {
		t.Fatalf("answers = %v", got)
	}
	if _, answered := got["t3"]; answered {
		t.Fatalf("t3 was not answered and must stay unknown, got %v", got)
	}
	if transport.bearer != "fixture-token-org-1" || transport.org != "org-1" || len(transport.ids) != 3 {
		t.Fatalf("transport saw bearer=%q org=%q ids=%v", transport.bearer, transport.org, transport.ids)
	}
}

func TestSessionCoreExistenceFailsClosedOnEveryError(t *testing.T) {
	t.Parallel()
	reader, _ := NewSessionCoreExistence(&fakeTransport{err: status.Error(codes.Unavailable, "secret-detail")}, &fakeTokens{})
	if got, err := reader.ThreadsExist(context.Background(), "org-1", []string{"t1"}); err == nil || got != nil || strings.Contains(err.Error(), "secret-detail") {
		t.Fatalf("transport error = (%v, %v)", got, err)
	}
	reader, _ = NewSessionCoreExistence(&fakeTransport{}, &fakeTokens{err: errors.New("no token")})
	if _, err := reader.ThreadsExist(context.Background(), "org-1", []string{"t1"}); err == nil {
		t.Fatal("a token failure was not an error")
	}
	reader, _ = NewSessionCoreExistence(&fakeTransport{}, &fakeTokens{})
	if _, err := reader.ThreadsExist(context.Background(), "", []string{"t1"}); err == nil {
		t.Fatal("an empty org was accepted")
	}
}
