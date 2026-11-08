package api

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// WS05-S4: audit list v2 (keyset cursor, filters, limit clamp). The store is an
// in-memory stub that honours the keyset contract of store.ListAudit, no DB.

// keysetStub serves rows already ordered (ingested_at DESC, id DESC), applies
// the cursor as "strictly older than" and cuts at Limit, like the SQL does.
type keysetStub struct {
	scopeStub
	all []store.AuditRow
}

func (s *keysetStub) ListAudit(_ context.Context, f store.AuditFilter) ([]store.AuditRow, error) {
	s.auditFilters = append(s.auditFilters, f)
	out := make([]store.AuditRow, 0, f.Limit)
	for _, row := range s.all {
		if f.HasCursor {
			older := row.IngestedAt.Before(f.CursorIngestedAt) ||
				(row.IngestedAt.Equal(f.CursorIngestedAt) && row.ID < f.CursorID)
			if !older {
				continue
			}
		}
		out = append(out, row)
		if f.Limit > 0 && len(out) == f.Limit {
			break
		}
	}
	return out, nil
}

// keysetRows builds n rows, newest first. Pairs of rows share an ingest instant
// so the id tie-break is exercised.
func keysetRows(n int) []store.AuditRow {
	base := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	rows := make([]store.AuditRow, 0, n)
	for i := 0; i < n; i++ {
		rows = append(rows, store.AuditRow{
			ID:         int64(n - i),
			IngestedAt: base.Add(-time.Duration(i/2) * time.Minute),
			OccurredAt: base,
			OrgID:      "org-a",
			UserID:     "user-a",
			Plane:      "control",
			Event:      "login",
			Outcome:    "ok",
		})
	}
	return rows
}

type listPage struct {
	ids        []int64
	nextCursor string
	meta       map[string]any
	status     int
}

func fetchListPage(t *testing.T, router http.Handler, target, role string) listPage {
	t.Helper()
	recorder := scopedGet(router, target, "user-a", role)
	page := listPage{status: recorder.Code}
	if recorder.Code != http.StatusOK {
		return page
	}
	envelope := decodeEnvelope(t, recorder)
	rows, _ := envelope["data"].([]any)
	for _, raw := range rows {
		row, _ := raw.(map[string]any)
		id, _ := row["id"].(float64)
		page.ids = append(page.ids, int64(id))
	}
	page.meta, _ = envelope["meta"].(map[string]any)
	page.nextCursor, _ = page.meta["next_cursor"].(string)
	return page
}

func TestAuditListCursorRoundTripCoversExactlyTheUnpagedRows(t *testing.T) {
	setReadMode(t, "", "")
	stub := &keysetStub{all: keysetRows(7)}
	router := keysetRouter(t, stub)

	var got []int64
	target := auditURL + "&limit=3"
	pages := 0
	for {
		page := fetchListPage(t, router, target, "admin")
		if page.status != http.StatusOK {
			t.Fatalf("page %d status %d", pages, page.status)
		}
		pages++
		got = append(got, page.ids...)
		if page.nextCursor == "" {
			break
		}
		if len(page.ids) != 3 {
			t.Fatalf("a page with a next_cursor must be full, got %d rows", len(page.ids))
		}
		target = auditURL + "&limit=3&cursor=" + page.nextCursor
		if pages > 5 {
			t.Fatal("paging does not terminate")
		}
	}
	if pages != 3 {
		t.Fatalf("7 rows at limit 3 must take 3 pages, took %d", pages)
	}
	if len(got) != 7 {
		t.Fatalf("pages returned %d rows, want 7: %v", len(got), got)
	}
	seen := map[int64]bool{}
	for i, id := range got {
		if seen[id] {
			t.Fatalf("row %d returned twice: %v", id, got)
		}
		seen[id] = true
		if want := int64(7 - i); id != want {
			t.Fatalf("row %d is %d, want %d (order must match the unpaged list): %v", i, id, want, got)
		}
	}
}

func keysetRouter(t *testing.T, stub *keysetStub) http.Handler {
	t.Helper()
	handler, err := New(stub, complianceTestRegistry())
	if err != nil {
		t.Fatal(err)
	}
	router := chi.NewRouter()
	handler.Mount(router)
	return router
}

