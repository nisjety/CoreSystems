package actions

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	neturl "net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

// F3: calendar read for the connected user's own Outlook and Google calendar.
// These tests pin the provider-facing request of calendar.events (the window,
// the time zone, the paging) and the local search filter, with the clock
// pinned so a default window is an exact assertion.

// calendarTestNow is 14:00 in Oslo (CEST, UTC+2).
var calendarTestNow = time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)

type calendarCall struct {
	Path     string
	RawQuery string
	Query    neturl.Values
	Header   http.Header
}

type calendarRecorder struct {
	mu    sync.Mutex
	calls []calendarCall
}

func (r *calendarRecorder) record(req *http.Request) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.calls = append(r.calls, calendarCall{
		Path:     req.URL.Path,
		RawQuery: req.URL.RawQuery,
		Query:    req.URL.Query(),
		Header:   req.Header.Clone(),
	})
}

func (r *calendarRecorder) all() []calendarCall {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]calendarCall(nil), r.calls...)
}

func (r *calendarRecorder) only(t *testing.T) calendarCall {
	t.Helper()
	calls := r.all()
	if len(calls) != 1 {
		t.Fatalf("provider received %d requests, want exactly 1: %+v", len(calls), calls)
	}
	return calls[0]
}

func (r *calendarRecorder) none(t *testing.T) {
	t.Helper()
	if calls := r.all(); len(calls) != 0 {
		t.Fatalf("provider received %d requests, want none: %+v", len(calls), calls)
	}
}

func calendarJSON(w http.ResponseWriter, value any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(value)
}

// newCalendarGraphService serves Graph over TLS: the page-token validator only
// accepts https links on the configured Graph host.
func newCalendarGraphService(t *testing.T, handler http.HandlerFunc) (*Service, *httptest.Server) {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	service := NewService(config.Config{MicrosoftGraphBaseURL: server.URL}, server.Client())
	service.now = func() time.Time { return calendarTestNow }
	return service, server
}

func newCalendarGoogleService(t *testing.T, handler http.HandlerFunc) *Service {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	service := NewService(config.Config{GoogleAPIBaseURL: server.URL}, server.Client())
	service.now = func() time.Time { return calendarTestNow }
	return service
}

func executeCalendar(service *Service, provider string, params map[string]any) (map[string]any, error) {
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: provider}, AccessToken: "token-1",
		Operation: "calendar.events", Params: params,
	})
	if err != nil {
		return nil, err
	}
	payload, _ := result.Result.(map[string]any)
	return payload, nil
}

func mustExecuteCalendar(t *testing.T, service *Service, provider string, params map[string]any) map[string]any {
	t.Helper()
	payload, err := executeCalendar(service, provider, params)
	if err != nil {
		t.Fatalf("Execute error: %v", err)
	}
	return payload
}

func graphEventFixture(id, subject string, fields map[string]any) map[string]any {
	event := map[string]any{
		"id":      id,
		"subject": subject,
		"start":   map[string]any{"dateTime": "2026-10-02T09:00:00.0000000", "timeZone": "W. Europe Standard Time"},
		"end":     map[string]any{"dateTime": "2026-10-02T10:00:00.0000000", "timeZone": "W. Europe Standard Time"},
	}
	for key, value := range fields {
		event[key] = value
	}
	return event
}

func graphEventIDs(t *testing.T, payload map[string]any) []string {
	t.Helper()
	rows, _ := payload["value"].([]any)
	ids := []string{}
	for _, row := range rows {
		event, _ := row.(map[string]any)
		ids = append(ids, stringFromAny(event["id"]))
	}
	return ids
}

func assertCalendarWindow(t *testing.T, query neturl.Values, startKey, endKey, wantStart, wantEnd string) {
	t.Helper()
	if got := query.Get(startKey); got != wantStart {
		t.Fatalf("%s = %q, want %q", startKey, got, wantStart)
	}
	if got := query.Get(endKey); got != wantEnd {
		t.Fatalf("%s = %q, want %q", endKey, got, wantEnd)
	}
}

// --- Outlook: request ---------------------------------------------------------

