package codexsubscription

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"
)

// storeHarness is a credential store against a fake token endpoint.
func storeHarness(t *testing.T, respond func(call int, form url.Values) (int, string)) (*credentialStore, *fakeAuth, string, time.Time) {
	t.Helper()
	now := time.Now().UTC().Truncate(time.Second)
	auth := newFakeAuth(t, respond)
	store := newCredentialStore(credentialStoreConfig{TokenURL: auth.URL + "/oauth/token", Now: func() time.Time { return now }})
	home := filepath.Join(t.TempDir(), "conn-1")
	if err := os.MkdirAll(home, 0o700); err != nil {
		t.Fatal(err)
	}
	return store, auth, home, now
}

func rotatedAccess(now time.Time, sig string) string {
	return fakeJWT(anyMap{"exp": now.Add(time.Hour).Unix(), "client_id": "app_TESTCLIENT12345"}, sig)
}

func TestReadAuthJSON(t *testing.T) {
	store, _, home, now := storeHarness(t, nil)
	fixture := writeAuthJSON(t, home, authOptions{
		accessExp:   now.Add(time.Hour),
		accountID:   strPtr("acct-file"),
		lastRefresh: now.Format(time.RFC3339Nano),
		idClaims:    anyMap{openAIAuthClaim: anyMap{"chatgpt_account_is_fedramp": true}},
	})
	tokens, err := store.Current(t.Context(), home)
	if err != nil {
		t.Fatalf("Current: %v", err)
	}
	if tokens.Access.reveal() != fixture.access || tokens.Refresh.reveal() != fixture.refresh || tokens.ID.reveal() != fixture.id {
		t.Fatal("token values were not read from their key paths")
	}
	if tokens.AccountID != "acct-file" || !tokens.AccessExpiry.Equal(now.Add(time.Hour)) || !tokens.LastRefresh.Equal(now) || tokens.ClientID != "app_TESTCLIENT12345" || !tokens.FedRAMP {
		t.Fatalf("claims = account %q exp %v last %v client %q fedramp %v", tokens.AccountID, tokens.AccessExpiry, tokens.LastRefresh, tokens.ClientID, tokens.FedRAMP)
	}

	reauth := map[string]func(dir string){
		"missing": func(string) {},
		"empty":   func(dir string) { _ = os.WriteFile(filepath.Join(dir, "auth.json"), nil, 0o600) },
		"directory": func(dir string) {
			_ = os.MkdirAll(filepath.Join(dir, "auth.json"), 0o700)
		},
		"apikey mode": func(dir string) {
			writeAuthJSON(t, dir, authOptions{authMode: "apikey", accessExp: now.Add(time.Hour)})
		},
		"no tokens": func(dir string) {
			_ = os.WriteFile(filepath.Join(dir, "auth.json"), []byte(`{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":null}`), 0o600)
		},
		"no refresh token": func(dir string) {
			_ = os.WriteFile(filepath.Join(dir, "auth.json"), []byte(`{"tokens":{"access_token":"a.b.c"}}`), 0o600)
		},
		"garbage": func(dir string) { _ = os.WriteFile(filepath.Join(dir, "auth.json"), []byte(`{"tokens":`), 0o600) },
	}
	for name, prepare := range reauth {
		dir := filepath.Join(t.TempDir(), "conn")
		_ = os.MkdirAll(dir, 0o700)
		prepare(dir)
		if _, err := store.Current(t.Context(), dir); !errors.Is(err, ErrReauthenticationRequired) {
			t.Fatalf("%s: error = %v, want ErrReauthenticationRequired", name, err)
		}
	}

	// A torn read is retried: invalid first, valid shortly after.
	torn := filepath.Join(t.TempDir(), "conn")
	_ = os.MkdirAll(torn, 0o700)
	if err := os.WriteFile(filepath.Join(torn, "auth.json"), []byte(`{"tokens":{"acc`), 0o600); err != nil {
		t.Fatal(err)
	}
	valid := filepath.Join(t.TempDir(), "valid")
	fixture = writeAuthJSON(t, valid, authOptions{accessExp: now.Add(time.Hour), lastRefresh: now.Format(time.RFC3339)})
	content, _ := os.ReadFile(fixture.path)
	time.AfterFunc(20*time.Millisecond, func() { _ = os.WriteFile(filepath.Join(torn, "auth.json"), content, 0o600) })
	if tokens, err := store.Current(t.Context(), torn); err != nil || tokens.Access.reveal() != fixture.access {
		t.Fatalf("torn read: err=%v", err)
	}
}

