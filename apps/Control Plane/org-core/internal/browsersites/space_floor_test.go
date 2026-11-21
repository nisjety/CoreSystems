package browsersites

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

// floorTestToken is a low-entropy fixture for the service token on the user-core hop.
const floorTestToken = "fixture-floor-token-aaaaaaaaaaaaaaaaaaaa"

// floorCall is one floor read the fake user-core saw.
type floorCall struct {
	path      string
	orgID     string
	serviceID string
	token     string
}

// floorCallLog records the reads a fake user-core answered.
type floorCallLog struct {
	mu    sync.Mutex
	calls []floorCall
}

func (l *floorCallLog) record(call floorCall) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.calls = append(l.calls, call)
}

func (l *floorCallLog) snapshot() []floorCall {
	l.mu.Lock()
	defer l.mu.Unlock()
	return append([]floorCall(nil), l.calls...)
}

// newFakeUserCore serves every floor read with answer and records each read. The server is
// closed when the test ends.
func newFakeUserCore(t *testing.T, answer http.HandlerFunc) (*httptest.Server, *floorCallLog) {
	t.Helper()
	calls := &floorCallLog{}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.record(floorCall{
			path:      r.URL.Path,
			orgID:     r.URL.Query().Get("org_id"),
			serviceID: r.Header.Get("X-Service-Id"),
			token:     r.Header.Get("X-Service-Token"),
		})
		answer(w, r)
	}))
	t.Cleanup(server.Close)
	return server, calls
}

// answerWith answers every read with one status and body.
func answerWith(status int, body string) http.HandlerFunc {
	return func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}
}

// entitledBody is user-core's answer for one Space.
func entitledBody(spaceID string, entitled bool) string {
	return fmt.Sprintf(`{"data":{"space_ref":%q,"browser_entitled":%t}}`, spaceID, entitled)
}

func TestUserCoreFloorReaderReadsTheStoredBitAndSendsItsIdentity(t *testing.T) {
	for _, entitled := range []bool{true, false} {
		server, calls := newFakeUserCore(t, answerWith(http.StatusOK, entitledBody("sp-1", entitled)))
		reader := NewUserCoreFloorReader(server.URL, floorTestToken, nil)
		if reader == nil {
			t.Fatal("a reader with a URL and a token must exist")
		}
		got, err := reader.BrowserFloor(context.Background(), "org-1", "sp-1")
		if err != nil || got != entitled {
			t.Fatalf("entitled=%v: BrowserFloor = (%v, %v)", entitled, got, err)
		}
		seen := calls.snapshot()
		if len(seen) != 1 {
			t.Fatalf("one read must make one call: %d", len(seen))
		}
		want := floorCall{path: "/api/v1/internal/spaces/sp-1/browser-floor", orgID: "org-1", serviceID: "org-core", token: floorTestToken}
		if seen[0] != want {
			t.Fatalf("the read carries %+v, want %+v", seen[0], want)
		}
	}
}

func TestUserCoreFloorReaderAddressesTheSpaceAndOrganisationExactly(t *testing.T) {
	const spaceID = "sp_1-a:b"
	const orgID = "org 1&x=y"
	server, calls := newFakeUserCore(t, answerWith(http.StatusOK, entitledBody(spaceID, true)))
	reader := NewUserCoreFloorReader(server.URL+"/", floorTestToken, nil)
	if got, err := reader.BrowserFloor(context.Background(), orgID, spaceID); err != nil || !got {
		t.Fatalf("BrowserFloor = (%v, %v), want (true, nil)", got, err)
	}
	seen := calls.snapshot()
	if len(seen) != 1 || seen[0].path != "/api/v1/internal/spaces/sp_1-a:b/browser-floor" || seen[0].orgID != orgID {
		t.Fatalf("the read addressed %+v", seen)
	}
}

