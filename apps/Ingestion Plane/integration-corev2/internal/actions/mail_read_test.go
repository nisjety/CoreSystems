package actions

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	neturl "net/url"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

// F2: mail read for the connected user's own Outlook and Gmail mailbox. These
// tests pin the provider-facing request each parameter combination produces,
// so a Graph or Gmail query is never assembled from unvalidated text.

type mailReadCall struct {
	Method   string
	Path     string
	RawQuery string
	Query    neturl.Values
	Header   http.Header
}

type mailReadRecorder struct {
	mu    sync.Mutex
	calls []mailReadCall
}

func (r *mailReadRecorder) record(req *http.Request) mailReadCall {
	call := mailReadCall{
		Method:   req.Method,
		Path:     req.URL.Path,
		RawQuery: req.URL.RawQuery,
		Query:    req.URL.Query(),
		Header:   req.Header.Clone(),
	}
	r.mu.Lock()
	r.calls = append(r.calls, call)
	r.mu.Unlock()
	return call
}

func (r *mailReadRecorder) reset() {
	r.mu.Lock()
	r.calls = nil
	r.mu.Unlock()
}

func (r *mailReadRecorder) all() []mailReadCall {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]mailReadCall(nil), r.calls...)
}

func (r *mailReadRecorder) only(t *testing.T) mailReadCall {
	t.Helper()
	calls := r.all()
	if len(calls) != 1 {
		t.Fatalf("provider received %d requests, want exactly 1: %+v", len(calls), calls)
	}
	return calls[0]
}

func mailReadJSON(w http.ResponseWriter, value any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(value)
}

// newGraphMailTestService serves Graph over TLS: the page-token validator only
// accepts https links on the configured Graph host, so a plain-http test
// server could never exercise the paging path.
func newGraphMailTestService(t *testing.T, handler http.HandlerFunc) (*Service, *httptest.Server) {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	return NewService(config.Config{MicrosoftGraphBaseURL: server.URL}, server.Client()), server
}

func newGmailTestService(t *testing.T, handler http.HandlerFunc) *Service {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	return NewService(config.Config{GoogleAPIBaseURL: server.URL}, server.Client())
}

func executeMailRead(service *Service, provider, operation string, params map[string]any) (map[string]any, error) {
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: provider}, AccessToken: "token-1",
		Operation: operation, Params: params,
	})
	if err != nil {
		return nil, err
	}
	payload, _ := result.Result.(map[string]any)
	return payload, nil
}

func mustExecuteMailRead(t *testing.T, service *Service, provider, operation string, params map[string]any) map[string]any {
	t.Helper()
	payload, err := executeMailRead(service, provider, operation, params)
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	return payload
}

// --- Outlook: list ------------------------------------------------------------

func TestMailReadOutlookSearchComposesKQLAndNeverCombinesFilterOrOrderby(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{
		"query": "invoice", "from": "bob@contoso.com", "since": "2026-01-05", "until": "2026-01-31",
		"folder": "inbox", "maxResults": float64(7),
	})
	call := rec.only(t)
	if call.Path != "/v1.0/me/mailFolders/inbox/messages" {
		t.Fatalf("path = %q, want the inbox folder's messages", call.Path)
	}
	want := `"invoice from:bob@contoso.com received>=2026-01-05 received<=2026-01-31"`
	if got := call.Query.Get("$search"); got != want {
		t.Fatalf("$search = %q, want %q", got, want)
	}
	for _, forbidden := range []string{"$filter", "$orderby"} {
		if _, present := call.Query[forbidden]; present {
			t.Fatalf("%s must not be combined with $search on messages, query = %s", forbidden, call.RawQuery)
		}
	}
	if got := call.Query.Get("$top"); got != "7" {
		t.Fatalf("$top = %q, want 7", got)
	}
	selected := call.Query.Get("$select")
	for _, field := range []string{"id", "subject", "from", "receivedDateTime", "webLink", "bodyPreview", "hasAttachments", "isRead", "conversationId", "parentFolderId"} {
		if !strings.Contains(","+selected+",", ","+field+",") {
			t.Fatalf("$select = %q is missing %s", selected, field)
		}
	}
	if got := call.Header.Get("Authorization"); got != "Bearer token-1" {
		t.Fatalf("Authorization = %q", got)
	}
}

func TestMailReadOutlookSearchEscapesQuoteBackslashAndControlCharacters(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	// A quote must not close the $search phrase early, a backslash must not
	// swallow the escape in front of the next quote, and a newline, tab or NUL
	// must not survive into the provider query.
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{
		"query": "\"a\"b\\c\nd\te\x00f g",
	})
	call := rec.only(t)
	want := `"\"a\"b\\c d e f g"`
	if got := call.Query.Get("$search"); got != want {
		t.Fatalf("$search = %q, want %q", got, want)
	}
	if strings.ContainsAny(call.RawQuery, "\r\n") || strings.Contains(call.RawQuery, "%00") || strings.Contains(strings.ToLower(call.RawQuery), "%e2%80%a8") {
		t.Fatalf("control characters reached the provider: %s", call.RawQuery)
	}
}

func TestMailReadOutlookSearchGroupsAnORQueryBeforeAddingFilters(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{
		"query": "budget OR forecast", "from": "cfo@contoso.com",
	})
	want := `"(budget OR forecast) from:cfo@contoso.com"`
	if got := rec.only(t).Query.Get("$search"); got != want {
		t.Fatalf("$search = %q, want %q so the sender restriction applies to both alternatives", got, want)
	}
}