func TestRefreshBeforeExpiry(t *testing.T) {
	var newAccess string
	h := newDirectHarness(t, nil, func(o *harnessOptions) {
		o.respond = func(int, url.Values) (int, string) { return http.StatusOK, grantBody(newAccess, "", "") }
	})
	newAccess = rotatedAccess(h.now, sentinel("rotated"))
	writeAuthJSON(t, h.home, authOptions{accessExp: h.now.Add(9 * time.Minute), lastRefresh: h.now.Format(time.RFC3339)})
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
		t.Fatal(err)
	}
	if h.auth.callCount() != 1 {
		t.Fatalf("token calls = %d, want 1", h.auth.callCount())
	}
	if got := h.upstream.recorded()[0].header.Get("Authorization"); got != "Bearer "+newAccess {
		t.Fatal("the upstream did not receive the refreshed access token")
	}
	if !strings.Contains(h.logs.String(), `"refreshed":"proactive"`) {
		t.Fatalf("proactive refresh not logged: %s", h.logs.String())
	}

	cases := []struct {
		name  string
		opts  authOptions
		calls int
	}{
		{name: "11m left", opts: authOptions{accessExp: h.now.Add(11 * time.Minute), lastRefresh: h.now.Format(time.RFC3339)}, calls: 0},
		{name: "no exp, 9 days", opts: authOptions{lastRefresh: h.now.Add(-9 * 24 * time.Hour).Format(time.RFC3339)}, calls: 1},
		{name: "no exp, 7 days", opts: authOptions{lastRefresh: h.now.Add(-7 * 24 * time.Hour).Format(time.RFC3339)}, calls: 0},
		{name: "no exp, no last_refresh", opts: authOptions{}, calls: 1},
	}
	for _, tc := range cases {
		store, auth, home, _ := storeHarness(t, func(int, url.Values) (int, string) {
			return http.StatusOK, grantBody(rotatedAccess(time.Now(), "x"), "", "")
		})
		writeAuthJSON(t, home, tc.opts)
		if _, err := store.Current(t.Context(), home); err != nil {
			t.Fatalf("%s: %v", tc.name, err)
		}
		if auth.callCount() != tc.calls {
			t.Fatalf("%s: token calls = %d, want %d", tc.name, auth.callCount(), tc.calls)
		}
	}
}

