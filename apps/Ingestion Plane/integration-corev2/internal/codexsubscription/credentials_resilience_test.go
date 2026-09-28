package codexsubscription

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// testClock is a settable clock for the credential store.
type testClock struct {
	mu sync.Mutex
	at time.Time
}

func newTestClock() *testClock { return &testClock{at: time.Now().UTC().Truncate(time.Second)} }

func (c *testClock) now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.at
}

func (c *testClock) advance(by time.Duration) {
	c.mu.Lock()
	c.at = c.at.Add(by)
	c.mu.Unlock()
}

// rotatingAuth is a token endpoint that rotates refresh tokens and rejects
// any refresh token it already consumed, as the real one does (D7).
func rotatingAuth(answer func(call int) (int, string)) func(int, url.Values) (int, string) {
	var mu sync.Mutex
	consumed := map[string]bool{}
	return func(call int, form url.Values) (int, string) {
		refresh := form.Get("refresh_token")
		mu.Lock()
		reused := consumed[refresh]
		consumed[refresh] = true
		mu.Unlock()
		if reused {
			return http.StatusBadRequest, `{"error":{"code":"refresh_token_reused"}}`
		}
		return answer(call)
	}
}

type diskTokens struct {
	Tokens struct {
		AccessToken  string `json:"access_token"`
		RefreshToken string `json:"refresh_token"`
		IDToken      string `json:"id_token"`
	} `json:"tokens"`
}

func readDiskTokens(t *testing.T, path string) diskTokens {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read auth.json: %v", err)
	}
	var disk diskTokens
	if err := json.Unmarshal(raw, &disk); err != nil {
		t.Fatalf("decode auth.json: %v", err)
	}
	return disk
}

func TestRefreshSurvivesCallerCancel(t *testing.T) {
	var newAccess, newRefresh string
	// The caller goes away once the request has reached the endpoint, never
	// before it is sent, so the test does not race a wall-clock deadline.
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	store, auth, home, now := storeHarness(t, rotatingAuth(func(int) (int, string) {
		// The endpoint has rotated; its answer arrives after the caller left.
		cancel()
		time.Sleep(300 * time.Millisecond)
		return http.StatusOK, grantBody(newAccess, newRefresh, "")
	}))
	newAccess, newRefresh = rotatedAccess(now, sentinel("access2")), sentinel("refresh2")
	fixture := writeAuthJSON(t, home, authOptions{accessExp: now.Add(5 * time.Minute)})
	started := time.Now()
	_, _ = store.Current(ctx, home)
	if elapsed := time.Since(started); elapsed > 5*time.Second {
		t.Fatalf("the detached refresh took %v", elapsed)
	}
	if ctx.Err() == nil || auth.callCount() != 1 {
		t.Fatalf("token calls = %d, caller gone = %v; want the refresh sent before the caller left", auth.callCount(), ctx.Err() != nil)
	}
	if disk := readDiskTokens(t, fixture.path); disk.Tokens.RefreshToken != newRefresh || disk.Tokens.AccessToken != newAccess {
		t.Fatal("the rotated grant was dropped when the caller's deadline passed")
	}
	tokens, err := store.Current(t.Context(), home)
	if err != nil {
		t.Fatalf("next call: %v (the consumed refresh token was replayed)", err)
	}
	if tokens.Refresh.reveal() != newRefresh || auth.callCount() != 1 {
		t.Fatalf("next call: rotated refresh used %v, token calls %d, want true and 1", tokens.Refresh.reveal() == newRefresh, auth.callCount())
	}

	// A caller that is gone before anything was sent sends nothing.
	store, auth, home, now = storeHarness(t, rotatingAuth(func(int) (int, string) { return http.StatusOK, grantBody(rotatedAccess(time.Now(), "x"), "", "") }))
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	gone, cancelGone := context.WithCancel(t.Context())
	cancelGone()
	if _, err := store.Current(gone, home); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled caller: %v, want context.Canceled", err)
	}
	if auth.callCount() != 0 {
		t.Fatalf("a cancelled caller made %d token calls", auth.callCount())
	}
}

