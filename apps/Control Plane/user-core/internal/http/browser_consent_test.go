package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
	"github.com/gin-gonic/gin"
)

// fakeBrowserConsentStore is an in-memory BrowserConsentStore keyed by user,
// which also records the audit events the handlers hand to the store.
type fakeBrowserConsentStore struct {
	mu     sync.Mutex
	rows   map[string]map[string]any
	audits []users.BrowserConsentAudit
}

func newFakeBrowserConsentStore() *fakeBrowserConsentStore {
	return &fakeBrowserConsentStore{rows: map[string]map[string]any{}}
}

func (f *fakeBrowserConsentStore) GetBrowserConsentSettings(_ context.Context, userID string) (*users.UserSettings, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	row, ok := f.rows[userID]
	if !ok {
		return nil, nil
	}
	return &users.UserSettings{UserID: userID, Category: users.BrowserConsentCategory, Settings: row}, nil
}

func (f *fakeBrowserConsentStore) MutateBrowserConsentSettings(_ context.Context, userID string, mutate users.BrowserConsentMutation) (*users.UserSettings, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	current := f.rows[userID]
	if current == nil {
		current = map[string]any{}
	}
	next, audit, err := mutate(current)
	if err != nil {
		return nil, err
	}
	// Round-trip through JSON like the JSONB column does.
	raw, _ := json.Marshal(next)
	stored := map[string]any{}
	_ = json.Unmarshal(raw, &stored)
	f.rows[userID] = stored
	if audit != nil {
		f.audits = append(f.audits, *audit)
	}
	return &users.UserSettings{UserID: userID, Category: users.BrowserConsentCategory, Settings: stored}, nil
}

func browserConsentTestRouter(enabled bool, store BrowserConsentStore, userID string) *gin.Engine {
	gin.SetMode(gin.TestMode)
	router := gin.New()
	router.Use(func(c *gin.Context) {
		if userID != "" {
			c.Set("user_id", userID)
		}
		c.Next()
	})
	h := &browserConsentHandlers{
		enabled: enabled,
		store:   store,
		now:     func() time.Time { return time.Date(2026, 11, 3, 12, 0, 0, 0, time.UTC) },
	}
	h.register(router.Group("/api/v1"))
	return router
}

func doBrowserConsent(t *testing.T, router *gin.Engine, method, path, body string) *httptest.ResponseRecorder {
	t.Helper()
	var req *http.Request
	if body != "" {
		req = httptest.NewRequest(method, path, strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
	} else {
		req = httptest.NewRequest(method, path, nil)
	}
	w := httptest.NewRecorder()
	router.ServeHTTP(w, req)
	return w
}

func browserConsentErrorCode(t *testing.T, w *httptest.ResponseRecorder) string {
	t.Helper()
	var body struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("error body does not decode: %v (%s)", err, w.Body.String())
	}
	return body.Error.Code
}

