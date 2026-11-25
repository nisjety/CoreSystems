// Package threadowner answers one question for sandbox-manager: does this user
// own this thread? (O-13, D-f06-A.)
//
// The thread workspace is keyed by (org, owner, thread), and its blobs by (org,
// thread) alone. Until now sandbox-manager trusted the gateway to have checked
// that the caller owns the thread. With SANDBOX_MANAGER_THREAD_OWNER_CHECK on it
// asks session-core itself, through ResolveThreadOwner, so a foreign or missing
// thread id never reaches a lease, a read, a write or an erase.
//
// The answer is authoritative only when session-core says yes. Anything else is
// a refusal: a "no" is a refusal, and so is every error (session-core down, a
// token that could not be minted, a malformed answer). Only a "yes" is cached,
// and only for a short while, so a thread that stops being the user's is not
// served from memory for long and an outage never turns into an allow.
package threadowner

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// Scope is the one scope sandbox-manager asks session-core for: the narrow right
// to resolve thread ownership and existence, and nothing else of session-core.
// It is requested through the Control principal train (O-27) and is never read
// as an authorization input by sandbox-manager itself.
// Session Core honours it only for ResolveThreadOwner, and only from the sandbox-manager
// principal (auth.rs authorize_thread_owner_resolve); the registry row keyed sandbox-manager
// is what grants it.
const Scope = "session:thread-owner:resolve"

// DefaultCacheTTL is how long a positive answer is served from memory.
const DefaultCacheTTL = 30 * time.Second

// maxCacheEntries bounds the positive cache, so a caller that touches very many
// thread ids cannot grow it without limit.
const maxCacheEntries = 10000

// callTimeout bounds one session-core call, including minting its token.
const callTimeout = 5 * time.Second

// Resolver says whether a user owns a thread in an org. A nil error with false is
// a definite "no"; any error is "could not tell" and the caller must refuse.
type Resolver interface {
	OwnsThread(ctx context.Context, orgID, userID, threadID string) (bool, error)
}

// cacheKey is one (org, user, thread) triple. The user is part of the key, so a
// yes for one user is never served to another.
type cacheKey struct {
	orgID    string
	userID   string
	threadID string
}

// Cache serves a recent "yes" from memory and asks the wrapped Resolver for
// everything else. A "no" and an error are never cached.
type Cache struct {
	next Resolver
	ttl  time.Duration
	now  func() time.Time

	mu      sync.Mutex
	entries map[cacheKey]time.Time
}

// NewCache wraps next with a positive cache of the given lifetime. A non-positive
// ttl takes DefaultCacheTTL.
func NewCache(next Resolver, ttl time.Duration) (*Cache, error) {
	if next == nil {
		return nil, errors.New("threadowner: a resolver is required")
	}
	if ttl <= 0 {
		ttl = DefaultCacheTTL
	}
	return &Cache{next: next, ttl: ttl, now: time.Now, entries: map[cacheKey]time.Time{}}, nil
}

// WithClock returns the cache using now as its clock. It exists for tests.
func (c *Cache) WithClock(now func() time.Time) *Cache {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.now = now
	return c
}

// OwnsThread answers from memory while a yes is fresh, and otherwise asks the
// wrapped Resolver and remembers a yes.
func (c *Cache) OwnsThread(ctx context.Context, orgID, userID, threadID string) (bool, error) {
	key := cacheKey{orgID: orgID, userID: userID, threadID: threadID}
	if c.fresh(key) {
		return true, nil
	}
	owns, err := c.next.OwnsThread(ctx, orgID, userID, threadID)
	if err != nil || !owns {
		c.forget(key)
		return false, err
	}
	c.remember(key)
	return true, nil
}

func (c *Cache) fresh(key cacheKey) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	expires, ok := c.entries[key]
	if !ok {
		return false
	}
	if !c.now().Before(expires) {
		delete(c.entries, key)
		return false
	}
	return true
}

func (c *Cache) forget(key cacheKey) {
	c.mu.Lock()
	defer c.mu.Unlock()
	delete(c.entries, key)
}

func (c *Cache) remember(key cacheKey) {
	c.mu.Lock()
	defer c.mu.Unlock()
	now := c.now()
	if len(c.entries) >= maxCacheEntries {
		for k, expires := range c.entries {
			if !now.Before(expires) {
				delete(c.entries, k)
			}
		}
		if len(c.entries) >= maxCacheEntries {
			// Still full of live entries: start over rather than refuse to answer.
			c.entries = map[cacheKey]time.Time{}
		}
	}
	c.entries[key] = now.Add(c.ttl)
}