func TestCalendarOutlookUsesCalendarViewWithDefaultWindowAndLocalTime(t *testing.T) {
	var rec calendarRecorder
	service, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		calendarJSON(w, map[string]any{"value": []any{}})
	})
	payload := mustExecuteCalendar(t, service, "microsoft", map[string]any{})
	call := rec.only(t)

	if call.Path != "/v1.0/me/calendarView" {
		t.Fatalf("path = %q, want /v1.0/me/calendarView (a window of occurrences, not the raw /me/events list)", call.Path)
	}
	assertCalendarWindow(t, call.Query, "startDateTime", "endDateTime", "2026-10-01T12:00:00Z", "2026-10-15T12:00:00Z")
	if got := call.Query.Get("$orderby"); got != "start/dateTime" {
		t.Fatalf("$orderby = %q, want start/dateTime", got)
	}
	if got := call.Query.Get("$top"); got != "10" {
		t.Fatalf("$top = %q, want the existing default of 10", got)
	}
	for _, forbidden := range []string{"$search", "$filter"} {
		if _, present := call.Query[forbidden]; present {
			t.Fatalf("%s must not be sent: events do not support $search and the query is filtered locally; query = %s", forbidden, call.RawQuery)
		}
	}
	if got := call.Header.Get("ConsistencyLevel"); got != "" {
		t.Fatalf("ConsistencyLevel = %q, want none (it only belonged to $search)", got)
	}
	// Without this header 09:00 in Oslo is delivered as 07:00 UTC.
	prefer := call.Header.Get("Prefer")
	if !strings.Contains(prefer, `outlook.timezone="W. Europe Standard Time"`) {
		t.Fatalf("Prefer = %q, want the Windows zone name of Europe/Oslo", prefer)
	}
	selected := map[string]bool{}
	for _, field := range strings.Split(call.Query.Get("$select"), ",") {
		selected[field] = true
	}
	// body stays selected because the composer picker builds an Outlook event's
	// attachment text from body.content.
	for _, field := range []string{"id", "subject", "start", "end", "location", "organizer", "attendees", "bodyPreview", "body", "isAllDay", "isCancelled", "webLink", "onlineMeeting", "type", "seriesMasterId"} {
		if !selected[field] {
			t.Fatalf("$select = %q, missing %s", call.Query.Get("$select"), field)
		}
	}
	window, _ := payload["window"].(map[string]any)
	if window["start"] != "2026-10-01T14:00:00+02:00" || window["end"] != "2026-10-15T14:00:00+02:00" || window["timeZone"] != "Europe/Oslo" {
		t.Fatalf("window = %#v, want the effective window in Oslo local time", payload["window"])
	}
}

func TestCalendarOutlookQueryWithoutWindowWidensToAroundNow(t *testing.T) {
	var rec calendarRecorder
	service, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		calendarJSON(w, map[string]any{"value": []any{}})
	})
	mustExecuteCalendar(t, service, "microsoft", map[string]any{"query": "budget"})
	call := rec.only(t)
	assertCalendarWindow(t, call.Query, "startDateTime", "endDateTime", "2026-09-01T12:00:00Z", "2026-12-30T12:00:00Z")
	if got := call.Query.Get("$top"); got != "100" {
		t.Fatalf("$top = %q, want 100: the local filter only sees the fetched page, so a search fetches the widest one", got)
	}
	if _, present := call.Query["$search"]; present {
		t.Fatalf("$search must not be sent, query = %s", call.RawQuery)
	}
}