func TestRefreshRequest(t *testing.T) {
	h := newDirectHarness(t, nil, func(o *harnessOptions) {
		o.respond = func(int, url.Values) (int, string) {
			return http.StatusOK, grantBody(rotatedAccess(time.Now(), "rotated"), "", "")
		}
	})
	fixture := writeAuthJSON(t, h.home, authOptions{accessExp: h.now.Add(time.Minute)})
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
		t.Fatal(err)
	}
	h.auth.mu.Lock()
	form, headers := h.auth.forms[0], h.auth.headers[0]
	h.auth.mu.Unlock()
	want := url.Values{"grant_type": {"refresh_token"}, "refresh_token": {fixture.refresh}, "client_id": {"app_TESTCLIENT12345"}}
	if form.Encode() != want.Encode() {
		t.Fatal("refresh form is not exactly grant_type, refresh_token and client_id")
	}
	if headers.Get("Content-Type") != "application/x-www-form-urlencoded" || headers.Get("Originator") != "verevon" || !userAgentPattern.MatchString(headers.Get("User-Agent")) {
		t.Fatalf("token request headers: content-type %q originator %q", headers.Get("Content-Type"), headers.Get("Originator"))
	}
	for key, values := range headers {
		for _, value := range values {
			if strings.Contains(value, fixture.access) {
				t.Fatalf("the access token was sent to the token endpoint in %s", key)
			}
		}
	}
	upstream := h.upstream.recorded()[0]
	if bytes.Contains(upstream.body, []byte(fixture.refresh)) {
		t.Fatal("the refresh token reached the Responses endpoint body")
	}
	for key, values := range upstream.header {
		for _, value := range values {
			if strings.Contains(value, fixture.refresh) {
				t.Fatalf("the refresh token reached the Responses endpoint in %s", key)
			}
		}
	}

	// Without a valid client_id claim the Codex public client id is used.
	store, auth, home, now := storeHarness(t, func(int, url.Values) (int, string) {
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), "x"), "", "")
	})
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute), accessClaims: anyMap{"client_id": "not-an-app-id"}})
	if _, err := store.Current(t.Context(), home); err != nil {
		t.Fatal(err)
	}
	auth.mu.Lock()
	defer auth.mu.Unlock()
	if auth.forms[0].Get("client_id") != codexPublicClientID {
		t.Fatalf("client_id = %q, want the Codex public client", auth.forms[0].Get("client_id"))
	}
}

func TestRefreshResponseMerging(t *testing.T) {
	var access string
	store, _, home, now := storeHarness(t, func(int, url.Values) (int, string) { return http.StatusOK, grantBody(access, "", "") })
	access = rotatedAccess(now, sentinel("rotated"))
	fixture := writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute), accountID: strPtr("acct-keep"), lastRefresh: now.Add(-time.Hour).Format(time.RFC3339)})
	tokens, err := store.Current(t.Context(), home)
	if err != nil {
		t.Fatal(err)
	}
	if tokens.Access.reveal() != access || tokens.Refresh.reveal() != fixture.refresh || tokens.ID.reveal() != fixture.id || tokens.AccountID != "acct-keep" {
		t.Fatal("omitted refresh/id tokens or the account id were not kept")
	}
	var disk struct {
		Tokens struct {
			AccessToken  string `json:"access_token"`
			RefreshToken string `json:"refresh_token"`
			IDToken      string `json:"id_token"`
			AccountID    string `json:"account_id"`
		} `json:"tokens"`
		LastRefresh string `json:"last_refresh"`
	}
	raw, _ := os.ReadFile(fixture.path)
	if err := json.Unmarshal(raw, &disk); err != nil {
		t.Fatal(err)
	}
	if disk.Tokens.AccessToken != access || disk.Tokens.RefreshToken != fixture.refresh || disk.Tokens.IDToken != fixture.id || disk.Tokens.AccountID != "acct-keep" {
		t.Fatal("auth.json was not merged in place")
	}
	parsed, err := time.Parse(time.RFC3339, disk.LastRefresh)
	if err != nil || !parsed.Equal(now) {
		t.Fatalf("last_refresh = %q, want the fake now", disk.LastRefresh)
	}

	// A rotated refresh token and id token replace the old ones.
	newRefresh, newID := sentinel("refresh2"), fakeJWT(anyMap{}, sentinel("id2"))
	store2, _, home2, now2 := storeHarness(t, func(int, url.Values) (int, string) {
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), "y"), newRefresh, newID)
	})
	writeAuthJSON(t, home2, authOptions{accessExp: now2.Add(time.Minute)})
	tokens, err = store2.Current(t.Context(), home2)
	if err != nil || tokens.Refresh.reveal() != newRefresh || tokens.ID.reveal() != newID {
		t.Fatalf("rotation not applied: %v", err)
	}
}