func TestMailReadOutlookFromCannotInjectSearchSyntax(t *testing.T) {
	cases := []struct {
		name string
		from string
		want string
	}{
		{name: "plain address stays bare", from: "bob@contoso.com", want: `"from:bob@contoso.com"`},
		{name: "display name becomes one quoted phrase", from: "John Doe", want: `"from:\"John Doe\""`},
		{name: "operator smuggling becomes phrase text", from: `bob@x.com OR subject:salary`, want: `"from:\"bob@x.com OR subject:salary\""`},
		{name: "embedded quote cannot close the phrase", from: `x" OR to:ceo@corp.com "y`, want: `"from:\"x OR to:ceo@corp.com y\""`},
		{name: "backslash and grouping characters are dropped", from: `a\") OR (b`, want: `"from:\"a OR b\""`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var rec mailReadRecorder
			service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				mailReadJSON(w, map[string]any{"value": []any{}})
			})
			mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"from": tc.from})
			if got := rec.only(t).Query.Get("$search"); got != tc.want {
				t.Fatalf("$search = %q, want %q", got, tc.want)
			}
		})
	}
}

func TestMailReadOutlookFilterPathOrdersByReceivedDate(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{
		"since": "2026-01-05", "until": "2026-01-31", "unread": true,
	})
	call := rec.only(t)
	if call.Path != "/v1.0/me/messages" {
		t.Fatalf("path = %q, want the all-folders messages collection", call.Path)
	}
	// until is inclusive of its whole day, so the bound is the next midnight.
	wantFilter := "receivedDateTime ge 2026-01-05T00:00:00Z and receivedDateTime lt 2026-02-01T00:00:00Z and isRead eq false"
	if got := call.Query.Get("$filter"); got != wantFilter {
		t.Fatalf("$filter = %q, want %q", got, wantFilter)
	}
	if got := call.Query.Get("$orderby"); got != "receivedDateTime desc" {
		t.Fatalf("$orderby = %q, want receivedDateTime desc", got)
	}
	if _, present := call.Query["$search"]; present {
		t.Fatalf("$search must not be used on the filter path, query = %s", call.RawQuery)
	}
	if strings.Contains(call.RawQuery, "+") || !strings.Contains(call.RawQuery, "%20and%20") {
		t.Fatalf("raw query = %s, want spaces percent-encoded as %%20 and no bare +", call.RawQuery)
	}
}

func TestMailReadOutlookUnreadOnlyGetsAFloorDateSoGraphDoesNotRejectTheOrdering(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"unread": "true"})
	call := rec.only(t)
	// The $orderby property must lead the $filter, or Graph answers
	// InefficientFilter.
	wantFilter := "receivedDateTime ge 1900-01-01T00:00:00Z and isRead eq false"
	if got := call.Query.Get("$filter"); got != wantFilter {
		t.Fatalf("$filter = %q, want %q", got, wantFilter)
	}
	if got := call.Query.Get("$orderby"); got != "receivedDateTime desc" {
		t.Fatalf("$orderby = %q", got)
	}
}

func TestMailReadOutlookUntilAloneLeadsTheFilterWithTheOrderedProperty(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"until": "2026-12-31", "unread": true})
	wantFilter := "receivedDateTime lt 2027-01-01T00:00:00Z and isRead eq false"
	if got := rec.only(t).Query.Get("$filter"); got != wantFilter {
		t.Fatalf("$filter = %q, want %q", got, wantFilter)
	}
}

func TestMailReadOutlookWithoutParamsOnlyOrdersAndKeepsThePickerDefaults(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{map[string]any{"id": "m1"}}})
	})
	payload := mustExecuteMailRead(t, service, "microsoft", "mail.messages", nil)
	call := rec.only(t)
	if call.Path != "/v1.0/me/messages" || call.Query.Get("$top") != "10" {
		t.Fatalf("path %q $top %q, want /v1.0/me/messages and the default 10", call.Path, call.Query.Get("$top"))
	}
	if _, present := call.Query["$filter"]; present {
		t.Fatalf("no $filter is needed without a date or unread bound: %s", call.RawQuery)
	}
	if call.Query.Get("$orderby") != "receivedDateTime desc" {
		t.Fatalf("$orderby = %q", call.Query.Get("$orderby"))
	}
	if rows, _ := payload["value"].([]any); len(rows) != 1 {
		t.Fatalf("value = %#v, want the Graph rows passed through", payload["value"])
	}
	if _, present := payload["nextPageToken"]; present {
		t.Fatalf("nextPageToken present without a nextLink: %#v", payload)
	}

	// The picker's own cap of 100 still holds, and nothing above it passes.
	rec.reset()
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"maxResults": float64(500)})
	if got := rec.only(t).Query.Get("$top"); got != "100" {
		t.Fatalf("$top = %q, want the picker's 100 cap", got)
	}
}

func TestMailReadOutlookFolderMapsToWellKnownNames(t *testing.T) {
	cases := map[string]string{
		"inbox": "inbox", "sent": "sentitems", "drafts": "drafts", "archive": "archive", "deleted": "deleteditems",
		"  Inbox ": "inbox", "SENT": "sentitems", "trash": "deleteditems", "draft": "drafts",
	}
	for folder, wantSegment := range cases {
		t.Run(folder, func(t *testing.T) {
			var rec mailReadRecorder
			service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				mailReadJSON(w, map[string]any{"value": []any{}})
			})
			mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"folder": folder})
			if want := "/v1.0/me/mailFolders/" + wantSegment + "/messages"; rec.only(t).Path != want {
				t.Fatalf("path = %q, want %q", rec.only(t).Path, want)
			}
		})
	}
}