func TestCalendarWindowParamsAreLocalDaysAndDateTimes(t *testing.T) {
	cases := []struct {
		name       string
		params     map[string]any
		start, end string
	}{
		{"dates in summer time", map[string]any{"since": "2026-10-05", "until": "2026-10-07"}, "2026-10-04T22:00:00Z", "2026-10-07T22:00:00Z"},
		{"single winter day", map[string]any{"since": "2026-12-01", "until": "2026-12-01"}, "2026-11-30T23:00:00Z", "2026-12-01T23:00:00Z"},
		{"the 25-hour day clocks go back", map[string]any{"since": "2026-10-25", "until": "2026-10-25"}, "2026-10-24T22:00:00Z", "2026-10-25T23:00:00Z"},
		{"local date-times", map[string]any{"since": "2026-10-05T09:00", "until": "2026-10-05T17:30"}, "2026-10-05T07:00:00Z", "2026-10-05T15:30:00Z"},
		{"RFC 3339 with an offset and with Z", map[string]any{"since": "2026-10-05T09:00:00+02:00", "until": "2026-10-05T10:00:00Z"}, "2026-10-05T07:00:00Z", "2026-10-05T10:00:00Z"},
		{"since alone runs two weeks", map[string]any{"since": "2026-10-05"}, "2026-10-04T22:00:00Z", "2026-10-18T22:00:00Z"},
		{"until alone starts now", map[string]any{"until": "2026-10-10"}, "2026-10-01T12:00:00Z", "2026-10-10T22:00:00Z"},
		{"until in the past looks two weeks back", map[string]any{"until": "2026-09-20"}, "2026-09-06T22:00:00Z", "2026-09-20T22:00:00Z"},
		{"until alone with a query starts 30 days back", map[string]any{"until": "2026-10-10", "query": "x"}, "2026-09-01T12:00:00Z", "2026-10-10T22:00:00Z"},
		{"exactly 366 days is allowed", map[string]any{"since": "2026-01-01", "until": "2027-01-01"}, "2025-12-31T23:00:00Z", "2027-01-01T23:00:00Z"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var outlook, google calendarRecorder
			graph, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
				outlook.record(r)
				calendarJSON(w, map[string]any{"value": []any{}})
			})
			gcal := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
				google.record(r)
				calendarJSON(w, map[string]any{"items": []any{}})
			})
			mustExecuteCalendar(t, graph, "microsoft", tc.params)
			assertCalendarWindow(t, outlook.only(t).Query, "startDateTime", "endDateTime", tc.start, tc.end)
			mustExecuteCalendar(t, gcal, "google", tc.params)
			assertCalendarWindow(t, google.only(t).Query, "timeMin", "timeMax", tc.start, tc.end)
		})
	}
}

func TestCalendarRefusesBadWindowsBeforeAnyProviderCall(t *testing.T) {
	cases := []struct {
		name    string
		params  map[string]any
		wantErr string
	}{
		{"not a date", map[string]any{"since": "yesterday"}, "since must be"},
		{"impossible date", map[string]any{"until": "2026-02-30"}, "until must be"},
		{"year before 1970", map[string]any{"since": "1969-12-31"}, "since must be"},
		{"not text", map[string]any{"since": float64(5)}, "since must be text"},
		{"until before since", map[string]any{"since": "2026-10-10", "until": "2026-10-05"}, "until must be after since"},
		{"367 days", map[string]any{"since": "2026-01-01", "until": "2027-01-02"}, "366"},
		{"query too long", map[string]any{"query": strings.Repeat("a", 501)}, "query is too long"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var outlook, google calendarRecorder
			graph, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) { outlook.record(r) })
			gcal := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) { google.record(r) })
			if _, err := executeCalendar(graph, "microsoft", tc.params); err == nil || !strings.Contains(err.Error(), tc.wantErr) {
				t.Fatalf("outlook error = %v, want one containing %q", err, tc.wantErr)
			}
			if _, err := executeCalendar(gcal, "google", tc.params); err == nil || !strings.Contains(err.Error(), tc.wantErr) {
				t.Fatalf("google error = %v, want one containing %q", err, tc.wantErr)
			}
			outlook.none(t)
			google.none(t)
		})
	}
}

func TestCalendarMaxResultsIsThePageSizeAndIsClamped(t *testing.T) {
	cases := []struct {
		name    string
		params  map[string]any
		wantTop string
	}{
		{"default", map[string]any{}, "10"},
		{"explicit", map[string]any{"maxResults": float64(7)}, "7"},
		{"string from a legacy GET route", map[string]any{"maxResults": "5"}, "5"},
		{"clamped", map[string]any{"maxResults": float64(500)}, "100"},
		{"explicit with a query", map[string]any{"maxResults": float64(7), "query": "x"}, "7"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var outlook, google calendarRecorder
			graph, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
				outlook.record(r)
				calendarJSON(w, map[string]any{"value": []any{}})
			})
			gcal := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
				google.record(r)
				calendarJSON(w, map[string]any{"items": []any{}})
			})
			mustExecuteCalendar(t, graph, "microsoft", tc.params)
			if got := outlook.only(t).Query.Get("$top"); got != tc.wantTop {
				t.Fatalf("outlook $top = %q, want %q", got, tc.wantTop)
			}
			mustExecuteCalendar(t, gcal, "google", tc.params)
			if got := google.only(t).Query.Get("maxResults"); got != tc.wantTop {
				t.Fatalf("google maxResults = %q, want %q", got, tc.wantTop)
			}
		})
	}
}