func TestAtomicWriteBackFormat(t *testing.T) {
	store, _, home, now := storeHarness(t, func(int, url.Values) (int, string) {
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), "z"), "", "")
	})
	extra := `"agent_identity": {"b": 2, "a": [1, 2], "html": "<&>"},
  "zzz_future": "keep"`
	fixture := writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute), accountID: strPtr("acct"), lastRefresh: now.Format(time.RFC3339), extra: extra, mode: 0o644})
	before, _ := os.Stat(fixture.path)
	if _, err := store.Current(t.Context(), home); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(fixture.path)
	if err != nil {
		t.Fatal(err)
	}
	document, err := parseOrderedObject(data)
	if err != nil {
		t.Fatal(err)
	}
	var keys []string
	for _, member := range document {
		keys = append(keys, member.Key)
	}
	if strings.Join(keys, ",") != "auth_mode,OPENAI_API_KEY,tokens,last_refresh,agent_identity,zzz_future" {
		t.Fatalf("top-level key order = %v", keys)
	}
	rawTokens, _ := document.get("tokens")
	tokens, _ := parseOrderedObject(rawTokens)
	keys = keys[:0]
	for _, member := range tokens {
		keys = append(keys, member.Key)
	}
	if strings.Join(keys, ",") != "id_token,access_token,refresh_token,account_id" {
		t.Fatalf("tokens key order = %v", keys)
	}
	identity, _ := document.get("agent_identity")
	var compact bytes.Buffer
	_ = json.Compact(&compact, identity)
	if compact.String() != `{"b":2,"a":[1,2],"html":"<&>"}` {
		t.Fatalf("agent_identity = %s", compact.String())
	}
	if future := document.getString("zzz_future"); future != "keep" {
		t.Fatalf("zzz_future = %q", future)
	}
	var reindented bytes.Buffer
	compact.Reset()
	_ = json.Compact(&compact, data)
	_ = json.Indent(&reindented, compact.Bytes(), "", "  ")
	if !bytes.Equal(reindented.Bytes(), data) || bytes.HasSuffix(data, []byte("\n")) || !bytes.HasPrefix(data, []byte("{\n  \"auth_mode\": \"chatgpt\",")) {
		t.Fatalf("auth.json is not 2-space pretty JSON without a trailing newline:\n%s", data)
	}
	entries, _ := os.ReadDir(home)
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".auth.json.") {
			t.Fatalf("temp file %s remained", entry.Name())
		}
	}
	if runtime.GOOS == "windows" {
		return
	}
	after, _ := os.Stat(fixture.path)
	if after.Mode().Perm() != 0o600 {
		t.Fatalf("mode = %v, want 0600", after.Mode().Perm())
	}
	if os.SameFile(before, after) {
		t.Fatal("auth.json was rewritten in place instead of renamed over")
	}
}

func TestWriteBackAfterLogoutDoesNotResurrect(t *testing.T) {
	var home string
	store, _, dir, now := storeHarness(t, func(int, url.Values) (int, string) {
		// Logout deletes the file while the token endpoint is answering.
		_ = os.Remove(filepath.Join(home, "auth.json"))
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), "late"), "", "")
	})
	home = dir
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	if _, err := store.Current(t.Context(), home); !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("error = %v, want ErrReauthenticationRequired", err)
	}
	if _, err := os.Stat(filepath.Join(home, "auth.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("a refresh recreated auth.json after logout")
	}
}