func TestMailReadRejectsInvalidParametersBeforeAnyProviderCall(t *testing.T) {
	cases := []struct {
		name   string
		params map[string]any
		want   string
	}{
		{name: "unknown folder", params: map[string]any{"folder": "inbox&labelIds=TRASH"}, want: "folder"},
		{name: "folder that is a path", params: map[string]any{"folder": "../drive"}, want: "folder"},
		{name: "since without zero padding", params: map[string]any{"since": "2026-1-5"}, want: "since"},
		{name: "since in words", params: map[string]any{"since": "yesterday"}, want: "since"},
		{name: "since with a time", params: map[string]any{"since": "2026-01-05T00:00:00Z"}, want: "since"},
		{name: "impossible date", params: map[string]any{"until": "2026-02-30"}, want: "until"},
		{name: "date as a number", params: map[string]any{"since": float64(20260105)}, want: "since"},
		{name: "range reversed", params: map[string]any{"since": "2026-03-01", "until": "2026-02-01"}, want: "since"},
		{name: "unread that is not a boolean", params: map[string]any{"unread": "maybe"}, want: "unread"},
		{name: "query that is not text", params: map[string]any{"query": map[string]any{"$ne": ""}}, want: "query"},
		{name: "query too long", params: map[string]any{"query": strings.Repeat("a", 501)}, want: "query"},
		{name: "from with nothing usable in it", params: map[string]any{"from": `"\"`}, want: "from"},
		{name: "page token with a control character", params: map[string]any{"pageToken": "abc\ndef"}, want: "pageToken"},
		{name: "page token too long", params: map[string]any{"pageToken": strings.Repeat("a", 4097)}, want: "pageToken"},
	}
	for _, provider := range []struct{ key, operation string }{
		{"microsoft", "mail.messages"},
		{"google", "gmail.messages"},
	} {
		for _, tc := range cases {
			t.Run(provider.key+"/"+tc.name, func(t *testing.T) {
				var hits atomic.Int32
				handler := func(w http.ResponseWriter, r *http.Request) {
					hits.Add(1)
					mailReadJSON(w, map[string]any{})
				}
				var service *Service
				if provider.key == "microsoft" {
					service, _ = newGraphMailTestService(t, handler)
				} else {
					service = newGmailTestService(t, handler)
				}
				_, err := executeMailRead(service, provider.key, provider.operation, tc.params)
				if err == nil {
					t.Fatalf("want an error for %v", tc.params)
				}
				if !strings.Contains(err.Error(), tc.want) {
					t.Fatalf("error = %q, want it to name %q", err, tc.want)
				}
				if hits.Load() != 0 {
					t.Fatalf("the provider was called %d times for an invalid request", hits.Load())
				}
			})
		}
	}
}

func TestMailReadOutlookHalfTypedQueriesAreNotSentAsSyntaxErrors(t *testing.T) {
	// The picker sends its search box on every debounced keystroke, so an
	// unterminated phrase or group is an ordinary input, not an attack.
	cases := []struct {
		name  string
		query string
		want  string
	}{
		{name: "unterminated phrase", query: `"quarterly rep`, want: `"quarterly rep"`},
		{name: "unclosed group", query: `invoice (final`, want: `"invoice final"`},
		{name: "closing before opening", query: `a) b (c`, want: `"a b c"`},
		{name: "paired phrase is kept", query: `"exact phrase" budget`, want: `"\"exact phrase\" budget"`},
		{name: "balanced group is kept", query: `(a OR b) c`, want: `"(a OR b) c"`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var rec mailReadRecorder
			service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				mailReadJSON(w, map[string]any{"value": []any{}})
			})
			mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"query": tc.query})
			if got := rec.only(t).Query.Get("$search"); got != tc.want {
				t.Fatalf("$search = %q, want %q", got, tc.want)
			}
		})
	}

	// A query with nothing left in it is no search at all: the list falls back
	// to the ordered, unfiltered path rather than sending an empty $search.
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"query": `"`, "since": "2026-01-05"})
	call := rec.only(t)
	if _, present := call.Query["$search"]; present {
		t.Fatalf("an empty query still sent $search: %s", call.RawQuery)
	}
	if got := call.Query.Get("$filter"); got != "receivedDateTime ge 2026-01-05T00:00:00Z" {
		t.Fatalf("$filter = %q", got)
	}
}

// --- Outlook: paging ----------------------------------------------------------