func TestCalendarOperationAliasesReachTheCalendarReader(t *testing.T) {
	var outlook, google calendarRecorder
	graph, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		outlook.record(r)
		calendarJSON(w, map[string]any{"value": []any{}})
	})
	gcal := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		google.record(r)
		calendarJSON(w, map[string]any{"items": []any{}})
	})
	if _, err := graph.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "microsoft"}, AccessToken: "t", Operation: "microsoft.calendar.events",
	}); err != nil {
		t.Fatalf("microsoft.calendar.events: %v", err)
	}
	if _, err := gcal.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: "google"}, AccessToken: "t", Operation: "google.calendar.events",
	}); err != nil {
		t.Fatalf("google.calendar.events: %v", err)
	}
	if call := outlook.only(t); call.Path != "/v1.0/me/calendarView" {
		t.Fatalf("outlook alias path = %q", call.Path)
	}
	if call := google.only(t); call.Query.Get("singleEvents") != "true" {
		t.Fatalf("google alias query = %s", call.RawQuery)
	}
}

// --- Outlook: local search filter ---------------------------------------------

func searchFixtureEvents() []any {
	return []any{
		graphEventFixture("by-preview", "Quarterly review", map[string]any{"bodyPreview": "Please bring the Budget file"}),
		graphEventFixture("by-location", "Offsite", map[string]any{"location": map[string]any{"displayName": "Budget Room"}}),
		graphEventFixture("by-organizer", "Sync", map[string]any{"organizer": map[string]any{"emailAddress": map[string]any{"name": "Budget Office", "address": "office@contoso.com"}}}),
		graphEventFixture("by-attendee", "Standup", map[string]any{"attendees": []any{
			map[string]any{"emailAddress": map[string]any{"name": "Kari Hansen", "address": "budget@fabrikam.no"}},
		}}),
		graphEventFixture("none", "Lunch", map[string]any{"location": map[string]any{"displayName": "Canteen"}}),
		graphEventFixture("norwegian", "Årsmøte i styret", nil),
	}
}

func TestCalendarOutlookLocalFilterHitsEveryFieldAndMissesTheRest(t *testing.T) {
	service, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		calendarJSON(w, map[string]any{"value": searchFixtureEvents()})
	})
	cases := []struct {
		name  string
		query string
		want  []string
	}{
		{"preview, location, organizer and attendee address", "budget", []string{"by-preview", "by-location", "by-organizer", "by-attendee"}},
		{"case-insensitive", "BuDgEt", []string{"by-preview", "by-location", "by-organizer", "by-attendee"}},
		{"every term must be found", "budget room", []string{"by-location"}},
		{"terms may be found in different fields", "kari quarterly", []string{}},
		{"non-ASCII letters fold", "ÅRSMØTE", []string{"norwegian"}},
		{"attendee name", "hansen", []string{"by-attendee"}},
		{"a miss", "zzz-nothing", []string{}},
		{"quotes are not part of the match", `"budget room"`, []string{"by-location"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			payload := mustExecuteCalendar(t, service, "microsoft", map[string]any{"query": tc.query})
			got := graphEventIDs(t, payload)
			if strings.Join(got, ",") != strings.Join(tc.want, ",") {
				t.Fatalf("ids = %v, want %v", got, tc.want)
			}
			if payload["scanned"] != 6 || payload["filteredOut"] != 6-len(tc.want) {
				t.Fatalf("scanned/filteredOut = %v/%v, want 6/%d", payload["scanned"], payload["filteredOut"], 6-len(tc.want))
			}
		})
	}
}

func TestCalendarOutlookWithoutQueryReturnsEveryRowUntouched(t *testing.T) {
	service, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		calendarJSON(w, map[string]any{"value": searchFixtureEvents()})
	})
	payload := mustExecuteCalendar(t, service, "microsoft", map[string]any{})
	if got := graphEventIDs(t, payload); len(got) != 6 {
		t.Fatalf("ids = %v, want all 6", got)
	}
	for _, key := range []string{"scanned", "filteredOut", "query", "nextPageToken", "moreAvailable"} {
		if _, present := payload[key]; present {
			t.Fatalf("%s must be absent without a query or a next page: %#v", key, payload)
		}
	}
	// The picker reads the list from Graph's own "value" key.
	if _, ok := payload["value"].([]any); !ok {
		t.Fatalf("value = %#v, want the Graph list array", payload["value"])
	}
}

// --- Outlook: paging ----------------------------------------------------------