func TestUserCoreFloorReaderRefusesEveryUnreadableAnswer(t *testing.T) {
	overlong := `{"data":{"space_ref":"sp-1","browser_entitled":true},"pad":"` + strings.Repeat("x", spaceFloorBodyLimit) + `"}`
	cases := map[string]struct {
		status int
		body   string
	}{
		"service unavailable": {http.StatusServiceUnavailable, entitledBody("sp-1", true)},
		"not found":           {http.StatusNotFound, entitledBody("sp-1", true)},
		"redirect status":     {http.StatusFound, entitledBody("sp-1", true)},
		"not json":            {http.StatusOK, "<html>not json</html>"},
		"empty body":          {http.StatusOK, ""},
		"null":                {http.StatusOK, "null"},
		"no data":             {http.StatusOK, `{"browser_entitled":true}`},
		"unknown field":       {http.StatusOK, `{"data":{"space_ref":"sp-1","browser_entitled":true,"extra":1}}`},
		"trailing data":       {http.StatusOK, entitledBody("sp-1", true) + `{}`},
		"no entitlement":      {http.StatusOK, `{"data":{"space_ref":"sp-1"}}`},
		"string entitlement":  {http.StatusOK, `{"data":{"space_ref":"sp-1","browser_entitled":"true"}}`},
		"another space":       {http.StatusOK, entitledBody("sp-2", true)},
		"over the size limit": {http.StatusOK, overlong},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			server, _ := newFakeUserCore(t, answerWith(tc.status, tc.body))
			reader := NewUserCoreFloorReader(server.URL, floorTestToken, nil)
			got, err := reader.BrowserFloor(context.Background(), "org-1", "sp-1")
			if !errors.Is(err, ErrFloorUnavailable) || got {
				t.Fatalf("want (false, ErrFloorUnavailable), got (%v, %v)", got, err)
			}
		})
	}
}

func TestUserCoreFloorReaderRefusesAnUnreachableOrCancelledUserCore(t *testing.T) {
	down := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {}))
	address := down.URL
	down.Close()
	if got, err := NewUserCoreFloorReader(address, floorTestToken, nil).BrowserFloor(context.Background(), "org-1", "sp-1"); !errors.Is(err, ErrFloorUnavailable) || got {
		t.Fatalf("an unreachable user-core must refuse: (%v, %v)", got, err)
	}

	up, calls := newFakeUserCore(t, answerWith(http.StatusOK, entitledBody("sp-1", true)))
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if got, err := NewUserCoreFloorReader(up.URL, floorTestToken, nil).BrowserFloor(ctx, "org-1", "sp-1"); !errors.Is(err, ErrFloorUnavailable) || got {
		t.Fatalf("a cancelled read must refuse: (%v, %v)", got, err)
	}
	if n := len(calls.snapshot()); n != 0 {
		t.Fatalf("a cancelled read must not reach user-core: %d calls", n)
	}
}

func TestUserCoreFloorReaderNeverFollowsARedirect(t *testing.T) {
	target, targetCalls := newFakeUserCore(t, answerWith(http.StatusOK, entitledBody("sp-1", true)))
	front := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL+r.URL.RequestURI(), http.StatusFound)
	}))
	t.Cleanup(front.Close)

	// The caller's own client follows redirects; the reader must still refuse to.
	following := &http.Client{}
	for name, client := range map[string]*http.Client{"default client": nil, "following client": following} {
		reader := NewUserCoreFloorReader(front.URL, floorTestToken, client)
		got, err := reader.BrowserFloor(context.Background(), "org-1", "sp-1")
		if !errors.Is(err, ErrFloorUnavailable) || got {
			t.Fatalf("%s: a redirect must refuse, got (%v, %v)", name, got, err)
		}
	}
	if n := len(targetCalls.snapshot()); n != 0 {
		t.Fatalf("the redirect target must never be read: %d calls", n)
	}
}

