package sitepolicy

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestParseMode(t *testing.T) {
	for raw, want := range map[string]Mode{
		"":         ModeOff,
		"  ":       ModeOff,
		"off":      ModeOff,
		"shadow":   ModeShadow,
		"enforce":  ModeEnforce,
		"Enforce":  ModeEnforce,
		"typo":     ModeEnforce,
		"true":     ModeEnforce,
		" shadow ": ModeShadow,
	} {
		if got := ParseMode(raw); got != want {
			t.Errorf("ParseMode(%q) = %q, want %q", raw, got, want)
		}
	}
}

func TestNewClientRefusesMissingConfiguration(t *testing.T) {
	for _, tc := range []struct{ name, url, id, token string }{
		{"no url", "", "browser-broker", "tok"},
		{"no id", "http://org-core", "", "tok"},
		{"no token", "http://org-core", "browser-broker", ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := NewClient(tc.url, tc.id, tc.token); err == nil {
				t.Fatal("NewClient accepted incomplete configuration")
			}
		})
	}
}

// fakeOrgCore records every request and answers from a fixed decision map.
type fakeOrgCore struct {
	mu        sync.Mutex
	hits      int
	lastQuery map[string][]string
	lastPath  string
	lastHead  http.Header
	answer    map[string]string
}

func (f *fakeOrgCore) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.hits++
	f.lastPath = r.URL.Path
	f.lastQuery = r.URL.Query()
	f.lastHead = r.Header.Clone()
	var decisions []Decision
	for _, host := range f.lastQuery["host"] {
		effective, ok := f.answer[host]
		if !ok {
			effective = EffectiveAsk
		}
		decisions = append(decisions, Decision{Host: host, Effective: effective})
	}
	_ = json.NewEncoder(w).Encode(Resolution{Decisions: decisions})
}

func (f *fakeOrgCore) hitCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.hits
}

func TestResolveAsksOrgCoreForTheCallerWithTheServiceCredential(t *testing.T) {
	core := &fakeOrgCore{answer: map[string]string{"example.com": EffectiveAct}}
	server := httptest.NewServer(core)
	defer server.Close()
	client, err := NewClient(server.URL+"/", "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}

	resolution, err := client.Resolve(context.Background(), "org1", "user1", "", []string{"example.com", "other.test"})
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if core.lastPath != "/internal/orgs/org1/browser-sites/resolve" {
		t.Errorf("path = %q", core.lastPath)
	}
	if got := core.lastQuery["host"]; len(got) != 2 || got[0] != "example.com" || got[1] != "other.test" {
		t.Errorf("host query = %v", got)
	}
	if got := core.lastQuery["user_id"]; len(got) != 1 || got[0] != "user1" {
		t.Errorf("user_id = %v, want [user1]", got)
	}
	if got := core.lastHead.Get("X-Service-Id"); got != "browser-broker" {
		t.Errorf("X-Service-Id = %q", got)
	}
	if got := core.lastHead.Get("X-Service-Token"); got != "test-token" {
		t.Errorf("X-Service-Token not sent")
	}
	if len(resolution.Decisions) != 2 || resolution.Decisions[0].Effective != EffectiveAct || resolution.Decisions[1].Effective != EffectiveAsk {
		t.Errorf("decisions = %+v", resolution.Decisions)
	}
}

func TestResolveOmitsUserForAServiceCall(t *testing.T) {
	core := &fakeOrgCore{}
	server := httptest.NewServer(core)
	defer server.Close()
	client, err := NewClient(server.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Resolve(context.Background(), "org1", "", "", []string{"example.com"}); err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if _, present := core.lastQuery["user_id"]; present {
		t.Fatal("a service call must not name a user")
	}
}

func TestAllowedAnswerIsReusedWithinThirtySecondsOnly(t *testing.T) {
	core := &fakeOrgCore{answer: map[string]string{"example.com": EffectiveAct}}
	server := httptest.NewServer(core)
	defer server.Close()
	now := time.Unix(1_700_000_000, 0)
	client, err := newClient(server.URL, "browser-broker", "test-token", time.Second, func() time.Time { return now })
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	hosts := []string{"example.com"}

	if _, err := client.Resolve(ctx, "org1", "user1", "", hosts); err != nil {
		t.Fatal(err)
	}
	if _, err := client.Resolve(ctx, "org1", "user1", "", hosts); err != nil {
		t.Fatal(err)
	}
	if got := core.hitCount(); got != 1 {
		t.Fatalf("hits within TTL = %d, want 1", got)
	}
	now = now.Add(31 * time.Second)
	if _, err := client.Resolve(ctx, "org1", "user1", "", hosts); err != nil {
		t.Fatal(err)
	}
	if got := core.hitCount(); got != 2 {
		t.Fatalf("hits after TTL = %d, want 2", got)
	}
}

func TestDenialIsNeverCached(t *testing.T) {
	core := &fakeOrgCore{answer: map[string]string{"blocked.test": EffectiveDeny}}
	server := httptest.NewServer(core)
	defer server.Close()
	client, err := NewClient(server.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		if _, err := client.Resolve(context.Background(), "org1", "user1", "", []string{"blocked.test"}); err != nil {
			t.Fatal(err)
		}
	}
	if got := core.hitCount(); got != 2 {
		t.Fatalf("hits = %d, want 2: a denial must be read fresh every time", got)
	}
}

func TestNonOKAnswerIsUnavailableWithoutTheURLOrToken(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()
	client, err := NewClient(server.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.Resolve(context.Background(), "org1", "user1", "", []string{"example.com"})
	if !errors.Is(err, ErrUnavailable) {
		t.Fatalf("err = %v, want ErrUnavailable", err)
	}
	if strings.Contains(err.Error(), "test-token") || strings.Contains(err.Error(), server.URL) {
		t.Fatalf("error leaks the credential or URL: %q", err)
	}
}

func TestMalformedAnswerIsUnavailable(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("not json"))
	}))
	defer server.Close()
	client, err := NewClient(server.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Resolve(context.Background(), "org1", "user1", "", []string{"example.com"}); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("err = %v, want ErrUnavailable", err)
	}
}