func TestMailReadOutlookNextLinkIsReturnedAsPageTokenAndRoundTripsUnchanged(t *testing.T) {
	var rec mailReadRecorder
	var serverURL string
	service, server := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Query().Get("$skiptoken") == "" {
			mailReadJSON(w, map[string]any{
				"value":           []any{map[string]any{"id": "m1"}},
				"@odata.nextLink": serverURL + "/v1.0/me/mailFolders('inbox')/messages?$top=10&$skiptoken=Page%202",
			})
			return
		}
		mailReadJSON(w, map[string]any{"value": []any{map[string]any{"id": "m2"}}})
	})
	serverURL = server.URL

	first := mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"folder": "inbox"})
	token, _ := first["nextPageToken"].(string)
	wantToken := server.URL + "/v1.0/me/mailFolders('inbox')/messages?$top=10&$skiptoken=Page%202"
	if token != wantToken {
		t.Fatalf("nextPageToken = %q, want the nextLink unchanged %q", token, wantToken)
	}
	if _, kept := first["value"]; !kept {
		t.Fatalf("value is dropped from %#v", first)
	}

	// The page request ignores every other param and reuses the link as is.
	second := mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{
		"pageToken": token, "query": "ignored", "maxResults": float64(3),
	})
	calls := rec.all()
	if len(calls) != 2 {
		t.Fatalf("requests = %d, want 2", len(calls))
	}
	if calls[1].Path != "/v1.0/me/mailFolders('inbox')/messages" || calls[1].RawQuery != "$top=10&$skiptoken=Page%202" {
		t.Fatalf("page request = %s?%s, want the nextLink replayed", calls[1].Path, calls[1].RawQuery)
	}
	if got := calls[1].Header.Get("Authorization"); got != "Bearer token-1" {
		t.Fatalf("page request Authorization = %q", got)
	}
	if rows, _ := second["value"].([]any); len(rows) != 1 {
		t.Fatalf("second page value = %#v", second["value"])
	}
	if _, present := second["nextPageToken"]; present {
		t.Fatalf("the last page must not carry a nextPageToken: %#v", second)
	}
}

func TestMailReadOutlookRejectsPageTokensThatCouldCarryTheBearerElsewhere(t *testing.T) {
	var hits atomic.Int32
	var foreignHits atomic.Int32
	foreign := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		foreignHits.Add(1)
		mailReadJSON(w, map[string]any{"value": []any{}})
	}))
	t.Cleanup(foreign.Close)
	service, server := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		mailReadJSON(w, map[string]any{"value": []any{}})
	})
	host := strings.TrimPrefix(server.URL, "https://")

	rejected := map[string]string{
		"plain http":                  "http://" + host + "/v1.0/me/messages?$skiptoken=x",
		"user info before the host":   "https://evil@" + host + "/v1.0/me/messages?$skiptoken=x",
		"another host entirely":       foreign.URL + "/v1.0/me/messages?$skiptoken=x",
		"a lookalike host":            "https://graph.microsoft.com.evil.example/v1.0/me/messages?$skiptoken=x",
		"scheme-relative":             "//" + host + "/v1.0/me/messages?$skiptoken=x",
		"no scheme":                   "/v1.0/me/messages?$skiptoken=x",
		"beta endpoint":               server.URL + "/beta/me/messages?$skiptoken=x",
		"another user":                server.URL + "/v1.0/users/victim@contoso.com/messages?$skiptoken=x",
		"outside the mailbox":         server.URL + "/v1.0/me/drive/root/children?$skiptoken=x",
		"a send endpoint":             server.URL + "/v1.0/me/sendMail",
		"a messages-looking path":     server.URL + "/v1.0/me/drive/root/messages",
		"dot segments":                server.URL + "/v1.0/me/messages/../drive/root/children",
		"dot segments inside folders": server.URL + "/v1.0/me/mailFolders/inbox/messages/../../../drive/root/children",
		"empty segment":               server.URL + "/v1.0/me//messages",
		"a fragment":                  server.URL + "/v1.0/me/messages?$skiptoken=x#frag",
		"a single message":            server.URL + "/v1.0/me/messages/AAMkAD",
		"the bare mailbox":            server.URL + "/v1.0/me/",
	}
	for name, token := range rejected {
		t.Run(name, func(t *testing.T) {
			_, err := executeMailRead(service, "microsoft", "mail.messages", map[string]any{"pageToken": token})
			if err == nil {
				t.Fatalf("pageToken %q was accepted", token)
			}
			if !strings.Contains(err.Error(), "pageToken") {
				t.Fatalf("error = %q, want it to name pageToken", err)
			}
		})
	}
	if hits.Load() != 0 || foreignHits.Load() != 0 {
		t.Fatalf("a rejected page token still caused provider traffic: graph=%d foreign=%d", hits.Load(), foreignHits.Load())
	}
}

func TestMailReadOutlookDoesNotHandOutANextLinkItWouldRefuse(t *testing.T) {
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		mailReadJSON(w, map[string]any{
			"value":           []any{},
			"@odata.nextLink": "https://evil.example/v1.0/me/messages?$skiptoken=x",
		})
	})
	payload := mustExecuteMailRead(t, service, "microsoft", "mail.messages", nil)
	if token, present := payload["nextPageToken"]; present {
		t.Fatalf("nextPageToken = %#v for a link on a foreign host", token)
	}
}

func TestMailReadOutlookUnreadWithSearchIsAppliedLocallyBecauseSearchCannotTakeAFilter(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"value": []any{
			map[string]any{"id": "m1", "isRead": false},
			map[string]any{"id": "m2", "isRead": true},
			map[string]any{"id": "m3"},
		}})
	})
	payload := mustExecuteMailRead(t, service, "microsoft", "mail.messages", map[string]any{"query": "invoice", "unread": true})
	call := rec.only(t)
	if _, present := call.Query["$filter"]; present {
		t.Fatalf("$filter must not be sent with $search: %s", call.RawQuery)
	}
	rows, _ := payload["value"].([]any)
	var ids []string
	for _, row := range rows {
		ids = append(ids, row.(map[string]any)["id"].(string))
	}
	if !reflect.DeepEqual(ids, []string{"m1", "m3"}) {
		t.Fatalf("ids = %v, want the already-read message dropped", ids)
	}
	if payload["filteredOutRead"] != float64(1) && payload["filteredOutRead"] != 1 {
		t.Fatalf("filteredOutRead = %#v, want 1 so a short page can be explained", payload["filteredOutRead"])
	}
}