func TestAuditListNextCursorOnlyWhenAnotherPageExists(t *testing.T) {
	setReadMode(t, "", "")

	// Exactly limit rows: the store is asked for limit+1 and returns limit, so
	// there is no next page and no next_cursor.
	stub := &keysetStub{all: keysetRows(3)}
	router := keysetRouter(t, stub)
	page := fetchListPage(t, router, auditURL+"&limit=3", "admin")
	if page.status != http.StatusOK || len(page.ids) != 3 {
		t.Fatalf("status %d rows %v", page.status, page.ids)
	}
	if _, present := page.meta["next_cursor"]; present {
		t.Fatalf("next_cursor must be absent on the last page: %v", page.meta)
	}
	if len(stub.auditFilters) != 1 || stub.auditFilters[0].Limit != 4 {
		t.Fatalf("the store must be asked for limit+1: %+v", stub.auditFilters)
	}
	if page.meta["limit"] != float64(3) || page.meta["count"] != float64(3) {
		t.Fatalf("meta limit/count: %v", page.meta)
	}

	// limit+1 rows available: the extra row is trimmed and a cursor is issued.
	stub = &keysetStub{all: keysetRows(4)}
	router = keysetRouter(t, stub)
	page = fetchListPage(t, router, auditURL+"&limit=3", "admin")
	if len(page.ids) != 3 || page.nextCursor == "" {
		t.Fatalf("rows %v cursor %q", page.ids, page.nextCursor)
	}
	cursor, err := decodeAuditCursor(page.nextCursor)
	if err != nil {
		t.Fatal(err)
	}
	// The cursor names the last row served (id 2), not the trimmed lookahead row.
	if cursor.ID != 2 {
		t.Fatalf("cursor id = %d, want the last served row (2)", cursor.ID)
	}
}

func TestAuditListCursorRoundTripEncoding(t *testing.T) {
	in := auditCursor{IngestedAt: time.Date(2026, 10, 1, 12, 30, 15, 123456000, time.UTC), ID: 42}
	out, err := decodeAuditCursor(encodeAuditCursor(in))
	if err != nil {
		t.Fatal(err)
	}
	if !out.IngestedAt.Equal(in.IngestedAt) || out.ID != in.ID {
		t.Fatalf("round trip lost data: %+v != %+v", out, in)
	}
	// A non-UTC instant is normalised to UTC on the wire and still equal.
	local := time.FixedZone("x", 3*3600)
	in2 := auditCursor{IngestedAt: time.Date(2026, 10, 1, 15, 0, 0, 5000, local), ID: 7}
	out2, err := decodeAuditCursor(encodeAuditCursor(in2))
	if err != nil || !out2.IngestedAt.Equal(in2.IngestedAt) {
		t.Fatalf("zone round trip: %+v %v", out2, err)
	}
}

func TestAuditListInvalidCursorIsATypedBadRequest(t *testing.T) {
	setReadMode(t, "", "")
	enc := func(s string) string { return base64.RawURLEncoding.EncodeToString([]byte(s)) }
	cursors := map[string]string{
		"not base64":        "!!!not-base64!!!",
		"padded base64":     base64.URLEncoding.EncodeToString([]byte(`{"t":"2026-10-01T12:00:00Z","id":5}`)) + "=",
		"not json":          enc("this is not json"),
		"json array":        enc(`[1,2]`),
		"id zero":           enc(`{"t":"2026-10-01T12:00:00Z","id":0}`),
		"id negative":       enc(`{"t":"2026-10-01T12:00:00Z","id":-4}`),
		"id missing":        enc(`{"t":"2026-10-01T12:00:00Z"}`),
		"id as string":      enc(`{"t":"2026-10-01T12:00:00Z","id":"5"}`),
		"bad time":          enc(`{"t":"yesterday","id":5}`),
		"missing time":      enc(`{"id":5}`),
		"truncated payload": enc(`{"t":"2026-10-01T12:00:00Z","id":5`),
	}
	for name, cursor := range cursors {
		stub := &keysetStub{all: keysetRows(5)}
		router := keysetRouter(t, stub)
		recorder := scopedGet(router, auditURL+"&cursor="+cursor, "user-a", "admin")
		if recorder.Code != http.StatusBadRequest {
			t.Fatalf("%s: status %d, want 400: %s", name, recorder.Code, recorder.Body.String())
		}
		if code := envelopeErrorCode(t, recorder); code != "invalid_cursor" {
			t.Fatalf("%s: error code %q, want invalid_cursor", name, code)
		}
		failure, _ := decodeEnvelope(t, recorder)["error"].(map[string]any)
		if failure["message"] == "" || failure["details"] == nil {
			t.Fatalf("%s: typed envelope needs message and details: %v", name, failure)
		}
		if len(stub.auditFilters) != 0 {
			t.Fatalf("%s: a bad cursor must not reach the store", name)
		}
	}
}