func TestPermanentRefreshFailures(t *testing.T) {
	cases := map[string]struct {
		status int
		body   string
	}{
		"401":                       {http.StatusUnauthorized, `{}`},
		"invalid_grant":             {http.StatusBadRequest, `{"error":"invalid_grant","error_description":"SENTINEL-description"}`},
		"refresh_token_reused":      {http.StatusBadRequest, `{"error":{"code":"refresh_token_reused","message":"SENTINEL-description"}}`},
		"refresh_token_expired":     {http.StatusBadRequest, `{"error":{"code":"refresh_token_expired"}}`},
		"refresh_token_invalidated": {http.StatusBadRequest, `{"code":"refresh_token_invalidated"}`},
		"invalid_client":            {http.StatusBadRequest, `{"error":"invalid_client"}`},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			store, auth, home, now := storeHarness(t, func(int, url.Values) (int, string) { return tc.status, tc.body })
			var logs syncBuffer
			store.logger = store.logger.Output(&logs)
			writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
			for range 2 {
				if _, err := store.Current(t.Context(), home); !errors.Is(err, ErrReauthenticationRequired) {
					t.Fatalf("error = %v, want ErrReauthenticationRequired", err)
				}
			}
			if auth.callCount() != 1 {
				t.Fatalf("token calls = %d, want 1 (the failure is cached)", auth.callCount())
			}
			if strings.Contains(logs.String(), "SENTINEL") {
				t.Fatal("error_description reached the log")
			}
			// A new login (new refresh token) clears the cache.
			writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
			_, _ = store.Current(t.Context(), home)
			if auth.callCount() != 2 {
				t.Fatalf("token calls after re-login = %d, want 2", auth.callCount())
			}
		})
	}
}

func TestTransientRefreshFailure(t *testing.T) {
	store, _, home, now := storeHarness(t, func(int, url.Values) (int, string) { return http.StatusServiceUnavailable, `{}` })
	var logs syncBuffer
	store.logger = store.logger.Output(&logs)
	fixture := writeAuthJSON(t, home, authOptions{accessExp: now.Add(5 * time.Minute)})
	tokens, err := store.Current(t.Context(), home)
	if err != nil || tokens.Access.reveal() != fixture.access {
		t.Fatalf("still-valid token: err=%v", err)
	}
	if !strings.Contains(logs.String(), "refresh_failed_transient") {
		t.Fatalf("transient failure not logged: %s", logs.String())
	}
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(-time.Minute)})
	_, err = store.Current(t.Context(), home)
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindRefresh {
		t.Fatalf("expired token error = %v, want token_refresh_unavailable", err)
	}
}

// A 401 that the one successful forced refresh did not cure is transient, as
// on the app-server transport: the token endpoint accepted the sign-in, so
// the connection must not be marked needs_refresh.
func TestUnauthorizedAfterSuccessfulRecoveryIsTransient(t *testing.T) {
	h := newDirectHarness(t, []fakeTurn{{status: http.StatusUnauthorized, body: `{}`, headers: map[string]string{"x-request-id": "req_upstream_401"}}}, func(o *harnessOptions) {
		o.respond = func(int, url.Values) (int, string) {
			return http.StatusOK, grantBody(rotatedAccess(time.Now(), "r"), "", "")
		}
	})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if err == nil || errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("error = %v, want a transient failure, not ErrReauthenticationRequired", err)
	}
	var upstream *UpstreamError
	if !errors.As(err, &upstream) || upstream.Kind != upstreamKindUnauthorized || upstream.HTTPStatus != http.StatusUnauthorized || upstream.RequestID != "req_upstream_401" {
		t.Fatalf("error = %#v, want an unauthorized_after_recovery UpstreamError", err)
	}
	if got := ClassifyFailure(err, time.Now()).Code; got != FailureUpstreamUnavailable {
		t.Fatalf("classified as %s, want %s", got, FailureUpstreamUnavailable)
	}
	if h.auth.callCount() != 1 || h.upstream.count() != 2 {
		t.Fatalf("token calls = %d upstream = %d, want exactly one refresh and one retry", h.auth.callCount(), h.upstream.count())
	}
	if logs := h.logs.String(); !strings.Contains(logs, `"reason":"subscription_upstream_unavailable"`) || !strings.Contains(logs, `"error_kind":"unauthorized_after_recovery"`) {
		t.Fatalf("log = %s", logs)
	}
}