// --- Outlook: get -------------------------------------------------------------

func TestMailReadOutlookGetAddsRecipientsAndAttachmentMetadataAndKeepsBodyText(t *testing.T) {
	var rec mailReadRecorder
	service, _ := newGraphMailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{
			"id":                "msg-1",
			"subject":           "Quarterly numbers",
			"hasAttachments":    true,
			"conversationId":    "conv-1",
			"internetMessageId": "<abc@contoso.com>",
			"toRecipients":      []any{map[string]any{"emailAddress": map[string]any{"address": "me@contoso.com"}}},
			"ccRecipients":      []any{},
			"body":              map[string]any{"contentType": "html", "content": "<table><tr><td>Q1</td><td>42</td></tr></table>"},
			"attachments": []any{
				map[string]any{"id": "att-1", "name": "q1.xlsx", "contentType": "application/vnd.ms-excel", "size": float64(2048), "isInline": false},
			},
		})
	})
	payload := mustExecuteMailRead(t, service, "microsoft", "mail.messages.get", map[string]any{"messageId": "msg-1"})
	call := rec.only(t)
	if call.Path != "/v1.0/me/messages/msg-1" {
		t.Fatalf("path = %q", call.Path)
	}
	selected := call.Query.Get("$select")
	for _, field := range []string{"id", "subject", "from", "receivedDateTime", "webLink", "body", "toRecipients", "ccRecipients", "hasAttachments", "conversationId", "internetMessageId"} {
		if !strings.Contains(","+selected+",", ","+field+",") {
			t.Fatalf("$select = %q is missing %s", selected, field)
		}
	}
	// Metadata only: never the attachment bytes.
	if got := call.Query.Get("$expand"); got != "attachments($select=id,name,contentType,size,isInline)" {
		t.Fatalf("$expand = %q, want the attachment metadata projection", got)
	}
	if bodyText, _ := payload["bodyText"].(string); !strings.Contains(bodyText, "Q1") || !strings.Contains(bodyText, "42") || strings.Contains(bodyText, "<td>") {
		t.Fatalf("bodyText = %q, want the HTML-only body rendered to text", payload["bodyText"])
	}
	if _, ok := payload["body"].(map[string]any); !ok {
		t.Fatalf("body is dropped: %#v", payload)
	}
	attachments, _ := payload["attachments"].([]any)
	if len(attachments) != 1 || attachments[0].(map[string]any)["name"] != "q1.xlsx" {
		t.Fatalf("attachments = %#v, want the Graph metadata passed through", payload["attachments"])
	}
	if payload["internetMessageId"] != "<abc@contoso.com>" || payload["conversationId"] != "conv-1" {
		t.Fatalf("payload = %#v", payload)
	}
}

// --- Gmail: list --------------------------------------------------------------

func TestMailReadGmailDefaultListStaysAPlainIdListWithOneRequest(t *testing.T) {
	// execution-core's gmail.send read-back lists 100 ids and matches the new
	// message's id; it must keep getting exactly that, with no fan-out.
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{
			"messages":           []any{map[string]any{"id": "m1", "threadId": "t1"}, map[string]any{"id": "m2", "threadId": "t2"}},
			"nextPageToken":      "next-1",
			"resultSizeEstimate": float64(2),
		})
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"maxResults": float64(100)})
	call := rec.only(t)
	if call.Path != "/gmail/v1/users/me/messages" || call.RawQuery != "maxResults=100" {
		t.Fatalf("request = %s?%s, want the unchanged ids-only list", call.Path, call.RawQuery)
	}
	messages, _ := payload["messages"].([]any)
	if len(messages) != 2 {
		t.Fatalf("messages = %#v", payload["messages"])
	}
	first := messages[0].(map[string]any)
	if len(first) != 2 || first["id"] != "m1" || first["threadId"] != "t1" {
		t.Fatalf("first message = %#v, want id and threadId only", first)
	}
	if payload["nextPageToken"] != "next-1" {
		t.Fatalf("nextPageToken = %#v, want Gmail's own token passed through", payload["nextPageToken"])
	}
}

func TestMailReadGmailComposesQAndLabelsFromTheNeutralParams(t *testing.T) {
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"resultSizeEstimate": float64(0)})
	})
	mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{
		"query": "invoice", "from": "bob@contoso.com", "since": "2026-01-05", "until": "2026-01-31",
		"folder": "inbox", "unread": true, "maxResults": float64(5), "pageToken": "tok/en+1==",
	})
	call := rec.only(t)
	if call.Path != "/gmail/v1/users/me/messages" {
		t.Fatalf("path = %q", call.Path)
	}
	// before: is exclusive on Gmail's side, so the inclusive until day is sent
	// as the following day.
	wantQ := "invoice from:bob@contoso.com after:2026/01/05 before:2026/02/01"
	if got := call.Query.Get("q"); got != wantQ {
		t.Fatalf("q = %q, want %q", got, wantQ)
	}
	if got := call.Query["labelIds"]; !reflect.DeepEqual(got, []string{"INBOX", "UNREAD"}) {
		t.Fatalf("labelIds = %v, want INBOX and UNREAD", got)
	}
	if call.Query.Get("maxResults") != "5" || call.Query.Get("pageToken") != "tok/en+1==" {
		t.Fatalf("query = %s, want maxResults=5 and the page token passed through", call.RawQuery)
	}
}