func TestAuditListLimitHandling(t *testing.T) {
	setReadMode(t, "", "")
	cases := []struct {
		query     string
		wantLimit int // meta.limit
	}{
		{"", 100},
		{"&limit=25", 25},
		{"&limit=500", 500},
		{"&limit=501", 500},
		{"&limit=100000", 500},
		{"&limit=0", 100},
		{"&limit=-5", 100},
		{"&limit=abc", 100},
	}
	for _, tc := range cases {
		stub := &keysetStub{all: keysetRows(2)}
		router := keysetRouter(t, stub)
		page := fetchListPage(t, router, auditURL+tc.query, "admin")
		if page.status != http.StatusOK {
			t.Fatalf("%q: status %d", tc.query, page.status)
		}
		if page.meta["limit"] != float64(tc.wantLimit) {
			t.Fatalf("%q: meta.limit = %v, want %d", tc.query, page.meta["limit"], tc.wantLimit)
		}
		if got := stub.auditFilters[0].Limit; got != tc.wantLimit+1 {
			t.Fatalf("%q: store limit = %d, want %d (clamped page size + 1)", tc.query, got, tc.wantLimit+1)
		}
		if stub.auditFilters[0].Limit > store.MaxAuditListLimit+1 {
			t.Fatalf("%q: the store limit must never exceed MaxAuditListLimit+1", tc.query)
		}
	}
}

func TestAuditListForwardsFiltersAndCursorToTheStore(t *testing.T) {
	setReadMode(t, "", "")
	stub := &keysetStub{all: keysetRows(2)}
	router := keysetRouter(t, stub)
	cursor := encodeAuditCursor(auditCursor{IngestedAt: time.Date(2026, 10, 1, 11, 0, 0, 0, time.UTC), ID: 9})
	target := auditURL + "&plane=model&outcome=denied&subject=agent%3Aalpha&resource_id=res-1" +
		"&event=policy_check&user_id=user-z&cursor=" + cursor
	recorder := scopedGet(router, target, "user-a", "owner")
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if len(stub.auditFilters) != 1 {
		t.Fatalf("store calls: %d", len(stub.auditFilters))
	}
	f := stub.auditFilters[0]
	if f.OrgID != "org-a" || f.Plane != "model" || f.Outcome != "denied" || f.Subject != "agent:alpha" ||
		f.ResourceID != "res-1" || f.Event != "policy_check" || f.UserID != "user-z" {
		t.Fatalf("filters not forwarded: %+v", f)
	}
	if !f.HasCursor || f.CursorID != 9 || !f.CursorIngestedAt.Equal(time.Date(2026, 10, 1, 11, 0, 0, 0, time.UTC)) {
		t.Fatalf("cursor not forwarded: %+v", f)
	}
	// Without a cursor the keyset is off.
	stub = &keysetStub{}
	recorder = scopedGet(keysetRouter(t, stub), auditURL, "user-a", "owner")
	if recorder.Code != http.StatusOK || stub.auditFilters[0].HasCursor {
		t.Fatalf("no cursor must mean HasCursor=false: %+v", stub.auditFilters)
	}
}

func TestAuditListMemberSelfScopeNarrowsEveryFilter(t *testing.T) {
	setReadMode(t, "", "")
	for _, query := range []string{
		"&plane=control",
		"&plane=model&outcome=denied",
		"&subject=someone&resource_id=r-1",
		"&event=login&plane=frontend",
		"&limit=500&cursor=" + encodeAuditCursor(auditCursor{IngestedAt: time.Now(), ID: 3}),
	} {
		stub := &keysetStub{all: keysetRows(2)}
		recorder := scopedGet(keysetRouter(t, stub), auditURL+query, "user-a", "member")
		if recorder.Code != http.StatusOK {
			t.Fatalf("%q: status %d: %s", query, recorder.Code, recorder.Body.String())
		}
		if got := metaScope(t, recorder); got != scopeSelf {
			t.Fatalf("%q: scope %q, want self", query, got)
		}
		f := stub.auditFilters[0]
		if f.UserID != "user-a" {
			t.Fatalf("%q: a plane/outcome/subject filter must not widen past the caller: %+v", query, f)
		}
		if f.OrgID != "org-a" {
			t.Fatalf("%q: org pinned wrongly: %+v", query, f)
		}
	}

	// Naming a foreign user alongside any filter is still refused up front.
	stub := &keysetStub{all: keysetRows(2)}
	recorder := scopedGet(keysetRouter(t, stub), auditURL+"&plane=control&user_id=user-b", "user-a", "viewer")
	if recorder.Code != http.StatusForbidden || envelopeErrorCode(t, recorder) != "audit_scope_denied" || len(stub.auditFilters) != 0 {
		t.Fatalf("foreign user_id: status %d code %q calls %d", recorder.Code, envelopeErrorCode(t, recorder), len(stub.auditFilters))
	}
}

