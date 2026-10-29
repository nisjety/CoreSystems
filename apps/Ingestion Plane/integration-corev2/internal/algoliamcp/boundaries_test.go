package algoliamcp

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/triodelab/integration-corev2/internal/sitesources"
	"github.com/triodelab/integration-corev2/internal/store"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

func responseSession(body string, status int, content string) *session {
	return &session{valid: func() bool { return true }, http: &http.Client{Transport: fixtureTransport(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: status, Header: http.Header{"Content-Type": []string{content}}, Body: io.NopCloser(strings.NewReader(body))}, nil
	})}}
}

type rejectingTokens struct {
	fixtureTokens
	repo  *store.MemoryRepository
	calls int
}

func (f *rejectingTokens) InvalidateAlgoliaAccessToken(ctx context.Context, org, id, token string) error {
	f.calls++
	c, _ := f.repo.GetConnection(ctx, id)
	if c.OrganizationID == org && token == "protected-token" {
		c.Status = "needs_refresh"
		_, err := f.repo.UpsertConnection(ctx, c)
		return err
	}
	return nil
}
func TestObserved401InvalidatesGrantAndStopsFurtherRPC(t *testing.T) {
	s, repo, _ := fixtureService(t)
	tokens := &rejectingTokens{repo: repo}
	s.tokens = tokens
	calls := 0
	s.http = &http.Client{Transport: fixtureTransport(func(*http.Request) (*http.Response, error) {
		calls++
		return &http.Response{StatusCode: 401, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(`{"error":"revoked"}`))}, nil
	})}
	if _, err := s.Applications(context.Background(), "org-1", "conn_one"); err == nil {
		t.Fatal("rejected MCP accepted")
	}
	a, _ := s.Access(context.Background(), "org-1", "conn_one")
	if tokens.calls != 1 || calls != 1 || a.Status != "reconnect_required" {
		t.Fatal("provider rejection not reflected in local access")
	}
}