func TestMailReadGmailFolderMapping(t *testing.T) {
	cases := []struct {
		folder     string
		wantLabels []string
		wantQ      string
		wantTrash  bool
	}{
		{folder: "inbox", wantLabels: []string{"INBOX"}},
		{folder: "sent", wantLabels: []string{"SENT"}},
		{folder: "drafts", wantLabels: []string{"DRAFT"}},
		{folder: "deleted", wantLabels: []string{"TRASH"}, wantTrash: true},
		{folder: "archive", wantQ: "-in:inbox -in:sent -in:drafts -in:chats"},
	}
	for _, tc := range cases {
		t.Run(tc.folder, func(t *testing.T) {
			var rec mailReadRecorder
			service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				mailReadJSON(w, map[string]any{})
			})
			mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"folder": tc.folder})
			call := rec.only(t)
			if got := call.Query["labelIds"]; !reflect.DeepEqual(got, tc.wantLabels) {
				t.Fatalf("labelIds = %v, want %v", got, tc.wantLabels)
			}
			if got := call.Query.Get("q"); got != tc.wantQ {
				t.Fatalf("q = %q, want %q", got, tc.wantQ)
			}
			if got := call.Query.Get("includeSpamTrash") == "true"; got != tc.wantTrash {
				t.Fatalf("includeSpamTrash = %v, want %v", got, tc.wantTrash)
			}
		})
	}
}

func TestMailReadGmailInjectionAttemptsStayInsideTheirOwnParameter(t *testing.T) {
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{})
	})
	// query is the user's own Gmail search and passes as one q value; what it
	// must never do is become another request parameter or a second line.
	mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{
		"query":      "x&labelIds=TRASH&maxResults=500&includeSpamTrash=true\r\nX-Evil: 1",
		"from":       `bob@x.com OR from:ceo@corp.com" label:secret`,
		"maxResults": float64(4),
	})
	call := rec.only(t)
	if got := call.Query["labelIds"]; len(got) != 0 {
		t.Fatalf("labelIds = %v, a query smuggled a label", got)
	}
	if got := call.Query.Get("maxResults"); got != "4" {
		t.Fatalf("maxResults = %q, want 4", got)
	}
	if call.Query.Get("includeSpamTrash") != "" {
		t.Fatalf("includeSpamTrash was smuggled in: %s", call.RawQuery)
	}
	wantQ := `x&labelIds=TRASH&maxResults=500&includeSpamTrash=true X-Evil: 1 from:"bob@x.com OR from:ceo@corp.com label:secret"`
	if got := call.Query.Get("q"); got != wantQ {
		t.Fatalf("q = %q, want %q (the sender is one quoted phrase, control characters are gone)", got, wantQ)
	}
	if call.Header.Get("X-Evil") != "" {
		t.Fatal("a header was injected through the query")
	}
}

func TestMailReadGmailGroupsAnORQueryBeforeAddingFilters(t *testing.T) {
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{})
	})
	mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"query": "budget OR forecast", "since": "2026-01-05"})
	want := "(budget OR forecast) after:2026/01/05"
	if got := rec.only(t).Query.Get("q"); got != want {
		t.Fatalf("q = %q, want %q", got, want)
	}
}

func TestMailReadGmailUnreadAndHeadersAcceptStringValuesFromLegacyQueryRoutes(t *testing.T) {
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if strings.HasSuffix(r.URL.Path, "/messages") {
			mailReadJSON(w, map[string]any{"messages": []any{map[string]any{"id": "m1", "threadId": "t1"}}})
			return
		}
		mailReadJSON(w, map[string]any{"id": "m1", "threadId": "t1", "snippet": "hi"})
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"unread": "true", "headers": "true", "maxResults": "3"})
	calls := rec.all()
	if len(calls) != 2 {
		t.Fatalf("requests = %d, want the list and one metadata read", len(calls))
	}
	if got := calls[0].Query["labelIds"]; !reflect.DeepEqual(got, []string{"UNREAD"}) {
		t.Fatalf("labelIds = %v", got)
	}
	messages, _ := payload["messages"].([]any)
	if len(messages) != 1 || messages[0].(map[string]any)["snippet"] != "hi" {
		t.Fatalf("messages = %#v", payload["messages"])
	}
}

// --- Gmail: header fan-out ----------------------------------------------------

func gmailMetadataResponse(id string) map[string]any {
	return map[string]any{
		"id":       id,
		"threadId": "thread-" + id,
		"snippet":  "snippet of " + id,
		"labelIds": []any{"INBOX", "UNREAD"},
		"payload": map[string]any{
			"headers": []any{
				map[string]any{"name": "Subject", "value": "Subject " + id},
				map[string]any{"name": "From", "value": "Alice <alice@example.com>"},
				map[string]any{"name": "To", "value": "me@example.com"},
				map[string]any{"name": "Cc", "value": "carol@example.com"},
				map[string]any{"name": "Date", "value": "Mon, 5 Jan 2026 09:00:00 +0100"},
			},
		},
	}
}

func gmailIDList(count int) map[string]any {
	messages := make([]any, 0, count)
	for i := 1; i <= count; i++ {
		messages = append(messages, map[string]any{"id": fmt.Sprintf("m%d", i), "threadId": fmt.Sprintf("t%d", i)})
	}
	return map[string]any{"messages": messages, "nextPageToken": "page-2"}
}

