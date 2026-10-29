package redis

import (
	"context"
	"math/rand"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"
)

// testTTL is a comfortably long TTL so nothing expires mid-test.
const testTTL = 5 * time.Minute

func TestOrgUserIndexKey(t *testing.T) {
	if got, want := orgUserIndexKey("org-123"), "control:sess:org:org-123"; got != want {
		t.Fatalf("orgUserIndexKey = %q, want %q", got, want)
	}

	// The index set must live in a namespace the per-user snapshot wildcard
	// SCAN (controlsession:<userID>:*, see InvalidateControlSession) can
	// never match, so org-only and user-only invalidation cannot clobber
	// each other's keyspace.
	if strings.HasPrefix(orgUserIndexKey("x"), "controlsession:") {
		t.Fatalf("org index key %q collides with the snapshot keyspace", orgUserIndexKey("x"))
	}
}

// TestOrgIndexNilAndEmptySafety verifies the best-effort contract: a nil
// client and empty identifiers are no-ops that never touch Redis and never
// panic. No live Redis required (all paths short-circuit before any command).
func TestOrgIndexNilAndEmptySafety(t *testing.T) {
	ctx := context.Background()

	var nilClient *Client
	if err := nilClient.IndexOrgUser(ctx, "org", "user", testTTL); err != nil {
		t.Fatalf("nil IndexOrgUser: unexpected err %v", err)
	}
	if n, err := nilClient.InvalidateOrgSessions(ctx, "org"); err != nil || n != 0 {
		t.Fatalf("nil InvalidateOrgSessions = (%d,%v), want (0,nil)", n, err)
	}

	// Non-nil client, but empty args short-circuit before dialing, so a
	// bogus address is never contacted.
	c := NewClient("127.0.0.1:0", "", 0)
	t.Cleanup(c.Close)
	if err := c.IndexOrgUser(ctx, "", "user", testTTL); err != nil {
		t.Fatalf("empty-orgID IndexOrgUser: %v", err)
	}
	if err := c.IndexOrgUser(ctx, "org", "", testTTL); err != nil {
		t.Fatalf("empty-userID IndexOrgUser: %v", err)
	}
	if n, err := c.InvalidateOrgSessions(ctx, ""); err != nil || n != 0 {
		t.Fatalf("empty-orgID InvalidateOrgSessions = (%d,%v), want (0,nil)", n, err)
	}
}

// TestInvalidateOrgSessionsRoundTrip exercises the full write+read+invalidate
// path against a real Redis/Dragonfly. It skips when no server is reachable
// so unit-only runs (go test ./...) stay green without a broker.
func TestInvalidateOrgSessionsRoundTrip(t *testing.T) {
	c := testRedis(t)
	ctx := context.Background()

	sfx := randSuffix()
	org1, org2 := "org1-"+sfx, "org2-"+sfx
	userA, userB, userC := "userA-"+sfx, "userB-"+sfx, "userC-"+sfx

	t.Cleanup(func() {
		c.rdb.Del(context.Background(),
			controlSessionKey(userA, org1),
			controlSessionKey(userB, org1),
			controlSessionKey(userC, org2),
			orgUserIndexKey(org1),
			orgUserIndexKey(org2),
		)
	})

	// Warm snapshots + reverse index exactly as ControlSessionService.Get does.
	warm := func(u, o string) {
		t.Helper()
		if err := c.CacheControlSession(ctx, u, o, map[string]string{"u": u, "o": o}, testTTL); err != nil {
			t.Fatalf("CacheControlSession(%s,%s): %v", u, o, err)
		}
		if err := c.IndexOrgUser(ctx, o, u, testTTL); err != nil {
			t.Fatalf("IndexOrgUser(%s,%s): %v", o, u, err)
		}
	}
	warm(userA, org1)
	warm(userB, org1)
	warm(userC, org2)

	// org1 index holds exactly {userA, userB} and carries a positive TTL.
	if card, err := c.rdb.SCard(ctx, orgUserIndexKey(org1)).Result(); err != nil || card != 2 {
		t.Fatalf("org1 index SCARD = (%d,%v), want (2,nil)", card, err)
	}
	if ttl, err := c.rdb.TTL(ctx, orgUserIndexKey(org1)).Result(); err != nil || ttl <= 0 {
		t.Fatalf("org1 index TTL = (%v,%v), want positive (self-healing)", ttl, err)
	}

	// Bust org1: both org1 snapshots gone, index cleared, org2 untouched.
	n, err := c.InvalidateOrgSessions(ctx, org1)
	if err != nil {
		t.Fatalf("InvalidateOrgSessions(org1): %v", err)
	}
	if n != 2 {
		t.Fatalf("InvalidateOrgSessions(org1) returned %d, want 2", n)
	}
	assertAbsent(t, ctx, c, controlSessionKey(userA, org1))
	assertAbsent(t, ctx, c, controlSessionKey(userB, org1))
	assertAbsent(t, ctx, c, orgUserIndexKey(org1))
	assertPresent(t, ctx, c, controlSessionKey(userC, org2)) // other org's snapshot survives

	// Re-running against the now-empty index is a harmless no-op.
	if n, err := c.InvalidateOrgSessions(ctx, org1); err != nil || n != 0 {
		t.Fatalf("second InvalidateOrgSessions(org1) = (%d,%v), want (0,nil)", n, err)
	}
}

// --- helpers ---

func testRedis(t *testing.T) *Client {
	t.Helper()
	addr := firstNonEmpty(os.Getenv("REDIS_TEST_ADDR"), os.Getenv("REDIS_ADDR"), "127.0.0.1:6379")
	c := NewClient(addr, firstNonEmpty(os.Getenv("REDIS_TEST_PASSWORD"), os.Getenv("REDIS_PASSWORD")), 0)
	ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
	defer cancel()
	if err := c.rdb.Ping(ctx).Err(); err != nil {
		c.Close()
		t.Skipf("redis not reachable at %s (%v) — skipping round-trip", addr, err)
	}
	t.Cleanup(c.Close)
	return c
}

func assertAbsent(t *testing.T, ctx context.Context, c *Client, key string) {
	t.Helper()
	if exists, err := c.rdb.Exists(ctx, key).Result(); err != nil {
		t.Fatalf("EXISTS %s: %v", key, err)
	} else if exists != 0 {
		t.Fatalf("key %s should be gone, still present", key)
	}
}

func assertPresent(t *testing.T, ctx context.Context, c *Client, key string) {
	t.Helper()
	if exists, err := c.rdb.Exists(ctx, key).Result(); err != nil {
		t.Fatalf("EXISTS %s: %v", key, err)
	} else if exists != 1 {
		t.Fatalf("key %s should be present, missing", key)
	}
}

func firstNonEmpty(vals ...string) string {
	for _, v := range vals {
		if v != "" {
			return v
		}
	}
	return ""
}

func randSuffix() string {
	return strconv.FormatInt(time.Now().UnixNano(), 36) + "-" + strconv.Itoa(rand.Intn(1_000_000))
}