func TestNewUserCoreFloorReaderIsNilWithoutAURLOrAToken(t *testing.T) {
	for name, pair := range map[string][2]string{
		"no url":      {"", floorTestToken},
		"blank url":   {"  ", floorTestToken},
		"no token":    {"http://user-core:3010", ""},
		"blank token": {"http://user-core:3010", " \t "},
	} {
		if reader := NewUserCoreFloorReader(pair[0], pair[1], nil); reader != nil {
			t.Errorf("%s: a reader without its URL or token must be nil, got %T", name, reader)
		}
	}
	if NewUserCoreFloorReader("http://user-core:3010", floorTestToken, nil) == nil {
		t.Fatal("a URL and a token make a reader")
	}
}

func TestUserCoreFloorReaderWaitsAtMostTwoSecondsAndNeverChangesTheCallersClient(t *testing.T) {
	caller := &http.Client{Timeout: time.Minute}
	reader, ok := NewUserCoreFloorReader("http://user-core:3010", floorTestToken, caller).(*userCoreFloorReader)
	if !ok {
		t.Fatal("the reader must be the user-core reader")
	}
	if reader.client.Timeout != spaceFloorTimeout || reader.client.CheckRedirect == nil {
		t.Fatalf("a long caller timeout must be clamped and redirects refused: timeout=%v", reader.client.Timeout)
	}
	if caller.Timeout != time.Minute || caller.CheckRedirect != nil {
		t.Fatalf("the caller's client must not change: timeout=%v", caller.Timeout)
	}
	shorter, ok := NewUserCoreFloorReader("http://user-core:3010", floorTestToken, &http.Client{Timeout: time.Second}).(*userCoreFloorReader)
	if !ok || shorter.client.Timeout != time.Second {
		t.Fatalf("a shorter caller timeout is kept: %+v", shorter)
	}
}

// The reader keeps no answer: a Space whose entitlement is withdrawn is refused on the next read.
func TestUserCoreFloorReaderKeepsNoAnswer(t *testing.T) {
	var mu sync.Mutex
	entitled := true
	server, calls := newFakeUserCore(t, func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		current := entitled
		mu.Unlock()
		answerWith(http.StatusOK, entitledBody("sp-1", current))(w, r)
	})
	reader := NewUserCoreFloorReader(server.URL, floorTestToken, nil)
	if got, err := reader.BrowserFloor(context.Background(), "org-1", "sp-1"); err != nil || !got {
		t.Fatalf("first read = (%v, %v), want (true, nil)", got, err)
	}
	mu.Lock()
	entitled = false
	mu.Unlock()
	if got, err := reader.BrowserFloor(context.Background(), "org-1", "sp-1"); err != nil || got {
		t.Fatalf("second read = (%v, %v), want (false, nil)", got, err)
	}
	if n := len(calls.snapshot()); n != 2 {
		t.Fatalf("every read must reach user-core: %d calls", n)
	}
}

// A refused answer is logged by organisation and status only: never the body, never the token.
func TestUserCoreFloorReaderLogsTheOrganisationAndStatusOnly(t *testing.T) {
	const marker = "fixture-answer-marker-zzzzzzzz"
	var logs bytes.Buffer
	previous := log.Writer()
	log.SetOutput(&logs)
	t.Cleanup(func() { log.SetOutput(previous) })

	server, _ := newFakeUserCore(t, answerWith(http.StatusOK, `{"data":{"space_ref":"sp-1","browser_entitled":"`+marker+`"}}`))
	reader := NewUserCoreFloorReader(server.URL, floorTestToken, nil)
	if _, err := reader.BrowserFloor(context.Background(), "org-1", "sp-1"); !errors.Is(err, ErrFloorUnavailable) {
		t.Fatalf("an unreadable answer must refuse: %v", err)
	}
	out := logs.String()
	if !strings.Contains(out, "org=org-1") || !strings.Contains(out, "status=200") {
		t.Fatalf("the log must name the organisation and the status: %q", out)
	}
	if strings.Contains(out, marker) || strings.Contains(out, floorTestToken) {
		t.Fatalf("the log must not carry the body or the token: %q", out)
	}
}