func TestCalendarOutlookPageTokenReplaysTheNextLinkWithQueryAndWindow(t *testing.T) {
	var rec calendarRecorder
	service, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Query().Get("$skip") == "" {
			calendarJSON(w, map[string]any{
				"value": []any{
					graphEventFixture("budget-1", "Budget kickoff", nil),
					graphEventFixture("other-1", "Lunch", nil),
				},
				"@odata.nextLink": "https://" + r.Host + "/v1.0/me/calendarView?startDateTime=2026-09-01T12%3A00%3A00Z&endDateTime=2026-12-30T12%3A00%3A00Z&%24top=100&%24skip=100",
			})
			return
		}
		calendarJSON(w, map[string]any{"value": []any{
			graphEventFixture("budget-2", "Budget wrap-up", nil),
			graphEventFixture("other-2", "Coffee", nil),
		}})
	})
	first := mustExecuteCalendar(t, service, "microsoft", map[string]any{"query": "budget"})
	token, _ := first["nextPageToken"].(string)
	if token == "" || !strings.HasPrefix(token, "cal1.") {
		t.Fatalf("nextPageToken = %q, want a calendar page token", token)
	}
	if first["moreAvailable"] != true {
		t.Fatalf("moreAvailable = %v, want true", first["moreAvailable"])
	}
	if _, present := first["@odata.nextLink"]; present {
		t.Fatalf("the raw @odata.nextLink must not be handed on beside the token: %#v", first)
	}
	if got := strings.Join(graphEventIDs(t, first), ","); got != "budget-1" {
		t.Fatalf("first page ids = %q, want budget-1", got)
	}

	// The token alone continues the search: no query, no window.
	second := mustExecuteCalendar(t, service, "microsoft", map[string]any{"pageToken": token})
	calls := rec.all()
	if len(calls) != 2 {
		t.Fatalf("provider received %d requests, want 2", len(calls))
	}
	replay := calls[1]
	if replay.Path != "/v1.0/me/calendarView" || replay.Query.Get("$skip") != "100" || replay.Query.Get("startDateTime") != "2026-09-01T12:00:00Z" {
		t.Fatalf("replayed request = %s?%s, want the next link as Graph issued it", replay.Path, replay.RawQuery)
	}
	if !strings.Contains(replay.Header.Get("Prefer"), `outlook.timezone="W. Europe Standard Time"`) {
		t.Fatalf("replay Prefer = %q: the time zone header does not travel inside the link and must be sent again", replay.Header.Get("Prefer"))
	}
	if got := strings.Join(graphEventIDs(t, second), ","); got != "budget-2" {
		t.Fatalf("second page ids = %q, want budget-2 (the first page's query still applies)", got)
	}
	if second["query"] != "budget" {
		t.Fatalf("query echo = %v, want the token's query", second["query"])
	}
	if _, present := second["nextPageToken"]; present {
		t.Fatalf("last page must not carry a token: %#v", second)
	}
	firstWindow, _ := first["window"].(map[string]any)
	secondWindow, _ := second["window"].(map[string]any)
	if fmt.Sprint(firstWindow) != fmt.Sprint(secondWindow) || firstWindow["start"] == nil {
		t.Fatalf("window echo differs between pages: %v vs %v", firstWindow, secondWindow)
	}
}

func TestCalendarOutlookForeignNextLinkIsNeverHandedOn(t *testing.T) {
	service, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		calendarJSON(w, map[string]any{
			"value":           []any{graphEventFixture("e1", "One", nil)},
			"@odata.nextLink": "https://evil.example/v1.0/me/calendarView?%24skip=10",
		})
	})
	payload := mustExecuteCalendar(t, service, "microsoft", map[string]any{})
	if _, present := payload["nextPageToken"]; present {
		t.Fatalf("a link to another host became a page token: %#v", payload)
	}
	if payload["moreAvailable"] != true {
		t.Fatalf("moreAvailable = %v, want true so the caller can tell the list is cut short", payload["moreAvailable"])
	}
}