func TestSlowOrgCoreIsUnavailable(t *testing.T) {
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		<-release
	}))
	defer server.Close()
	defer close(release)
	client, err := newClient(server.URL, "browser-broker", "test-token", 50*time.Millisecond, time.Now)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Resolve(context.Background(), "org1", "user1", "", []string{"example.com"}); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("err = %v, want ErrUnavailable", err)
	}
}

func TestRedirectIsNotFollowedWithTheCredential(t *testing.T) {
	var mu sync.Mutex
	followed := 0
	elsewhere := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		mu.Lock()
		followed++
		mu.Unlock()
		w.WriteHeader(http.StatusOK)
	}))
	defer elsewhere.Close()
	redirecting := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, elsewhere.URL+r.URL.Path, http.StatusTemporaryRedirect)
	}))
	defer redirecting.Close()
	client, err := NewClient(redirecting.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Resolve(context.Background(), "org1", "user1", "", []string{"example.com"}); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("err = %v, want ErrUnavailable", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if followed != 0 {
		t.Fatalf("redirect was followed %d times with the service token", followed)
	}
}

// spaceOrgCore answers example.com as act for the org-wide call and for space-a, and as deny for space-b. It records the
// Space every call names.
type spaceOrgCore struct {
	mu     sync.Mutex
	hits   int
	spaces []string
}

func (f *spaceOrgCore) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.hits++
	space := r.URL.Query().Get("space_id")
	f.spaces = append(f.spaces, space)
	effective := EffectiveAct
	if space == "space-b" {
		effective = EffectiveDeny
	}
	var decisions []Decision
	for _, host := range r.URL.Query()["host"] {
		decisions = append(decisions, Decision{Host: host, Effective: effective})
	}
	_ = json.NewEncoder(w).Encode(Resolution{Decisions: decisions})
}

func (f *spaceOrgCore) hitCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.hits
}

// F-f10-31: an allow held for one Space never answers another Space's call, or the org-wide call, for the same org, user
// and host. The empty Space keeps its own entries, as it did before the Space was keyed.
func TestAllowedAnswerIsKeyedBySpace(t *testing.T) {
	core := &spaceOrgCore{}
	server := httptest.NewServer(core)
	defer server.Close()
	client, err := NewClient(server.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	hosts := []string{"example.com"}
	steps := []struct {
		space     string
		wantHits  int
		wantAllow string
	}{
		{space: "space-a", wantHits: 1, wantAllow: EffectiveAct},
		{space: "space-a", wantHits: 1, wantAllow: EffectiveAct},  // Space A's own allow, from the cache
		{space: "", wantHits: 2, wantAllow: EffectiveAct},         // the org-wide call is not Space A's answer
		{space: "", wantHits: 2, wantAllow: EffectiveAct},         // and its own allow is reused
		{space: "space-b", wantHits: 3, wantAllow: EffectiveDeny}, // Space B is asked, and gets its own deny
		{space: "space-a", wantHits: 3, wantAllow: EffectiveAct},  // Space A's allow still answers Space A
	}
	for i, step := range steps {
		resolution, err := client.Resolve(ctx, "org1", "user1", step.space, hosts)
		if err != nil {
			t.Fatalf("step %d: Resolve: %v", i, err)
		}
		if got := core.hitCount(); got != step.wantHits {
			t.Fatalf("step %d (space %q): org-core hits = %d, want %d", i, step.space, got, step.wantHits)
		}
		if len(resolution.Decisions) != 1 || resolution.Decisions[0].Effective != step.wantAllow {
			t.Fatalf("step %d (space %q): decisions = %+v, want %s", i, step.space, resolution.Decisions, step.wantAllow)
		}
	}
	wantSpaces := []string{"space-a", "", "space-b"}
	if got := core.spaces; len(got) != len(wantSpaces) || got[0] != wantSpaces[0] || got[1] != wantSpaces[1] || got[2] != wantSpaces[2] {
		t.Fatalf("spaces named to org-core = %q, want %q", got, wantSpaces)
	}
}

// F-f10-31: a Space is named to org-core only when the call has one, so the org-wide call's query is unchanged.
func TestResolveNamesTheSpaceOnlyWhenItHasOne(t *testing.T) {
	core := &fakeOrgCore{}
	server := httptest.NewServer(core)
	defer server.Close()
	client, err := NewClient(server.URL, "browser-broker", "test-token")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := client.Resolve(context.Background(), "org1", "user1", "space-a", []string{"example.com"}); err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if got := core.lastQuery["space_id"]; len(got) != 1 || got[0] != "space-a" {
		t.Fatalf("space_id = %v, want [space-a]", got)
	}
	if _, err := client.Resolve(context.Background(), "org1", "user1", "", []string{"example.com"}); err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if _, present := core.lastQuery["space_id"]; present {
		t.Fatal("an org-wide call must not name a Space")
	}
}