func TestMailReadGmailHeadersFanOutIsBoundedToFiveAtATime(t *testing.T) {
	var rec mailReadRecorder
	var inFlight, maxInFlight atomic.Int32
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Path == "/gmail/v1/users/me/messages" {
			mailReadJSON(w, gmailIDList(20))
			return
		}
		current := inFlight.Add(1)
		for {
			seen := maxInFlight.Load()
			if current <= seen || maxInFlight.CompareAndSwap(seen, current) {
				break
			}
		}
		time.Sleep(40 * time.Millisecond)
		inFlight.Add(-1)
		id := strings.TrimPrefix(r.URL.Path, "/gmail/v1/users/me/messages/")
		mailReadJSON(w, gmailMetadataResponse(id))
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"headers": true, "maxResults": float64(20)})

	calls := rec.all()
	if len(calls) != 21 {
		t.Fatalf("requests = %d, want 1 list + 20 metadata reads", len(calls))
	}
	if peak := maxInFlight.Load(); peak > 5 {
		t.Fatalf("peak concurrent metadata reads = %d, want at most 5", peak)
	} else if peak < 2 {
		t.Fatalf("peak concurrent metadata reads = %d, the fan-out is not running in parallel", peak)
	}
	metadata := calls[1]
	if metadata.Query.Get("format") != "metadata" {
		t.Fatalf("format = %q, want metadata", metadata.Query.Get("format"))
	}
	var requested []string
	requested = append(requested, metadata.Query["metadataHeaders"]...)
	if !reflect.DeepEqual(requested, []string{"Subject", "From", "To", "Cc", "Date"}) {
		t.Fatalf("metadataHeaders = %v", requested)
	}
	messages, _ := payload["messages"].([]any)
	if len(messages) != 20 {
		t.Fatalf("messages = %d, want all 20 kept in list order", len(messages))
	}
	for index, raw := range messages {
		message := raw.(map[string]any)
		id := fmt.Sprintf("m%d", index+1)
		if message["id"] != id || message["subject"] != "Subject "+id || message["from"] != "Alice <alice@example.com>" ||
			message["to"] != "me@example.com" || message["cc"] != "carol@example.com" ||
			message["date"] != "Mon, 5 Jan 2026 09:00:00 +0100" || message["snippet"] != "snippet of "+id ||
			message["threadId"] != fmt.Sprintf("t%d", index+1) {
			t.Fatalf("message %d = %#v", index, message)
		}
		if !reflect.DeepEqual(message["labelIds"], []any{"INBOX", "UNREAD"}) {
			t.Fatalf("message %d labelIds = %#v", index, message["labelIds"])
		}
	}
	if payload["nextPageToken"] != "page-2" {
		t.Fatalf("nextPageToken = %#v, want it kept next to the enriched messages", payload["nextPageToken"])
	}
}

func TestMailReadGmailHeadersClampsThePageToTheFanOutCap(t *testing.T) {
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Path == "/gmail/v1/users/me/messages" {
			mailReadJSON(w, map[string]any{"messages": []any{}})
			return
		}
		mailReadJSON(w, gmailMetadataResponse("x"))
	})
	mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"headers": true, "maxResults": float64(100)})
	if got := rec.only(t).Query.Get("maxResults"); got != "25" {
		t.Fatalf("maxResults = %q, want the list clamped to the fan-out cap of 25", got)
	}
}

func TestMailReadGmailHeadersNeverReadMoreThanTheCapEvenIfTheListIsLonger(t *testing.T) {
	var metadataReads atomic.Int32
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/gmail/v1/users/me/messages" {
			mailReadJSON(w, gmailIDList(40))
			return
		}
		metadataReads.Add(1)
		mailReadJSON(w, gmailMetadataResponse(strings.TrimPrefix(r.URL.Path, "/gmail/v1/users/me/messages/")))
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"headers": true})
	if got := metadataReads.Load(); got != 25 {
		t.Fatalf("metadata reads = %d, want exactly the cap of 25", got)
	}
	messages, _ := payload["messages"].([]any)
	if len(messages) != 40 {
		t.Fatalf("messages = %d, want the whole list kept", len(messages))
	}
	tail := messages[30].(map[string]any)
	if _, enriched := tail["subject"]; enriched || tail["id"] != "m31" {
		t.Fatalf("message beyond the cap = %#v, want ids only", tail)
	}
}

func TestMailReadGmailHeadersTolerateOneMessageFailingWithoutFailingTheList(t *testing.T) {
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/gmail/v1/users/me/messages":
			mailReadJSON(w, gmailIDList(5))
		case "/gmail/v1/users/me/messages/m2":
			w.WriteHeader(http.StatusInternalServerError)
			_, _ = w.Write([]byte(`{"error":{"message":"private detail that must not travel"}}`))
		case "/gmail/v1/users/me/messages/m4":
			w.WriteHeader(http.StatusForbidden)
			_, _ = w.Write([]byte(`{"error":{"code":403,"errors":[{"reason":"insufficientPermissions"}]}}`))
		case "/gmail/v1/users/me/messages/m5":
			// 200 with a body that is not an object.
			_, _ = w.Write([]byte(`[]`))
		default:
			mailReadJSON(w, gmailMetadataResponse(strings.TrimPrefix(r.URL.Path, "/gmail/v1/users/me/messages/")))
		}
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"headers": true, "maxResults": float64(5)})
	messages, _ := payload["messages"].([]any)
	if len(messages) != 5 {
		t.Fatalf("messages = %d, want 5", len(messages))
	}
	for _, index := range []int{0, 2} {
		if message := messages[index].(map[string]any); message["error"] != nil || message["subject"] == nil {
			t.Fatalf("message %d = %#v, want it enriched", index, message)
		}
	}
	for index, wantID := range map[int]string{1: "m2", 3: "m4", 4: "m5"} {
		message := messages[index].(map[string]any)
		text, _ := message["error"].(string)
		if message["id"] != wantID || text == "" {
			t.Fatalf("message %d = %#v, want {id, error}", index, message)
		}
		if _, enriched := message["subject"]; enriched {
			t.Fatalf("a failed message carries fields: %#v", message)
		}
		if strings.Contains(text, "private detail") {
			t.Fatalf("the provider's message travelled in the item error: %q", text)
		}
	}
	if got := messages[3].(map[string]any)["error"].(string); !strings.Contains(got, "403") {
		t.Fatalf("item error = %q, want the credential rejection to say 403", got)
	}
}