func TestDirectDeadlineDuringRefreshKeepsRotatedToken(t *testing.T) {
	var newAccess, newRefresh string
	// The caller's context ends, as its deadline would, once the refresh
	// request has reached the endpoint: never before it is sent, so the test
	// does not race a wall-clock deadline.
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	h := newDirectHarness(t, nil, func(o *harnessOptions) {
		o.respond = rotatingAuth(func(int) (int, string) {
			cancel()
			time.Sleep(300 * time.Millisecond)
			return http.StatusOK, grantBody(newAccess, newRefresh, "")
		})
	})
	newAccess, newRefresh = rotatedAccess(h.now, sentinel("access2")), sentinel("refresh2")
	fixture := writeAuthJSON(t, h.home, authOptions{accessExp: h.now.Add(time.Minute)})
	if _, err := h.transport.Invoke(ctx, h.home, testRequest()); !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want the caller's context error", err)
	}
	if disk := readDiskTokens(t, fixture.path); disk.Tokens.RefreshToken != newRefresh {
		t.Fatal("the rotated refresh token was not persisted")
	}
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
		t.Fatalf("next call: %v", err)
	}
	if h.auth.callCount() != 1 {
		t.Fatalf("token calls = %d, want 1", h.auth.callCount())
	}
	requests := h.upstream.recorded()
	if got := requests[len(requests)-1].header.Get("Authorization"); got != "Bearer "+newAccess {
		t.Fatal("the next call did not use the rotated access token")
	}
}

func TestWriteBackFailureKeepsRotatedGrant(t *testing.T) {
	var soonAccess, access3, refresh2 string
	store, auth, home, now := storeHarness(t, rotatingAuth(func(call int) (int, string) {
		if call == 1 {
			return http.StatusOK, grantBody(soonAccess, refresh2, "")
		}
		return http.StatusOK, grantBody(access3, "", "")
	}))
	// The first rotated access token expires soon, so the next call
	// refreshes again: it must use the rotated refresh token.
	soonAccess = fakeJWT(anyMap{"exp": now.Add(time.Minute).Unix(), "client_id": "app_TESTCLIENT12345"}, sentinel("access2"))
	access3, refresh2 = rotatedAccess(now, sentinel("access3")), sentinel("refresh2")
	var failWrites atomic.Bool
	failWrites.Store(true)
	store.writeFile = func(dir, path string, data []byte, mode os.FileMode) error {
		if failWrites.Load() {
			return errors.New("no space left on device")
		}
		return writeFileAtomic(dir, path, data, mode)
	}
	var logs syncBuffer
	store.logger = store.logger.Output(&logs)
	fixture := writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})

	tokens, err := store.Current(t.Context(), home)
	if err != nil || tokens.Access.reveal() != soonAccess || tokens.Refresh.reveal() != refresh2 {
		t.Fatalf("failed write: err=%v, want the rotated grant in use", err)
	}
	if disk := readDiskTokens(t, fixture.path); disk.Tokens.RefreshToken != fixture.refresh {
		t.Fatal("auth.json changed although the write failed")
	}
	if !strings.Contains(logs.String(), "write_back_failed") {
		t.Fatalf("write failure not logged: %s", logs.String())
	}

	tokens, err = store.Current(t.Context(), home)
	if err != nil || tokens.Access.reveal() != access3 || tokens.Refresh.reveal() != refresh2 {
		t.Fatalf("second refresh: err=%v, want access3 with the rotated refresh token", err)
	}
	auth.mu.Lock()
	secondForm := auth.forms[1]
	auth.mu.Unlock()
	if secondForm.Get("refresh_token") != refresh2 {
		t.Fatal("the second refresh replayed the consumed refresh token")
	}

	failWrites.Store(false)
	if tokens, err = store.Current(t.Context(), home); err != nil || tokens.Access.reveal() != access3 {
		t.Fatalf("recovered disk: err=%v", err)
	}
	if auth.callCount() != 2 {
		t.Fatalf("token calls = %d, want 2", auth.callCount())
	}
	disk := readDiskTokens(t, fixture.path)
	if disk.Tokens.AccessToken != access3 || disk.Tokens.RefreshToken != refresh2 || disk.Tokens.IDToken != fixture.id {
		t.Fatal("the unsaved grant was not written once the disk recovered")
	}
	if _, pending := store.unsavedFor(home); pending {
		t.Fatal("the unsaved grant was kept after it was written")
	}
	if strings.Contains(logs.String(), "SENTINEL") {
		t.Fatal("a token reached the log")
	}

	// Logout removes the file while a grant is unsaved: it is dropped, and
	// the login is never recreated.
	store, _, home, now = storeHarness(t, rotatingAuth(func(int) (int, string) {
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), "late"), sentinel("refresh-late"), "")
	}))
	store.writeFile = func(string, string, []byte, os.FileMode) error { return errors.New("read-only file system") }
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	if _, err := store.Current(t.Context(), home); err != nil {
		t.Fatalf("failed write: %v", err)
	}
	if err := os.Remove(filepath.Join(home, "auth.json")); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Current(t.Context(), home); !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("after logout: %v, want ErrReauthenticationRequired", err)
	}
	if _, err := os.Stat(filepath.Join(home, "auth.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("an unsaved grant recreated auth.json after logout")
	}
	if _, pending := store.unsavedFor(home); pending {
		t.Fatal("the unsaved grant outlived logout")
	}
}