func TestInitializedNotification401AlsoInvalidates(t *testing.T) {
	called := false
	s := responseSession(`{"error":"revoked"}`, 401, "application/json")
	s.unauthorized = func() { called = true }
	if _, err := s.rpc(context.Background(), "notifications/initialized", map[string]any{}); err == nil || !called {
		t.Fatal("notification unauthorized response did not invalidate")
	}
}
func TestSessionCancellationAndWindowRollover(t *testing.T) {
	s, _, _ := fixtureService(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	s.http = &http.Client{Transport: fixtureTransport(func(r *http.Request) (*http.Response, error) {
		if r.Context().Err() == nil {
			t.Fatal("cancellation missing")
		}
		return nil, r.Context().Err()
	})}
	if _, err := s.Applications(ctx, "org-1", "conn_one"); err == nil {
		t.Fatal("cancelled collection continued")
	}
	s, _, _ = fixtureService(t)
	now := time.Date(2026, 10, 23, 23, 59, 59, 0, time.UTC)
	s.now = func() time.Time { return now }
	original := s.http.Transport
	s.http.Transport = fixtureTransport(func(r *http.Request) (*http.Response, error) {
		raw, _ := io.ReadAll(r.Body)
		r.Body = io.NopCloser(bytes.NewReader(raw))
		res, err := original.RoundTrip(r)
		if strings.Contains(string(raw), noResults) && strings.Contains(string(raw), `"method":"tools/call"`) {
			now = now.Add(2 * time.Second)
		}
		return res, err
	})
	_, err := s.Report(context.Background(), "org-1", ReportInput{WebsiteID: "web_12345678901234567890123456789012", Site: "https://triodelab.no/", WebsiteRevision: 1, ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: "public_index"})
	if err == nil {
		t.Fatal("cross-midnight receipt accepted")
	}
}
func TestRPCRejectsUnsafeResponses(t *testing.T) {
	for _, tc := range []struct {
		name, body, content string
		status              int
	}{
		{"wrong-id", `{"jsonrpc":"2.0","id":2,"result":{}}`, "application/json", 200},
		{"duplicate", `{"jsonrpc":"2.0","id":1,"id":1,"result":{}}`, "application/json", 200},
		{"error", `{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"secret upstream"}}`, "application/json", 200},
		{"redirect", "", "application/json", 302},
		{"bad-type", "{}", "text/html", 200},
		{"no-type", "{}", "", 200},
		{"oversize", strings.Repeat(" ", maxResponse+1), "application/json", 200},
		{"null", `{"jsonrpc":"2.0","id":1,"result":null}`, "application/json", 200},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := responseSession(tc.body, tc.status, tc.content)
			if _, err := s.rpc(context.Background(), "tools/list", map[string]any{}); err == nil {
				t.Fatal("unsafe response accepted")
			}
		})
	}
	s := responseSession(`{"jsonrpc":"2.0","id":1,"result":{}}`, 200, "application/json")
	s.valid = func() bool { return false }
	if _, err := s.rpc(context.Background(), "tools/list", nil); err == nil {
		t.Fatal("revoked grant used")
	}
	s = responseSession("", 200, "application/json")
	s.http.Transport = fixtureTransport(func(*http.Request) (*http.Response, error) { return nil, errors.New("protected token URL") })
	if _, err := s.rpc(context.Background(), "tools/list", nil); err != ErrUnavailable {
		t.Fatal("raw network error exposed")
	}
}
func TestSSEIsBoundedAndSingleResponse(t *testing.T) {
	good := `{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}`
	s := responseSession("event: message\r\ndata: "+good+"\r\n\r\n", 200, "text/event-stream")
	if _, err := s.rpc(context.Background(), "tools/list", nil); err != nil {
		t.Fatal(err)
	}
	for _, bad := range []string{"event: message\n\n", strings.Repeat("\n\n", 33), "data: " + good + "\n\ndata: " + good + "\n\n"} {
		if _, err := sseResult([]byte(bad)); err == nil {
			t.Fatal("invalid SSE accepted")
		}
	}
}
func TestSchemaDrivenArgumentsFailClosed(t *testing.T) {
	w := sitesources.Window{StartDate: "2026-10-16", EndDate: "2026-10-22"}
	var base tool
	_ = json.Unmarshal([]byte(`{"name":"algolia_analytics_no_results_rate","inputSchema":{"type":"object","properties":{"applicationId":{"type":"string"},"index":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":["applicationId","index","startDate","endDate"]}}`), &base)
	base.RawSchema = json.RawMessage(`{"type":"object","properties":{"applicationId":{"type":"string"},"index":{"type":"string"},"startDate":{"type":"string"},"endDate":{"type":"string"}},"required":["applicationId","index","startDate","endDate"]}`)
	args, err := analyticsArgs(base, "APP1234567", "public_index", w)
	if err != nil || args["index"] != "public_index" {
		t.Fatal("actual schema alias not selected")
	}
	for _, mutate := range []func(*tool){
		func(t *tool) { t.InputSchema.Required = append(t.InputSchema.Required, "secret") },
		func(t *tool) { t.InputSchema.Properties["indexName"] = property{Type: "string"} },
		func(t *tool) { t.InputSchema.Properties["startDate"] = property{Type: "integer"} },
		func(t *tool) {
			t.InputSchema.Properties["applicationId"] = property{Type: "string", Enum: []string{"DIFFERENT1"}}
		},
		func(t *tool) { delete(t.InputSchema.Properties, "endDate") },
	} {
		copy := base
		copy.InputSchema.Properties = map[string]property{}
		for k, v := range base.InputSchema.Properties {
			copy.InputSchema.Properties[k] = v
		}
		copy.InputSchema.Required = append([]string{}, base.InputSchema.Required...)
		mutate(&copy)
		if _, err := analyticsArgs(copy, "APP1234567", "public_index", w); err == nil {
			t.Fatal("unsupported schema dispatched")
		}
	}
	if _, err := applications(map[string]tool{}); err == nil {
		t.Fatal("missing enum accepted")
	}
	if knownRequired(schema{Required: []string{"applicationId", "applicationId"}}, map[string]bool{"applicationId": true}) {
		t.Fatal("duplicate requirements accepted")
	}
}
func TestMalformedToolsAndResultPayloads(t *testing.T) {
	for _, raw := range []string{`{"tools":[]}`, `{"tools":[{"name":"algolia_search_list_indices","inputSchema":{"type":"array"}}]}`, `{"tools":[{"name":"algolia_search_list_indices"},{"name":"algolia_search_list_indices"}]}`, `{"tools":[],"nextCursor":"secret"}`} {
		s := responseSession(`{"jsonrpc":"2.0","id":1,"result":`+raw+`}`, 200, "application/json")
		if _, err := s.tools(context.Background()); err == nil {
			t.Fatal("malformed toolset accepted")
		}
	}
	for _, raw := range []string{`{"isError":true,"content":[]}`, `{"content":[{"type":"image","text":"{}"}]}`, `{"content":[{"type":"text","text":"not-json"}]}`} {
		s := responseSession(`{"jsonrpc":"2.0","id":1,"result":`+raw+`}`, 200, "application/json")
		if _, err := s.call(context.Background(), noResults, nil); err == nil {
			t.Fatal("unsafe tool result accepted")
		}
	}
	s := responseSession(`{"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"count":0}}}`, 200, "application/json")
	if _, err := s.call(context.Background(), noResults, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := s.call(context.Background(), "delete_index", nil); err == nil {
		t.Fatal("arbitrary tool permitted")
	}
}
func TestGrantRevocationCapacityAndResourceValidation(t *testing.T) {
	s, repo, _ := fixtureService(t)
	ctx := context.Background()
	for _, index := range []string{"public_index", "missing"} {
		r, err := s.Resources(ctx, "org-1", ResourceInput{ConnectionID: "conn_one", ApplicationID: "APP1234567", IndexName: index})
		if err != nil || (r.Status == "allowed") != (index == "public_index") {
			t.Fatalf("resource %+v %v", r, err)
		}
	}
	if _, err := s.Resources(ctx, "org-1", ResourceInput{IndexName: "private/escape"}); err == nil {
		t.Fatal("invalid index accepted")
	}
	if _, err := s.Indices(ctx, "org-1", "conn_one", "bad app"); err == nil {
		t.Fatal("invalid app accepted")
	}
	s.slots <- struct{}{}
	s.slots <- struct{}{}
	if _, err := s.Applications(ctx, "org-1", "conn_one"); err != ErrBusy {
		t.Fatal("global cap missing")
	}
	<-s.slots
	<-s.slots
	c, _ := repo.GetConnection(ctx, "conn_one")
	c.AccessTokenExpiresAt = time.Date(2026, 10, 20, 0, 0, 0, 0, time.UTC)
	_, _ = repo.UpsertConnection(ctx, c)
	a, _ := s.Access(ctx, "org-1", "conn_one")
	if a.Status != "reconnect_required" {
		t.Fatal("expired nonrefreshable grant active")
	}
	if _, err := s.Applications(ctx, "org-1", "conn_one"); err != ErrAccess {
		t.Fatal("inactive grant called upstream")
	}
	if _, err := s.Access(ctx, "org-1", "bad/id"); err == nil {
		t.Fatal("bad id accepted")
	}
	_ = New(repo, fixtureTokens{}, nil, nil)
}
