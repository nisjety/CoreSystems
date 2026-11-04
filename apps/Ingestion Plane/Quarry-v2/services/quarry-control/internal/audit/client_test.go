package audit

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

const testToken = "0123456789abcdef0123456789abcdef"

func env(values map[string]string) func(string) string {
	return func(key string) string { return values[key] }
}

func validEvent() Event {
	return Event{
		EventID:    "quarry-control:crawl.robots_ignored:job_1",
		OccurredAt: time.Date(2026, 11, 3, 10, 0, 0, 0, time.FixedZone("x", 3600)),
		OrgID:      "org_a",
		UserID:     "user_a",
		Event:      "crawl.robots_ignored",
		ResourceID: "job_1",
		Details:    map[string]any{"seed_host": "example.com"},
	}
}

func TestRecordAuditPostsAsTheQuarryControlProducerOnIngestion(t *testing.T) {
	t.Parallel()
	var got map[string]any
	var headers http.Header
	var path, method string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		method, path, headers = r.Method, r.URL.Path, r.Header.Clone()
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &got)
		w.WriteHeader(http.StatusAccepted)
	}))
	defer srv.Close()

	c, err := New(srv.URL+"/", testToken, srv.Client())
	if err != nil {
		t.Fatal(err)
	}
	ev := validEvent()
	ev.Plane = "control"   // overwritten
	ev.Producer = "forged" // overwritten
	if err := c.RecordAudit(context.Background(), ev); err != nil {
		t.Fatalf("RecordAudit: %v", err)
	}
	if method != http.MethodPost || path != "/v1/audit" {
		t.Fatalf("request = %s %s, want POST /v1/audit", method, path)
	}
	if headers.Get("X-Service-Id") != "quarry-control" || headers.Get("X-Service-Token") != testToken {
		t.Fatal("service principal headers missing or wrong")
	}
	if headers.Get("Authorization") != "" {
		t.Fatal("the audit write must not carry a bearer header")
	}
	if got["producer"] != "quarry-control" || got["plane"] != "ingestion" {
		t.Fatalf("producer=%v plane=%v, want quarry-control/ingestion", got["producer"], got["plane"])
	}
	if got["outcome"] != "ok" || got["event"] != "crawl.robots_ignored" || got["resource_id"] != "job_1" || got["user_id"] != "user_a" {
		t.Fatalf("body = %v", got)
	}
	if ts, _ := got["occurred_at"].(string); !strings.HasSuffix(ts, "Z") {
		t.Fatalf("occurred_at = %q, want UTC", ts)
	}
}

func TestRecordAuditTreatsDuplicateAsSuccessAndEveryNon2xxAsFailure(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct {
		status int
		ok     bool
	}{
		{http.StatusOK, true},
		{http.StatusAccepted, true},
		{http.StatusBadRequest, false},
		{http.StatusUnauthorized, false},
		{http.StatusForbidden, false},
		{http.StatusConflict, false},
		{http.StatusInternalServerError, false},
		{http.StatusFound, false},
	} {
		srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if tc.status == http.StatusFound {
				http.Redirect(w, r, "/elsewhere", http.StatusFound)
				return
			}
			w.WriteHeader(tc.status)
		}))
		c, err := New(srv.URL, testToken, srv.Client())
		if err != nil {
			t.Fatal(err)
		}
		err = c.RecordAudit(context.Background(), validEvent())
		srv.Close()
		if (err == nil) != tc.ok {
			t.Fatalf("status %d: err=%v, want ok=%v", tc.status, err, tc.ok)
		}
	}
}

func TestRecordAuditFailsOnTransportError(t *testing.T) {
	t.Parallel()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {}))
	url := srv.URL
	srv.Close()
	c, err := New(url, testToken, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := c.RecordAudit(context.Background(), validEvent()); err == nil {
		t.Fatal("a closed audit-core must be an error")
	}
}

func TestRecordAuditRejectsIncompleteEventsWithoutCalling(t *testing.T) {
	t.Parallel()
	called := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		called = true
		w.WriteHeader(http.StatusAccepted)
	}))
	defer srv.Close()
	c, err := New(srv.URL, testToken, srv.Client())
	if err != nil {
		t.Fatal(err)
	}
	for name, mutate := range map[string]func(*Event){
		"no event_id":   func(e *Event) { e.EventID = "" },
		"long event_id": func(e *Event) { e.EventID = strings.Repeat("a", 129) },
		"no time":       func(e *Event) { e.OccurredAt = time.Time{} },
		"no org":        func(e *Event) { e.OrgID = " " },
		"no event":      func(e *Event) { e.Event = "" },
	} {
		ev := validEvent()
		mutate(&ev)
		if err := c.RecordAudit(context.Background(), ev); err == nil {
			t.Fatalf("%s: accepted", name)
		}
	}
	if called {
		t.Fatal("an incomplete event reached audit-core")
	}
}

func TestNilOrUnconfiguredClientRefuses(t *testing.T) {
	t.Parallel()
	var c *Client
	if c.Configured() {
		t.Fatal("nil client reports configured")
	}
	if err := c.RecordAudit(context.Background(), validEvent()); !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("nil client err = %v", err)
	}
}

func TestNewValidatesURLAndToken(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct{ url, token string }{
		{"", testToken},
		{"http://audit-core:8080", ""},
		{"audit-core:8080", testToken},
		{"ftp://audit-core", testToken},
		{"http://", testToken},
	} {
		if _, err := New(tc.url, tc.token, nil); err == nil {
			t.Fatalf("New(%q, token=%v) accepted", tc.url, tc.token != "")
		}
	}
	if _, err := New("http://audit-core:8080", testToken, nil); err != nil {
		t.Fatalf("valid config rejected: %v", err)
	}
}

func TestFromEnv(t *testing.T) {
	t.Parallel()
	c, err := FromEnv(env(nil), nil)
	if err != nil || c != nil {
		t.Fatalf("both unset: client=%v err=%v, want nil, nil", c, err)
	}
	if _, err := FromEnv(env(map[string]string{EnvURL: "http://audit-core:8080"}), nil); err == nil {
		t.Fatal("URL without token accepted")
	}
	_, err = FromEnv(env(map[string]string{EnvToken: testToken}), nil)
	if err == nil {
		t.Fatal("token without URL accepted")
	}
	if strings.Contains(err.Error(), testToken) {
		t.Fatal("error message leaks the token")
	}
	c, err = FromEnv(env(map[string]string{EnvURL: " http://audit-core:8080/ ", EnvToken: testToken}), nil)
	if err != nil || !c.Configured() {
		t.Fatalf("configured: client=%v err=%v", c, err)
	}
}