// A 401 whose forced refresh the token endpoint refuses is the one case that
// needs a new sign-in.
func TestUnauthorizedRefusedRecoveryIsReauth(t *testing.T) {
	h := newDirectHarness(t, []fakeTurn{{status: http.StatusUnauthorized, body: `{}`}}, func(o *harnessOptions) {
		o.respond = func(int, url.Values) (int, string) {
			return http.StatusBadRequest, `{"error":"invalid_grant"}`
		}
	})
	freshAuth(t, h.home, h.now)
	_, err := h.transport.Invoke(t.Context(), h.home, testRequest())
	if !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("error = %v, want ErrReauthenticationRequired", err)
	}
	if h.auth.callCount() != 1 || h.upstream.count() != 1 {
		t.Fatalf("token calls = %d upstream = %d, want 1 and 1", h.auth.callCount(), h.upstream.count())
	}
}

// A 401 after answer text cannot be retried, but still gets the one recovery:
// a successful one leaves a transient failure for the next call to recover
// from, a refused one is reauthentication.
func TestUnauthorizedAfterAnswerText(t *testing.T) {
	cases := []struct {
		name    string
		respond func(int, url.Values) (int, string)
		want    FailureCode
	}{
		{name: "recovery succeeds", respond: func(int, url.Values) (int, string) {
			return http.StatusOK, grantBody(rotatedAccess(time.Now(), "r"), "", "")
		}, want: FailureUpstreamUnavailable},
		{name: "recovery refused", respond: func(int, url.Values) (int, string) {
			return http.StatusBadRequest, `{"error":"invalid_grant"}`
		}, want: FailureReauthRequired},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newDirectHarness(t, []fakeTurn{{frames: []string{
				createdFrame("gpt-5.6-terra"),
				deltaFrame("partial"),
				sse(anyMap{"type": "error", "status_code": http.StatusUnauthorized}),
			}}}, func(o *harnessOptions) { o.respond = tc.respond })
			freshAuth(t, h.home, h.now)
			forwarded := 0
			_, err := h.transport.InvokeStream(t.Context(), h.home, testRequest(), func(string) error {
				forwarded++
				return nil
			})
			if got := ClassifyFailure(err, time.Now()).Code; got != tc.want {
				t.Fatalf("error = %v classifies as %s, want %s", err, got, tc.want)
			}
			if (tc.want == FailureReauthRequired) != errors.Is(err, ErrReauthenticationRequired) {
				t.Fatalf("error = %v: only a refused recovery may mark the connection needs_refresh", err)
			}
			if forwarded != 1 || h.upstream.count() != 1 || h.auth.callCount() != 1 {
				t.Fatalf("forwarded = %d upstream = %d token calls = %d, want 1, 1 and 1: no retry after text", forwarded, h.upstream.count(), h.auth.callCount())
			}
		})
	}
}

func TestUnauthorizedAdoptsDiskToken(t *testing.T) {
	var adopted authFixture
	h := newDirectHarness(t, nil)
	h.upstream.setScript(
		fakeTurn{status: http.StatusUnauthorized, body: `{}`, before: func(*http.Request) {
			// Another process refreshed while this call was in flight.
			adopted = writeAuthJSON(t, h.home, authOptions{accessExp: h.now.Add(time.Hour), accountID: strPtr("acct-file")})
		}},
		fakeTurn{frames: successFrames("gpt-5.6-terra", "ok")},
	)
	freshAuth(t, h.home, h.now)
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); err != nil {
		t.Fatal(err)
	}
	if h.auth.callCount() != 0 {
		t.Fatalf("token calls = %d, want 0", h.auth.callCount())
	}
	if got := h.upstream.recorded()[1].header.Get("Authorization"); got != "Bearer "+adopted.access {
		t.Fatal("the retry did not use the token found on disk")
	}
}