func TestTransientRefreshBackoff(t *testing.T) {
	clock := newTestClock()
	auth := newFakeAuth(t, func(int, url.Values) (int, string) {
		time.Sleep(400 * time.Millisecond)
		return http.StatusServiceUnavailable, `{}`
	})
	store := newCredentialStore(credentialStoreConfig{TokenURL: auth.URL + "/oauth/token", Now: clock.now})
	home := filepath.Join(t.TempDir(), "conn-1")
	fixture := writeAuthJSON(t, home, authOptions{accessExp: clock.now().Add(5 * time.Minute)})

	var wg sync.WaitGroup
	errs := make(chan error, 4)
	for range 4 {
		wg.Go(func() {
			tokens, err := store.Current(t.Context(), home)
			if err == nil && tokens.Access.reveal() != fixture.access {
				err = errors.New("unexpected token")
			}
			if err != nil {
				errs <- err
			}
		})
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Fatalf("concurrent call: %v", err)
	}
	if auth.callCount() != 1 {
		t.Fatalf("token calls = %d, want 1: a transient failure must not repeat per waiter", auth.callCount())
	}
	started := time.Now()
	if _, err := store.Current(t.Context(), home); err != nil {
		t.Fatal(err)
	}
	if auth.callCount() != 1 || time.Since(started) > 200*time.Millisecond {
		t.Fatalf("during the backoff: token calls %d after %v, want 1 and no wait", auth.callCount(), time.Since(started))
	}
	clock.advance(transientRefreshBackoff + time.Second)
	if _, err := store.Current(t.Context(), home); err != nil {
		t.Fatal(err)
	}
	if auth.callCount() != 2 {
		t.Fatalf("after the backoff: token calls = %d, want 2", auth.callCount())
	}

	// An unusable token fails fast during the backoff, without a token call.
	auth = newFakeAuth(t, func(int, url.Values) (int, string) { return http.StatusServiceUnavailable, `{}` })
	store = newCredentialStore(credentialStoreConfig{TokenURL: auth.URL + "/oauth/token", Now: clock.now})
	writeAuthJSON(t, home, authOptions{accessExp: clock.now().Add(-time.Minute)})
	for attempt := range 2 {
		_, err := store.Current(t.Context(), home)
		var upstream *UpstreamError
		if !errors.As(err, &upstream) || upstream.Kind != upstreamKindRefresh {
			t.Fatalf("attempt %d: %v, want token_refresh_unavailable", attempt, err)
		}
	}
	if auth.callCount() != 1 {
		t.Fatalf("expired token: token calls = %d, want 1", auth.callCount())
	}
}