func TestAuditListOrgRedactedScopeBlanksSubjectAndResourceFilters(t *testing.T) {
	setReadMode(t, "", "")
	stub := &keysetStub{all: []store.AuditRow{
		{ID: 5, IngestedAt: time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC), Event: toolActionEvent, Plane: "model", Outcome: "ok",
			Subject: "secret-subject", ResourceID: "secret-res", UserID: "someone-else",
			Details: map[string]any{"tool": "search", "arguments": "leak"}},
	}}
	recorder := scopedGet(keysetRouter(t, stub),
		auditURL+"&event=tool_action&subject=probe&resource_id=probe&plane=model&outcome=ok", "user-a", "member")
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if got := metaScope(t, recorder); got != scopeOrgRedacted {
		t.Fatalf("scope %q, want org_redacted", got)
	}
	f := stub.auditFilters[0]
	if f.Subject != "" || f.ResourceID != "" {
		t.Fatalf("subject/resource_id filters must be blanked in redacted scope: %+v", f)
	}
	if f.UserID != "" || f.Event != toolActionEvent || f.Plane != "model" || f.Outcome != "ok" {
		t.Fatalf("plane/outcome/event stay, user_id stays empty: %+v", f)
	}
	body := recorder.Body.String()
	for _, leaked := range []string{"secret-subject", "secret-res", "someone-else", "leak"} {
		if strings.Contains(body, leaked) {
			t.Fatalf("redacted view leaked %q: %s", leaked, body)
		}
	}
}

// WS05-S4 added no export surface: the read API stays list-only, and the scope
// registry does not know an export scope.
func TestAuditHasNoExportRouteOrScope(t *testing.T) {
	setReadMode(t, "", "")
	stub := &keysetStub{all: keysetRows(1)}
	router := keysetRouter(t, stub)
	for _, path := range []string{"/v1/audit/export", "/v1/audit/export?org_id=org-a", "/v1/export", "/v1/audit/csv"} {
		for _, method := range []string{http.MethodGet, http.MethodPost} {
			request := httptest.NewRequest(method, path, nil)
			signScopedRead(request, "org-a", "user-a", "admin")
			recorder := httptest.NewRecorder()
			router.ServeHTTP(recorder, request)
			// An unrouted path has no required scope, so the auth layer refuses it
			// (403) before routing would answer 404/405. Either way: not served.
			switch recorder.Code {
			case http.StatusNotFound, http.StatusMethodNotAllowed, http.StatusForbidden:
			default:
				t.Fatalf("%s %s: status %d, want 403/404/405", method, path, recorder.Code)
			}
		}
	}
	if len(stub.auditFilters) != 0 {
		t.Fatal("an export probe must never reach the audit list")
	}

	registry := `[{"principal":"verevon-gateway","audience":"audit-core","token":"` + testGatewayToken +
		`","scopes":["audit:read:self","audit:export"]}]`
	if _, err := parseServiceCredentialRegistry(registry); err == nil {
		t.Fatal("audit:export must be an unsupported scope")
	}
}

func TestAuditListCursorEncodingIsOpaqueAndStable(t *testing.T) {
	cursor := encodeAuditCursor(auditCursor{IngestedAt: time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC), ID: 11})
	raw, err := base64.RawURLEncoding.DecodeString(cursor)
	if err != nil {
		t.Fatalf("cursor must be base64url without padding: %v", err)
	}
	var wire map[string]any
	if err := json.Unmarshal(raw, &wire); err != nil {
		t.Fatal(err)
	}
	if wire["id"] != float64(11) || strconv.Itoa(len(wire)) != "2" {
		t.Fatalf("cursor wire shape changed: %v", wire)
	}
}