func TestUnauthorizedAccountMismatch(t *testing.T) {
	h := newDirectHarness(t, nil)
	h.upstream.setScript(fakeTurn{status: http.StatusUnauthorized, body: `{}`, before: func(*http.Request) {
		writeAuthJSON(t, h.home, authOptions{accessExp: h.now.Add(time.Hour), accountID: strPtr("acct-other")})
	}})
	freshAuth(t, h.home, h.now)
	if _, err := h.transport.Invoke(t.Context(), h.home, testRequest()); !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("error = %v, want ErrReauthenticationRequired", err)
	}
	if h.auth.callCount() != 0 || h.upstream.count() != 1 {
		t.Fatalf("token calls = %d upstream = %d, want 0 and 1", h.auth.callCount(), h.upstream.count())
	}
}

func TestConcurrentRefreshSingleFlight(t *testing.T) {
	var newAccess string
	h := newDirectHarness(t, nil, func(o *harnessOptions) {
		o.respond = func(int, url.Values) (int, string) {
			time.Sleep(50 * time.Millisecond)
			return http.StatusOK, grantBody(newAccess, "", "")
		}
	})
	newAccess = rotatedAccess(h.now, sentinel("rotated"))
	writeAuthJSON(t, h.home, authOptions{accessExp: h.now.Add(time.Minute)})
	var wg sync.WaitGroup
	errs := make(chan error, 20)
	for range 20 {
		wg.Go(func() {
			if _, err := h.transport.Invoke(context.Background(), h.home, testRequest()); err != nil {
				errs <- err
			}
		})
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Fatalf("Invoke: %v", err)
	}
	if h.auth.callCount() != 1 {
		t.Fatalf("token calls = %d, want exactly 1", h.auth.callCount())
	}
	requests := h.upstream.recorded()
	if len(requests) != 20 {
		t.Fatalf("upstream requests = %d, want 20", len(requests))
	}
	for _, request := range requests {
		if request.header.Get("Authorization") != "Bearer "+newAccess {
			t.Fatal("an upstream request used the stale token")
		}
	}
}

type blockingAuthenticator struct {
	home    string
	entered chan struct{}
}

func (b *blockingAuthenticator) BeginDeviceLogin(context.Context, string) (LoginProcess, DeviceCode, error) {
	return nil, DeviceCode{}, errors.New("not used")
}

func (b *blockingAuthenticator) Logout(context.Context, string) error {
	close(b.entered)
	return os.Remove(filepath.Join(b.home, "auth.json"))
}

func TestLogoutHoldsLock(t *testing.T) {
	release := make(chan struct{})
	tokenCalled := make(chan struct{})
	var once sync.Once
	store, _, home, now := storeHarness(t, func(int, url.Values) (int, string) {
		once.Do(func() { close(tokenCalled) })
		<-release
		return http.StatusOK, grantBody(rotatedAccess(time.Now(), "during"), "", "")
	})
	writeAuthJSON(t, home, authOptions{accessExp: now.Add(time.Minute)})
	authenticator := &blockingAuthenticator{home: home, entered: make(chan struct{})}
	runner := NewRunnerWithTransport(authenticator, nil, store)

	refreshed := make(chan error, 1)
	go func() {
		_, err := store.Current(context.Background(), home)
		refreshed <- err
	}()
	<-tokenCalled
	loggedOut := make(chan error, 1)
	go func() { loggedOut <- runner.Logout(context.Background(), home) }()
	select {
	case <-authenticator.entered:
		t.Fatal("logout ran while a refresh held the lock")
	case <-time.After(150 * time.Millisecond):
	}
	close(release)
	if err := <-refreshed; err != nil {
		t.Fatalf("refresh: %v", err)
	}
	if err := <-loggedOut; err != nil {
		t.Fatalf("logout: %v", err)
	}
	if _, err := store.Current(t.Context(), home); !errors.Is(err, ErrReauthenticationRequired) {
		t.Fatalf("after logout: %v, want ErrReauthenticationRequired", err)
	}
}