func browserConsentData(t *testing.T, w *httptest.ResponseRecorder) browserConsentDocument {
	t.Helper()
	var body struct {
		Data browserConsentDocument `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("data body does not decode: %v (%s)", err, w.Body.String())
	}
	return body.Data
}

func TestBrowserConsentFlagOffAnswers404(t *testing.T) {
	store := newFakeBrowserConsentStore()
	router := browserConsentTestRouter(false, store, "user-a")
	for _, tc := range []struct{ method, path, body string }{
		{http.MethodGet, "/api/v1/settings/browser-consent", ""},
		{http.MethodPut, "/api/v1/settings/browser-consent/origins", `{"startHost":"example.org","decision":"allow","scope":"always"}`},
		{http.MethodDelete, "/api/v1/settings/browser-consent/origins?startHost=example.org&scope=always", ""},
	} {
		w := doBrowserConsent(t, router, tc.method, tc.path, tc.body)
		if w.Code != http.StatusNotFound || browserConsentErrorCode(t, w) != "browser_consent_disabled" {
			t.Fatalf("%s %s with the flag off: got %d %s", tc.method, tc.path, w.Code, w.Body.String())
		}
	}
	if len(store.rows) != 0 || len(store.audits) != 0 {
		t.Fatalf("flag off must not touch the store: rows=%v audits=%v", store.rows, store.audits)
	}
}

func TestBrowserConsentFlagDefaultsOff(t *testing.T) {
	t.Setenv(browserConsentFlagEnv, "")
	if browserConsentEnabledFromEnv() {
		t.Fatal("USER_CORE_BROWSER_CONSENT unset must leave the routes off")
	}
	t.Setenv(browserConsentFlagEnv, "true")
	if !browserConsentEnabledFromEnv() {
		t.Fatal("USER_CORE_BROWSER_CONSENT=true must turn the routes on")
	}
}

func TestBrowserConsentRequiresAuthenticatedPerson(t *testing.T) {
	router := browserConsentTestRouter(true, newFakeBrowserConsentStore(), "")
	w := doBrowserConsent(t, router, http.MethodGet, "/api/v1/settings/browser-consent", "")
	if w.Code != http.StatusUnauthorized {
		t.Fatalf("no identity: got %d %s", w.Code, w.Body.String())
	}
}

func TestBrowserConsentUnwiredStoreAnswers503(t *testing.T) {
	router := browserConsentTestRouter(true, nil, "user-a")
	w := doBrowserConsent(t, router, http.MethodGet, "/api/v1/settings/browser-consent", "")
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired store: got %d %s", w.Code, w.Body.String())
	}
}

func TestBrowserConsentPutGetRevokeRoundTrip(t *testing.T) {
	store := newFakeBrowserConsentStore()
	router := browserConsentTestRouter(true, store, "user-a")

	w := doBrowserConsent(t, router, http.MethodGet, "/api/v1/settings/browser-consent", "")
	if w.Code != http.StatusOK {
		t.Fatalf("empty GET: %d %s", w.Code, w.Body.String())
	}
	if doc := browserConsentData(t, w); doc.SchemaVersion != 1 || len(doc.Origins) != 0 {
		t.Fatalf("empty GET returned %+v", doc)
	}

	w = doBrowserConsent(t, router, http.MethodPut, "/api/v1/settings/browser-consent/origins",
		`{"startHost":"Docs.Example.ORG.","hosts":["cdn.example.org","docs.example.org","CDN.example.org"],"decision":"allow","scope":"conversation","conversationId":"conv_1"}`)
	if w.Code != http.StatusOK {
		t.Fatalf("PUT: %d %s", w.Code, w.Body.String())
	}
	doc := browserConsentData(t, w)
	if len(doc.Origins) != 1 {
		t.Fatalf("PUT stored %d decisions", len(doc.Origins))
	}
	got := doc.Origins[0]
	if got.StartHost != "docs.example.org" || len(got.Hosts) != 1 || got.Hosts[0] != "cdn.example.org" {
		t.Fatalf("hosts not canonicalised and de-duplicated: %+v", got)
	}
	if got.Kind != "origin" || got.Scope != "conversation" || got.ConversationID != "conv_1" || got.Decision != "allow" {
		t.Fatalf("decision fields: %+v", got)
	}

	// Same key replaces, a different scope adds.
	w = doBrowserConsent(t, router, http.MethodPut, "/api/v1/settings/browser-consent/origins",
		`{"startHost":"docs.example.org","decision":"deny","scope":"conversation","conversationId":"conv_1"}`)
	if w.Code != http.StatusOK || len(browserConsentData(t, w).Origins) != 1 {
		t.Fatalf("same-key PUT must replace: %d %s", w.Code, w.Body.String())
	}
	w = doBrowserConsent(t, router, http.MethodPut, "/api/v1/settings/browser-consent/origins",
		`{"startHost":"docs.example.org","decision":"allow","scope":"always"}`)
	if w.Code != http.StatusOK || len(browserConsentData(t, w).Origins) != 2 {
		t.Fatalf("new-scope PUT must add: %d %s", w.Code, w.Body.String())
	}

	w = doBrowserConsent(t, router, http.MethodDelete, "/api/v1/settings/browser-consent/origins?startHost=docs.example.org&scope=always", "")
	if w.Code != http.StatusOK || len(browserConsentData(t, w).Origins) != 1 {
		t.Fatalf("revoke: %d %s", w.Code, w.Body.String())
	}
	w = doBrowserConsent(t, router, http.MethodDelete, "/api/v1/settings/browser-consent/origins?startHost=docs.example.org&scope=always", "")
	if w.Code != http.StatusNotFound || browserConsentErrorCode(t, w) != "browser_consent_not_found" {
		t.Fatalf("second revoke: %d %s", w.Code, w.Body.String())
	}

	if len(store.audits) != 4 {
		t.Fatalf("every accepted change is audited exactly once: got %d", len(store.audits))
	}
	actions := []string{store.audits[0].Action, store.audits[1].Action, store.audits[2].Action, store.audits[3].Action}
	want := []string{browserConsentAuditDecided, browserConsentAuditDecided, browserConsentAuditDecided, browserConsentAuditRevoked}
	for i := range want {
		if actions[i] != want[i] {
			t.Fatalf("audit actions %v, want %v", actions, want)
		}
	}
	if replaced, _ := store.audits[1].Details["replaced"].(bool); !replaced {
		t.Fatalf("the replacing PUT must audit replaced=true: %v", store.audits[1].Details)
	}
	for _, audit := range store.audits {
		raw, _ := json.Marshal(audit.Details)
		if strings.Contains(string(raw), "example.org") {
			t.Fatalf("audit details must carry host digests, never host names: %s", raw)
		}
	}
}

func TestBrowserConsentIsPerPerson(t *testing.T) {
	store := newFakeBrowserConsentStore()
	a := browserConsentTestRouter(true, store, "user-a")
	b := browserConsentTestRouter(true, store, "user-b")
	if w := doBrowserConsent(t, a, http.MethodPut, "/api/v1/settings/browser-consent/origins",
		`{"startHost":"example.org","decision":"allow","scope":"always"}`); w.Code != http.StatusOK {
		t.Fatalf("PUT as a: %d %s", w.Code, w.Body.String())
	}
	w := doBrowserConsent(t, b, http.MethodGet, "/api/v1/settings/browser-consent", "")
	if w.Code != http.StatusOK || len(browserConsentData(t, w).Origins) != 0 {
		t.Fatalf("b must not see a's decisions: %s", w.Body.String())
	}
}

func TestBrowserConsentRejectsInvalidBodies(t *testing.T) {
	router := browserConsentTestRouter(true, newFakeBrowserConsentStore(), "user-a")
	nine := `["h1.example.org","h2.example.org","h3.example.org","h4.example.org","h5.example.org","h6.example.org","h7.example.org","h8.example.org","h9.example.org"]`
	for name, tc := range map[string]struct {
		body string
		code string
	}{
		"nine listed hosts":        {`{"startHost":"example.org","hosts":` + nine + `,"decision":"allow","scope":"always"}`, "too_many_hosts"},
		"ip literal start":         {`{"startHost":"10.0.0.1","decision":"allow","scope":"always"}`, "invalid_host"},
		"ipv6 literal start":       {`{"startHost":"[::1]","decision":"allow","scope":"always"}`, "invalid_host"},
		"localhost":                {`{"startHost":"localhost","decision":"allow","scope":"always"}`, "invalid_host"},
		"internal name":            {`{"startHost":"wiki.corp.internal","decision":"allow","scope":"always"}`, "invalid_host"},
		"url not host":             {`{"startHost":"https://example.org/path","decision":"allow","scope":"always"}`, "invalid_host"},
		"port":                     {`{"startHost":"example.org:8443","decision":"allow","scope":"always"}`, "invalid_host"},
		"wildcard":                 {`{"startHost":"*.example.org","decision":"allow","scope":"always"}`, "invalid_host"},
		"unicode not punycode":     {`{"startHost":"bücher.de","decision":"allow","scope":"always"}`, "invalid_host"},
		"bad listed host":          {`{"startHost":"example.org","hosts":["192.168.1.1"],"decision":"allow","scope":"always"}`, "invalid_host"},
		"unknown scope":            {`{"startHost":"example.org","decision":"allow","scope":"forever"}`, "invalid_scope"},
		"turn without turnId":      {`{"startHost":"example.org","decision":"allow","scope":"turn"}`, "invalid_scope_binding"},
		"conversation without id":  {`{"startHost":"example.org","decision":"allow","scope":"conversation"}`, "invalid_scope_binding"},
		"always with conversation": {`{"startHost":"example.org","decision":"allow","scope":"always","conversationId":"c1"}`, "invalid_scope_binding"},
		"unknown decision":         {`{"startHost":"example.org","decision":"maybe","scope":"always"}`, "invalid_decision"},
		"non-origin kind":          {`{"kind":"download","startHost":"example.org","decision":"allow","scope":"always"}`, "invalid_kind"},
		"deny with hosts":          {`{"startHost":"example.org","hosts":["cdn.example.org"],"decision":"deny","scope":"always"}`, "invalid_hosts"},
	} {
		w := doBrowserConsent(t, router, http.MethodPut, "/api/v1/settings/browser-consent/origins", tc.body)
		if w.Code != http.StatusUnprocessableEntity || browserConsentErrorCode(t, w) != tc.code {
			t.Errorf("%s: got %d %s, want 422 %s", name, w.Code, w.Body.String(), tc.code)
		}
	}
}

func TestBrowserConsentEightListedHostsPlusStartHostIsAccepted(t *testing.T) {
	hosts := make([]string, 0, 9)
	for _, h := range []string{"a", "b", "c", "d", "e", "f", "g", "h"} {
		hosts = append(hosts, h+".example.org")
	}
	// The start host repeated in the list does not count toward the 8.
	hosts = append(hosts, "example.org")
	decision, verr := validateBrowserConsentDecision(browserConsentDecisionRequest{
		StartHost: "example.org", Hosts: hosts, Decision: "allow", Scope: "turn", TurnID: "turn_1",
	}, time.Now())
	if verr != nil {
		t.Fatalf("8 listed hosts plus the start host must pass: %v", verr)
	}
	if len(decision.Hosts) != browserConsentMaxListedHosts {
		t.Fatalf("listed hosts: %v", decision.Hosts)
	}
}

func TestCanonicalBrowserConsentHost(t *testing.T) {
	for raw, want := range map[string]string{
		"Example.ORG":        "example.org",
		"example.org.":       "example.org",
		" docs.example.org ": "docs.example.org",
		"xn--bcher-kva.de":   "xn--bcher-kva.de",
		"a-b.example.co.uk":  "a-b.example.co.uk",
	} {
		got, verr := canonicalBrowserConsentHost(raw)
		if verr != nil || got != want {
			t.Errorf("canonicalBrowserConsentHost(%q) = %q, %v; want %q", raw, got, verr, want)
		}
	}
	for _, raw := range []string{"", "org", "-bad.example.org", "bad-.example.org", "a..example.org", "1.2.3.4", "host.123", "printer.local", "router.home.arpa", "user@example.org", strings.Repeat("a", 64) + ".example.org"} {
		if _, verr := canonicalBrowserConsentHost(raw); verr == nil {
			t.Errorf("canonicalBrowserConsentHost(%q) must refuse", raw)
		}
	}
}

func TestBrowserConsentEvictionKeepsAlwaysDecisions(t *testing.T) {
	base := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	always := func(i int) browserConsentDecision {
		return browserConsentDecision{Kind: "origin", StartHost: "always" + strconv.Itoa(i) + ".example.org", Scope: "always", Decision: "allow", DecidedAt: base, Hosts: []string{}}
	}

	doc := browserConsentDocument{SchemaVersion: 1}
	for i := 0; i < browserConsentMaxDecisions-2; i++ {
		doc.Origins = append(doc.Origins, always(i))
	}
	doc.Origins = append(doc.Origins,
		browserConsentDecision{Kind: "origin", StartHost: "old.example.org", Scope: "turn", TurnID: "t_old", Decision: "allow", DecidedAt: base, Hosts: []string{}},
		browserConsentDecision{Kind: "origin", StartHost: "conv.example.org", Scope: "conversation", ConversationID: "c1", Decision: "allow", DecidedAt: base.Add(-time.Hour), Hosts: []string{}},
	)
	next, _, err := upsertBrowserConsentDecision(doc, browserConsentDecision{Kind: "origin", StartHost: "new.example.org", Scope: "always", Decision: "allow", DecidedAt: base.Add(time.Hour), Hosts: []string{}})
	if err != nil {
		t.Fatalf("a full document with a turn entry must evict it: %v", err)
	}
	if len(next.Origins) != browserConsentMaxDecisions {
		t.Fatalf("size after eviction: %d", len(next.Origins))
	}
	sawConversation := false
	for _, d := range next.Origins {
		if d.StartHost == "old.example.org" {
			t.Fatal("the turn-scoped decision must be evicted before the older conversation-scoped one")
		}
		if d.StartHost == "conv.example.org" {
			sawConversation = true
		}
	}
	if !sawConversation {
		t.Fatal("the conversation-scoped decision must survive while a turn-scoped one can go")
	}

	full := browserConsentDocument{SchemaVersion: 1}
	for i := 0; i < browserConsentMaxDecisions; i++ {
		full.Origins = append(full.Origins, always(i))
	}
	_, _, err = upsertBrowserConsentDecision(full, browserConsentDecision{Kind: "origin", StartHost: "new.example.org", Scope: "always", Decision: "allow", DecidedAt: base, Hosts: []string{}})
	typed, ok := err.(*browserConsentError)
	if !ok || typed.code != "browser_consent_limit" {
		t.Fatalf("a document full of always decisions must refuse a new one with browser_consent_limit: %v", err)
	}
}

// The RLS migration must keep the unscoped branch: GDPR anonymize deletes
// user_settings by user_id without setting app.current_user_id, and the hard
// delete cascades from users. Dropping that branch would make erasure skip
// browser_consent rows silently.
func TestBrowserConsentMigrationKeepsErasurePath(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "..", "migrations", "044_user_settings_browser_consent_rls.up.sql"))
	if err != nil {
		t.Fatalf("read migration 044: %v", err)
	}
	sql := string(raw)
	for _, want := range []string{
		"ALTER TABLE user_settings ENABLE ROW LEVEL SECURITY",
		"ALTER TABLE user_settings FORCE ROW LEVEL SECURITY",
		"CREATE POLICY user_settings_user_scope ON user_settings",
		"COALESCE(current_setting('app.current_user_id', true), '') = ''",
		"user_id = current_setting('app.current_user_id', true)",
		"WITH CHECK",
	} {
		if !strings.Contains(sql, want) {
			t.Fatalf("migration 044 lost %q", want)
		}
	}
	down, err := os.ReadFile(filepath.Join("..", "..", "migrations", "044_user_settings_browser_consent_rls.down.sql"))
	if err != nil {
		t.Fatalf("read migration 044 down: %v", err)
	}
	if !strings.Contains(string(down), "DROP POLICY IF EXISTS user_settings_user_scope ON user_settings") {
		t.Fatal("migration 044 down must drop the policy")
	}
}

// Both erasure modes must still remove the category: anonymize deletes
// user_settings by user_id, hard delete removes the users row (FK cascade).
func TestBrowserConsentCoveredByGDPRErasure(t *testing.T) {
	raw, err := os.ReadFile(filepath.Join("..", "users", "gdpr_erasure_store.go"))
	if err != nil {
		t.Fatalf("read erasure store: %v", err)
	}
	src := string(raw)
	for _, want := range []string{
		"DELETE FROM user_settings WHERE user_id = $1",
		"DELETE FROM users WHERE id = $1",
	} {
		if !strings.Contains(src, want) {
			t.Fatalf("GDPR erasure no longer runs %q; browser_consent rows would survive erasure", want)
		}
	}
	if strings.Contains(src, "category <> 'browser_consent'") || strings.Contains(src, "category != 'browser_consent'") {
		t.Fatal("GDPR erasure must not exclude the browser_consent category")
	}
}