func TestMailReadGmailHeadersOnAnEmptyMailboxMakeNoMetadataCalls(t *testing.T) {
	var rec mailReadRecorder
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		mailReadJSON(w, map[string]any{"resultSizeEstimate": float64(0)})
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages", map[string]any{"headers": true})
	rec.only(t)
	if _, present := payload["messages"]; present {
		t.Fatalf("payload = %#v, want Gmail's empty answer untouched", payload)
	}
}

// --- Gmail: get ---------------------------------------------------------------

func TestMailReadGmailGetAddsRecipientsSnippetAndAttachmentMetadata(t *testing.T) {
	plain := base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString([]byte("See attached."))
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/gmail/v1/users/me/messages/msg-7" || r.URL.Query().Get("format") != "full" {
			t.Fatalf("request = %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		mailReadJSON(w, map[string]any{
			"id":       "msg-7",
			"threadId": "thread-7",
			"snippet":  "See attached.",
			"payload": map[string]any{
				"mimeType": "multipart/mixed",
				"headers": []any{
					map[string]any{"name": "Subject", "value": "Contract"},
					map[string]any{"name": "From", "value": "legal@example.com"},
					map[string]any{"name": "To", "value": "me@example.com, you@example.com"},
					map[string]any{"name": "Cc", "value": "boss@example.com"},
					map[string]any{"name": "Date", "value": "Tue, 6 Jan 2026 10:00:00 +0000"},
				},
				"parts": []any{
					map[string]any{"mimeType": "text/plain", "filename": "", "body": map[string]any{"data": plain, "size": float64(13)}},
					map[string]any{"mimeType": "application/pdf", "filename": "contract.pdf", "body": map[string]any{"attachmentId": "att-1", "size": float64(48213)}},
					// A large text body is delivered by attachmentId too: no filename, so not an attachment.
					map[string]any{"mimeType": "text/html", "filename": "", "body": map[string]any{"attachmentId": "body-2", "size": float64(900000)}},
					map[string]any{"mimeType": "multipart/mixed", "filename": "", "parts": []any{
						map[string]any{"mimeType": "text/csv", "filename": "lines.csv", "body": map[string]any{"attachmentId": "att-2", "size": float64(77)}},
					}},
				},
			},
		})
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages.get", map[string]any{"id": "msg-7"})
	if payload["to"] != "me@example.com, you@example.com" || payload["cc"] != "boss@example.com" || payload["snippet"] != "See attached." {
		t.Fatalf("payload = %#v", payload)
	}
	if payload["subject"] != "Contract" || payload["from"] != "legal@example.com" || payload["bodyText"] != "See attached." {
		t.Fatalf("the picker's existing keys changed: %#v", payload)
	}
	want := []any{
		map[string]any{"filename": "contract.pdf", "mimeType": "application/pdf", "size": int64(48213), "attachmentId": "att-1"},
		map[string]any{"filename": "lines.csv", "mimeType": "text/csv", "size": int64(77), "attachmentId": "att-2"},
	}
	if !reflect.DeepEqual(payload["attachments"], want) {
		t.Fatalf("attachments = %#v, want %#v", payload["attachments"], want)
	}
}

func TestMailReadGmailGetRendersAnHTMLOnlyMessageAndListsNoAttachments(t *testing.T) {
	html := base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString([]byte("<html><body><table><tr><td>Total</td><td>99</td></tr></table></body></html>"))
	service := newGmailTestService(t, func(w http.ResponseWriter, r *http.Request) {
		mailReadJSON(w, map[string]any{
			"id": "msg-8", "threadId": "thread-8",
			"payload": map[string]any{
				"mimeType": "text/html",
				"headers":  []any{map[string]any{"name": "Subject", "value": "Receipt"}},
				"body":     map[string]any{"data": html},
			},
		})
	})
	payload := mustExecuteMailRead(t, service, "google", "gmail.messages.get", map[string]any{"id": "msg-8"})
	bodyText, _ := payload["bodyText"].(string)
	if !strings.Contains(bodyText, "Total") || !strings.Contains(bodyText, "99") || strings.Contains(bodyText, "<td>") {
		t.Fatalf("bodyText = %q, want the HTML rendered down to text", bodyText)
	}
	attachments, ok := payload["attachments"].([]any)
	if !ok || len(attachments) != 0 {
		t.Fatalf("attachments = %#v, want an empty list, not null", payload["attachments"])
	}
	if payload["to"] != "" || payload["cc"] != "" {
		t.Fatalf("missing recipient headers should be empty strings: %#v", payload)
	}
}