// OwnerClient is the part of session-core's RunService client this package uses.
type OwnerClient interface {
	ResolveThreadOwner(ctx context.Context, in *mpv1.ResolveThreadOwnerRequest, opts ...grpc.CallOption) (*mpv1.ResolveThreadOwnerResponse, error)
}

// TokenSource mints a session-core service token bound to one org. The
// servicetoken.Provider is the production implementation.
type TokenSource interface {
	Token(ctx context.Context, orgID string) (string, error)
}

// SessionCore asks session-core's ResolveThreadOwner as sandbox-manager's own
// service principal. The user is named in the request, because a service token
// proves the service and the org and never the user.
type SessionCore struct {
	client OwnerClient
	tokens TokenSource
}

// NewSessionCore builds a resolver over a session-core client and a token source.
func NewSessionCore(client OwnerClient, tokens TokenSource) (*SessionCore, error) {
	if client == nil {
		return nil, errors.New("threadowner: a session-core client is required")
	}
	if tokens == nil {
		return nil, errors.New("threadowner: a token source is required")
	}
	return &SessionCore{client: client, tokens: tokens}, nil
}

// OwnsThread reports session-core's answer. An empty id is refused here, so a
// request session-core would reject as malformed never reads as a "no".
func (s *SessionCore) OwnsThread(ctx context.Context, orgID, userID, threadID string) (bool, error) {
	orgID, userID, threadID = strings.TrimSpace(orgID), strings.TrimSpace(userID), strings.TrimSpace(threadID)
	if orgID == "" || userID == "" || threadID == "" {
		return false, errors.New("threadowner: org, user and thread ids are required")
	}
	ctx, cancel := context.WithTimeout(ctx, callTimeout)
	defer cancel()
	token, err := s.tokens.Token(ctx, orgID)
	if err != nil {
		return false, fmt.Errorf("threadowner: session-core token: %w", err)
	}
	ctx = metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+token)
	resp, err := s.client.ResolveThreadOwner(ctx, &mpv1.ResolveThreadOwnerRequest{
		ThreadId: threadID, OrgId: orgID, UserId: userID,
	})
	if err != nil {
		// Only the code: the message of a refusal can carry request details.
		return false, fmt.Errorf("threadowner: session-core ResolveThreadOwner: %s", status.Code(err))
	}
	return resp.GetAuthorized(), nil
}

// ExistenceTransport is the wire half of the batch existence read: one call that
// asks session-core which of an org's thread ids exist. The gRPC adapter over the
// generated client implements it once the additive ThreadsExist RPC is generated.
// bearer is the service token to present.
type ExistenceTransport interface {
	ThreadsExist(ctx context.Context, bearer, orgID string, threadIDs []string) (map[string]bool, error)
}

// SessionCoreExistence answers the orphan sweep's question as sandbox-manager's
// service principal. It reads ids only: no thread content, owner or title.
type SessionCoreExistence struct {
	transport ExistenceTransport
	tokens    TokenSource
}

// NewSessionCoreExistence builds the existence reader.
func NewSessionCoreExistence(transport ExistenceTransport, tokens TokenSource) (*SessionCoreExistence, error) {
	if transport == nil {
		return nil, errors.New("threadowner: an existence transport is required")
	}
	if tokens == nil {
		return nil, errors.New("threadowner: a token source is required")
	}
	return &SessionCoreExistence{transport: transport, tokens: tokens}, nil
}

// ThreadsExist returns session-core's explicit answer for each id it knows. An id
// session-core did not answer for is absent from the map, and a caller treats an
// absent id as unknown, never as gone.
func (s *SessionCoreExistence) ThreadsExist(ctx context.Context, orgID string, threadIDs []string) (map[string]bool, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, errors.New("threadowner: an org id is required")
	}
	if len(threadIDs) == 0 {
		return map[string]bool{}, nil
	}
	ctx, cancel := context.WithTimeout(ctx, callTimeout)
	defer cancel()
	token, err := s.tokens.Token(ctx, orgID)
	if err != nil {
		return nil, fmt.Errorf("threadowner: session-core token: %w", err)
	}
	answers, err := s.transport.ThreadsExist(ctx, token, orgID, threadIDs)
	if err != nil {
		return nil, fmt.Errorf("threadowner: session-core ThreadsExist: %s", status.Code(err))
	}
	return answers, nil
}