func TestCalendarPageTokenValidation(t *testing.T) {
	window := func(token calendarPageToken) calendarPageToken {
		if token.Start == "" {
			token.Start = "2026-10-01T12:00:00Z"
			token.End = "2026-10-15T12:00:00Z"
		}
		return token
	}
	b64 := func(text string) string { return base64.RawURLEncoding.EncodeToString([]byte(text)) }

	var rec calendarRecorder
	service, server := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		calendarJSON(w, map[string]any{"value": []any{}})
	})
	good := server.URL + "/v1.0/me/calendarView?startDateTime=2026-10-01T12%3A00%3A00Z&%24skip=10"

	// Control: a well-formed token is replayed.
	mustExecuteCalendar(t, service, "microsoft", map[string]any{"pageToken": encodeCalendarPageToken(window(calendarPageToken{Page: good}))})
	if call := rec.only(t); call.Query.Get("$skip") != "10" {
		t.Fatalf("control request = %s", call.RawQuery)
	}

	hostOnly := strings.TrimPrefix(server.URL, "https://")
	bad := map[string]string{
		"not issued by this service":  "AAMkAGI2THVSAAA=",
		"prefix only":                 "cal1.",
		"not base64":                  "cal1.!!!",
		"not json":                    "cal1." + b64("{nope"),
		"json of another shape":       "cal1." + b64(`["x"]`),
		"unknown field":               "cal1." + b64(`{"p":"`+good+`","x":1}`),
		"empty link":                  encodeCalendarPageToken(window(calendarPageToken{Page: ""})),
		"plaintext link":              encodeCalendarPageToken(window(calendarPageToken{Page: "http://" + hostOnly + "/v1.0/me/calendarView?x=1"})),
		"other host":                  encodeCalendarPageToken(window(calendarPageToken{Page: "https://evil.example/v1.0/me/calendarView?x=1"})),
		"look-alike host":             encodeCalendarPageToken(window(calendarPageToken{Page: "https://" + hostOnly + ".evil.example/v1.0/me/calendarView"})),
		"user info in the link":       encodeCalendarPageToken(window(calendarPageToken{Page: "https://user:pw@" + hostOnly + "/v1.0/me/calendarView"})),
		"fragment":                    encodeCalendarPageToken(window(calendarPageToken{Page: good + "#x"})),
		"mail list":                   encodeCalendarPageToken(window(calendarPageToken{Page: server.URL + "/v1.0/me/messages?x=1"})),
		"drive listing":               encodeCalendarPageToken(window(calendarPageToken{Page: server.URL + "/v1.0/me/drive/root/children"})),
		"another user's calendar":     encodeCalendarPageToken(window(calendarPageToken{Page: server.URL + "/v1.0/users/someone/calendarView"})),
		"a named calendar":            encodeCalendarPageToken(window(calendarPageToken{Page: server.URL + "/v1.0/me/calendars/abc/calendarView"})),
		"path escape":                 encodeCalendarPageToken(window(calendarPageToken{Page: server.URL + "/v1.0/me/calendarView/../drive/root/children"})),
		"window longer than a year":   encodeCalendarPageToken(calendarPageToken{Page: good, Start: "2026-01-01T00:00:00Z", End: "2027-06-01T00:00:00Z"}),
		"window that is not a time":   encodeCalendarPageToken(calendarPageToken{Page: good, Start: "soon", End: "later"}),
		"window with a missing bound": encodeCalendarPageToken(calendarPageToken{Page: good, Start: "2026-10-01T12:00:00Z"}),
		"query too long":              encodeCalendarPageToken(window(calendarPageToken{Page: good, Query: strings.Repeat("q", 501)})),
	}
	for name, token := range bad {
		t.Run(name, func(t *testing.T) {
			before := len(rec.all())
			_, err := executeCalendar(service, "microsoft", map[string]any{"pageToken": token})
			if err == nil || !strings.Contains(err.Error(), "pageToken") {
				t.Fatalf("error = %v, want a pageToken refusal", err)
			}
			if after := len(rec.all()); after != before {
				t.Fatalf("a refused token still caused %d provider request(s)", after-before)
			}
		})
	}
}

// --- Google -------------------------------------------------------------------

func TestCalendarGoogleDefaultRequestIsAWindowOfSingleEventsInLocalTime(t *testing.T) {
	var rec calendarRecorder
	service := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		calendarJSON(w, map[string]any{"items": []any{map[string]any{"id": "ev1", "summary": "Standup"}}})
	})
	payload := mustExecuteCalendar(t, service, "google", map[string]any{})
	call := rec.only(t)

	if call.Path != "/calendar/v3/calendars/primary/events" {
		t.Fatalf("path = %q", call.Path)
	}
	assertCalendarWindow(t, call.Query, "timeMin", "timeMax", "2026-10-01T12:00:00Z", "2026-10-15T12:00:00Z")
	for key, want := range map[string]string{
		"singleEvents": "true",
		"orderBy":      "startTime",
		"timeZone":     "Europe/Oslo",
		"maxResults":   "10",
		"fields":       "items(id,summary,description,location,start,end,attendees(displayName,email,responseStatus),organizer,htmlLink,status,hangoutLink),nextPageToken",
	} {
		if got := call.Query.Get(key); got != want {
			t.Fatalf("%s = %q, want %q", key, got, want)
		}
	}
	if _, present := call.Query["q"]; present {
		t.Fatalf("q must be absent without a query, query = %s", call.RawQuery)
	}
	items, _ := payload["items"].([]any)
	if len(items) != 1 {
		t.Fatalf("items = %#v, want the Google list array kept under items", payload["items"])
	}
	window, _ := payload["window"].(map[string]any)
	if window["start"] != "2026-10-01T14:00:00+02:00" || window["timeZone"] != "Europe/Oslo" {
		t.Fatalf("window = %#v", payload["window"])
	}
}

func TestCalendarGoogleQueryIsGoogleSideAndWidensTheWindow(t *testing.T) {
	var rec calendarRecorder
	service := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		calendarJSON(w, map[string]any{"items": []any{}})
	})
	mustExecuteCalendar(t, service, "google", map[string]any{"query": "budget\nreview"})
	call := rec.only(t)
	if got := call.Query.Get("q"); got != "budget review" {
		t.Fatalf("q = %q, want the cleaned free text", got)
	}
	assertCalendarWindow(t, call.Query, "timeMin", "timeMax", "2026-09-01T12:00:00Z", "2026-12-30T12:00:00Z")
	if got := call.Query.Get("maxResults"); got != "10" {
		t.Fatalf("maxResults = %q: Google filters on its side, so no wider default page is needed", got)
	}
}

func TestCalendarGoogleAnEmptyResultStillHasAnItemsArray(t *testing.T) {
	// With a fields mask Google answers {} when nothing matches; the picker
	// reads result.items.
	service := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		calendarJSON(w, map[string]any{})
	})
	payload := mustExecuteCalendar(t, service, "google", map[string]any{})
	items, ok := payload["items"].([]any)
	if !ok || len(items) != 0 {
		t.Fatalf("items = %#v, want an empty array", payload["items"])
	}
}

func TestCalendarGooglePageTokenKeepsTheWindowEvenWhenTheClockMovesOn(t *testing.T) {
	var rec calendarRecorder
	service := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Query().Get("pageToken") == "" {
			calendarJSON(w, map[string]any{"items": []any{map[string]any{"id": "a"}}, "nextPageToken": "google-opaque-1"})
			return
		}
		calendarJSON(w, map[string]any{"items": []any{map[string]any{"id": "b"}}})
	})
	first := mustExecuteCalendar(t, service, "google", map[string]any{"query": "review"})
	token, _ := first["nextPageToken"].(string)
	if token == "" || token == "google-opaque-1" || !strings.HasPrefix(token, "cal1.") {
		t.Fatalf("nextPageToken = %q, want Google's token wrapped with the window and query", token)
	}

	// Five minutes later: a window defaulted from "now" again would shift, and
	// Google binds a page token to the query it was issued for.
	service.now = func() time.Time { return calendarTestNow.Add(5 * time.Minute) }
	second := mustExecuteCalendar(t, service, "google", map[string]any{"pageToken": token, "since": "2030-01-01"})
	calls := rec.all()
	if len(calls) != 2 {
		t.Fatalf("provider received %d requests, want 2", len(calls))
	}
	firstCall, secondCall := calls[0], calls[1]
	if got := secondCall.Query.Get("pageToken"); got != "google-opaque-1" {
		t.Fatalf("pageToken sent to Google = %q, want the unwrapped token", got)
	}
	for _, key := range []string{"timeMin", "timeMax", "q", "orderBy", "singleEvents", "timeZone", "fields", "maxResults"} {
		if firstCall.Query.Get(key) != secondCall.Query.Get(key) {
			t.Fatalf("%s changed between pages: %q then %q", key, firstCall.Query.Get(key), secondCall.Query.Get(key))
		}
	}
	if _, present := second["nextPageToken"]; present {
		t.Fatalf("last page must not carry a token: %#v", second)
	}
	if second["query"] != "review" {
		t.Fatalf("query echo = %v, want review", second["query"])
	}
}

func TestCalendarGoogleRefusesTokensThatThisServiceDidNotIssue(t *testing.T) {
	var rec calendarRecorder
	service := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		calendarJSON(w, map[string]any{"items": []any{}})
	})
	for name, token := range map[string]string{
		"a bare Google token":     "CiAKGjBpNDd2Nmp2Zml2cXRwYjBpOXA",
		"window longer than year": encodeCalendarPageToken(calendarPageToken{Page: "x", Start: "2026-01-01T00:00:00Z", End: "2028-01-01T00:00:00Z"}),
		"control character":       encodeCalendarPageToken(calendarPageToken{Page: "a\nb", Start: "2026-10-01T12:00:00Z", End: "2026-10-15T12:00:00Z"}),
		"empty provider token":    encodeCalendarPageToken(calendarPageToken{Page: "", Start: "2026-10-01T12:00:00Z", End: "2026-10-15T12:00:00Z"}),
	} {
		t.Run(name, func(t *testing.T) {
			_, err := executeCalendar(service, "google", map[string]any{"pageToken": token})
			if err == nil || !strings.Contains(err.Error(), "pageToken") {
				t.Fatalf("error = %v, want a pageToken refusal", err)
			}
		})
	}
	if calls := rec.all(); len(calls) != 0 {
		t.Fatalf("refused tokens caused %d provider requests", len(calls))
	}
}

// --- Query text never becomes provider query syntax ---------------------------

func TestCalendarQueryTextCannotInjectProviderQuerySyntax(t *testing.T) {
	// Formerly guarded Graph's $search="..." phrase against an embedded quote.
	// Outlook now never puts the text in a provider query at all, and Google
	// receives it as one encoded q value.
	injected := `budget" OR subject:secret&maxResults=1000&timeMin=1970-01-01T00:00:00Z`
	var outlook, google calendarRecorder
	graph, _ := newCalendarGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		outlook.record(r)
		calendarJSON(w, map[string]any{"value": []any{}})
	})
	gcal := newCalendarGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		google.record(r)
		calendarJSON(w, map[string]any{"items": []any{}})
	})
	mustExecuteCalendar(t, graph, "microsoft", map[string]any{"query": injected})
	mustExecuteCalendar(t, gcal, "google", map[string]any{"query": injected})

	graphCall := outlook.only(t)
	for _, key := range []string{"$search", "$filter"} {
		if _, present := graphCall.Query[key]; present {
			t.Fatalf("%s was sent: %s", key, graphCall.RawQuery)
		}
	}
	if strings.Contains(graphCall.RawQuery, "secret") || strings.Contains(graphCall.RawQuery, "budget") {
		t.Fatalf("the query text reached the Graph request: %s", graphCall.RawQuery)
	}
	googleCall := google.only(t)
	if got := googleCall.Query.Get("q"); got != injected {
		t.Fatalf("q = %q, want the whole text as one value", got)
	}
	if got := googleCall.Query.Get("maxResults"); got != "10" {
		t.Fatalf("maxResults = %q: text in q must not be able to set another parameter", got)
	}
	if got := googleCall.Query.Get("timeMin"); got != "2026-09-01T12:00:00Z" {
		t.Fatalf("timeMin = %q: text in q must not be able to set another parameter", got)
	}
}

// --- Clock and time zone ------------------------------------------------------

func TestNewServiceDefaultsTheClockToWallTime(t *testing.T) {
	service := NewService(config.Config{}, http.DefaultClient)
	if service.now == nil {
		t.Fatal("NewService left the clock unset")
	}
	if drift := time.Since(service.now()); drift < -time.Minute || drift > time.Minute {
		t.Fatalf("default clock is %v away from the wall clock", drift)
	}
}

func TestCalendarLocationIsOsloWithDaylightSavingTime(t *testing.T) {
	// The runtime image has no zoneinfo (checked on the running integration-api
	// container), so this only holds in production because time/tzdata is
	// embedded by calendar_read.go.
	loc, err := calendarLocation()
	if err != nil {
		t.Fatalf("calendarLocation: %v", err)
	}
	cases := []struct {
		when time.Time
		want string
	}{
		{time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC), "+02:00"},
		{time.Date(2026, 10, 25, 0, 30, 0, 0, time.UTC), "+02:00"}, // clocks go back at 01:00 UTC
		{time.Date(2026, 10, 25, 1, 30, 0, 0, time.UTC), "+01:00"},
		{time.Date(2026, 12, 1, 12, 0, 0, 0, time.UTC), "+01:00"},
		{time.Date(2027, 7, 1, 12, 0, 0, 0, time.UTC), "+02:00"},
	}
	for _, tc := range cases {
		if got := tc.when.In(loc).Format("-07:00"); got != tc.want {
			t.Fatalf("offset at %s = %s, want %s", tc.when.Format(time.RFC3339), got, tc.want)
		}
	}
}
