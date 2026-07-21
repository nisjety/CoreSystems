package emailsync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// ── fakes ────────────────────────────────────────────────────────────────────

type fakeStore struct {
	connections      []store.Connection
	states           map[string]store.EmailSyncState
	upserts          []store.EmailSyncState
	contextUpdates   []map[string]string
	contextUpdateErr error
}

func (f *fakeStore) ListConnections(_ context.Context, filter store.ConnectionFilter) ([]store.Connection, error) {
	var out []store.Connection
	for _, c := range f.connections {
		if filter.ProviderKey == "" || c.ProviderKey == filter.ProviderKey {
			out = append(out, c)
		}
	}
	return out, nil
}

func (f *fakeStore) GetEmailSyncState(_ context.Context, connectionID string) (store.EmailSyncState, error) {
	if state, ok := f.states[connectionID]; ok {
		return state, nil
	}
	return store.EmailSyncState{}, store.ErrNotFound
}

func (f *fakeStore) UpsertEmailSyncState(_ context.Context, state store.EmailSyncState) error {
	if f.states == nil {
		f.states = map[string]store.EmailSyncState{}
	}
	f.states[state.ConnectionID] = state
	f.upserts = append(f.upserts, state)
	return nil
}

func (f *fakeStore) BindConnectionProviderContext(_ context.Context, id, key, value string) (store.Connection, error) {
	if f.contextUpdateErr != nil {
		return store.Connection{}, f.contextUpdateErr
	}
	for index, connection := range f.connections {
		if connection.ID != id {
			continue
		}
		if connection.DeletedAt != nil || (connection.Status != "active" && connection.Status != "needs_refresh") {
			return store.Connection{}, store.ErrConflict
		}
		if existing := connection.ProviderContext[key]; existing != "" && existing != value {
			return store.Connection{}, store.ErrConflict
		}
		update := make(map[string]string, len(connection.ProviderContext)+1)
		for currentKey, currentValue := range connection.ProviderContext {
			update[currentKey] = currentValue
		}
		update[key] = value
		f.contextUpdates = append(f.contextUpdates, update)
		connection.ProviderContext = update
		f.connections[index] = connection
		return connection, nil
	}
	return store.Connection{}, store.ErrNotFound
}

type fakeTokens struct{ err error }

func (f fakeTokens) AccessTokenForConnection(_ context.Context, connectionID string) (oauth.AccessTokenResult, error) {
	if f.err != nil {
		return oauth.AccessTokenResult{}, f.err
	}
	return oauth.AccessTokenResult{ConnectionID: connectionID, AccessToken: "tok-" + connectionID}, nil
}

type fakeFetcher struct {
	calls     []string // cursors observed
	backfills []time.Duration
	results   map[string]FetchResult
	errs      map[string]error
}

type versionedFakeFetcher struct {
	*fakeFetcher
	version string
}

func (f *versionedFakeFetcher) CursorVersion() string { return f.version }

func (f *fakeFetcher) Fetch(_ context.Context, _ string, cursor string, backfill time.Duration, _ int) (FetchResult, error) {
	f.calls = append(f.calls, cursor)
	f.backfills = append(f.backfills, backfill)
	if err, ok := f.errs[cursor]; ok {
		return FetchResult{}, err
	}
	return f.results[cursor], nil
}

func TestRunOnce_TeamsConsumesDurableHistoryExtension(t *testing.T) {
	conn := store.Connection{
		ID: "conn-ms", ProviderKey: "microsoft", OrganizationID: "org-1", UserID: "user-1",
		Status: "active", Capabilities: []string{"teams.messages.read"},
	}
	st := &fakeStore{
		connections: []store.Connection{conn},
		states: map[string]store.EmailSyncState{
			"conn-ms:teams": {
				ConnectionID: "conn-ms:teams", ProviderKey: "teams", Cursor: "teams-cursor",
				HistoryBackfillDays: 30,
			},
		},
	}
	fetcher := &fakeFetcher{results: map[string]FetchResult{"teams-cursor": {NextCursor: "teams-next"}}}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}, Teams: fetcher}

	if _, err := w.RunOnce(t.Context()); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if len(fetcher.backfills) != 1 || fetcher.backfills[0] != 60*24*time.Hour {
		t.Fatalf("Teams backfills = %v, want one 60-day request", fetcher.backfills)
	}
	if got := st.states["conn-ms:teams"].HistoryBackfillDays; got != 30 {
		t.Fatalf("persisted additional history days = %d, want 30", got)
	}
}

type fakeIngestor struct {
	events  []EmailMessage
	conns   []store.Connection
	failOn  string // ProviderEventID that fails
	failErr error
}

func (f *fakeIngestor) Ingest(_ context.Context, conn store.Connection, msg EmailMessage) error {
	if f.failOn != "" && msg.ProviderEventID == f.failOn {
		if f.failErr == nil {
			f.failErr = errors.New("ingest failed")
		}
		return f.failErr
	}
	f.events = append(f.events, msg)
	f.conns = append(f.conns, conn)
	return nil
}

func googleConnection(id string) store.Connection {
	return store.Connection{
		ID: id, ProviderKey: "google", OrganizationID: "org_1", UserEmail: "owner@example.com",
		Status: "active", Capabilities: []string{"gmail.read", "gmail.send"},
	}
}

// ── worker tests ─────────────────────────────────────────────────────────────

func TestRunOnce_IngestsAndAdvancesCursor(t *testing.T) {
	st := &fakeStore{connections: []store.Connection{googleConnection("conn_1")}}
	fetcher := &fakeFetcher{results: map[string]FetchResult{
		"": {
			Messages: []EmailMessage{
				{ProviderEventID: "m1", From: Participant{Email: "customer@x.no"}, BodyText: "hei"},
				{ProviderEventID: "m2", From: Participant{Email: "owner@example.com"}, BodyText: "self echo"},
			},
			NextCursor: "hist-100",
		},
	}}
	ing := &fakeIngestor{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Gmail: fetcher, Graph: &fakeFetcher{}}

	n, err := w.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if n != 1 {
		t.Fatalf("ingested = %d, want 1 (self-echo skipped)", n)
	}
	if len(ing.events) != 1 || ing.events[0].ProviderEventID != "m1" {
		t.Fatalf("unexpected ingested events: %+v", ing.events)
	}
	state := st.states["conn_1"]
	if state.Cursor != "hist-100" || state.LastError != "" || state.FailureCount != 0 {
		t.Fatalf("state not advanced cleanly: %+v", state)
	}
}

func TestRunOnce_PersistsProviderContextPatchBeforeIngest(t *testing.T) {
	conn := planConnection("conn_dc", "discord", "messages.read")
	conn.ProviderContext = map[string]string{"installation_id": "install-1"}
	st := &fakeStore{connections: []store.Connection{conn}}
	fetcher := &fakeFetcher{results: map[string]FetchResult{
		"": {
			Messages:             []EmailMessage{chatMessage("m1")},
			NextCursor:           "cur-1",
			ProviderContextPatch: map[string]string{"guild_id": "g1"},
		},
	}}
	ing := &fakeIngestor{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Discord: fetcher}

	n, err := w.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if n != 1 || len(ing.conns) != 1 {
		t.Fatalf("ingested = %d, connections = %d, want one delivery", n, len(ing.conns))
	}
	if len(st.contextUpdates) != 1 {
		t.Fatalf("provider context updates = %d, want 1", len(st.contextUpdates))
	}
	if got := st.contextUpdates[0]["installation_id"]; got != "install-1" {
		t.Errorf("existing provider context was not preserved: %v", st.contextUpdates[0])
	}
	if got := ing.conns[0].ProviderContext["guild_id"]; got != "g1" {
		t.Errorf("delivered guild_id = %q, want persisted binding", got)
	}
}

func TestRunOnce_ContextPatchFailureStopsIngestAndCursorAdvance(t *testing.T) {
	st := &fakeStore{
		connections:      []store.Connection{planConnection("conn_dc", "discord", "messages.read")},
		contextUpdateErr: errors.New("database unavailable"),
	}
	fetcher := &fakeFetcher{results: map[string]FetchResult{
		"": {
			Messages:             []EmailMessage{chatMessage("m1")},
			NextCursor:           "cur-1",
			ProviderContextPatch: map[string]string{"guild_id": "g1"},
		},
	}}
	ing := &fakeIngestor{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Discord: fetcher}

	n, err := w.RunOnce(context.Background())
	if err == nil || !strings.Contains(err.Error(), "persist provider context") {
		t.Fatalf("RunOnce error = %v, want provider context persistence failure", err)
	}
	if n != 0 || len(ing.events) != 0 {
		t.Fatalf("ingested = %d, events = %d, want no delivery before durable binding", n, len(ing.events))
	}
	if got := st.states["conn_dc:discord"].Cursor; got != "" {
		t.Fatalf("cursor = %q, want unchanged", got)
	}
}

func TestRunOnce_SkipsIneligibleConnections(t *testing.T) {
	deleted := googleConnection("conn_deleted")
	now := time.Now()
	deleted.DeletedAt = &now
	inactive := googleConnection("conn_inactive")
	inactive.Status = "error"
	noCap := googleConnection("conn_nocap")
	noCap.Capabilities = []string{"drive.read"}
	noCap.Scopes = []string{"https://www.googleapis.com/auth/drive.readonly"}

	st := &fakeStore{connections: []store.Connection{deleted, inactive, noCap}}
	fetcher := &fakeFetcher{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}, Gmail: fetcher, Graph: &fakeFetcher{}}

	if _, err := w.RunOnce(context.Background()); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if len(fetcher.calls) != 0 {
		t.Fatalf("fetcher should never run for ineligible connections, calls=%v", fetcher.calls)
	}
}

func TestRunOnce_ScopeFallbackAdmitsLegacyConnections(t *testing.T) {
	legacy := googleConnection("conn_legacy")
	legacy.Capabilities = nil
	legacy.Scopes = []string{"https://www.googleapis.com/auth/GMAIL.READONLY"}
	st := &fakeStore{connections: []store.Connection{legacy}}
	fetcher := &fakeFetcher{results: map[string]FetchResult{"": {NextCursor: "h1"}}}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}, Gmail: fetcher, Graph: &fakeFetcher{}}

	if _, err := w.RunOnce(context.Background()); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if len(fetcher.calls) != 1 {
		t.Fatalf("legacy scope connection should sync, calls=%v", fetcher.calls)
	}
}

func TestRunOnce_IngestFailureDoesNotAdvanceCursor(t *testing.T) {
	st := &fakeStore{
		connections: []store.Connection{googleConnection("conn_1")},
		states: map[string]store.EmailSyncState{
			"conn_1": {ConnectionID: "conn_1", ProviderKey: "google", Cursor: "hist-50"},
		},
	}
	fetcher := &fakeFetcher{results: map[string]FetchResult{
		"hist-50": {
			Messages: []EmailMessage{
				{ProviderEventID: "ok", From: Participant{Email: "a@x.no"}, BodyText: "fine"},
				{ProviderEventID: "boom", From: Participant{Email: "b@x.no"}, BodyText: "fails"},
			},
			NextCursor: "hist-99",
		},
	}}
	ing := &fakeIngestor{failOn: "boom"}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Gmail: fetcher, Graph: &fakeFetcher{}}

	_, err := w.RunOnce(context.Background())
	if err == nil {
		t.Fatal("expected the ingest failure to surface")
	}
	state := st.states["conn_1"]
	if state.Cursor != "hist-50" {
		t.Fatalf("cursor must NOT advance past a failed ingest, got %q", state.Cursor)
	}
	if state.FailureCount != 1 || state.LastError == "" {
		t.Fatalf("failure not recorded: %+v", state)
	}
}

func TestRunOnce_CursorExpiredRebootstraps(t *testing.T) {
	st := &fakeStore{
		connections: []store.Connection{googleConnection("conn_1")},
		states: map[string]store.EmailSyncState{
			"conn_1": {ConnectionID: "conn_1", ProviderKey: "google", Cursor: "stale"},
		},
	}
	fetcher := &fakeFetcher{
		errs: map[string]error{"stale": fmt.Errorf("wrap: %w", ErrCursorExpired)},
		results: map[string]FetchResult{
			"": {Messages: []EmailMessage{{ProviderEventID: "m1", From: Participant{Email: "c@x.no"}, BodyText: "hi"}}, NextCursor: "fresh"},
		},
	}
	ing := &fakeIngestor{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Gmail: fetcher, Graph: &fakeFetcher{}}

	n, err := w.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if n != 1 {
		t.Fatalf("ingested = %d, want 1 after re-bootstrap", n)
	}
	if len(fetcher.calls) != 2 || fetcher.calls[0] != "stale" || fetcher.calls[1] != "" {
		t.Fatalf("expected stale call then bootstrap call, got %v", fetcher.calls)
	}
	if st.states["conn_1"].Cursor != "fresh" {
		t.Fatalf("cursor = %q, want fresh", st.states["conn_1"].Cursor)
	}
}

func TestRunOnce_VersionedFetcherResetsLegacyCursorOnce(t *testing.T) {
	conn := store.Connection{
		ID: "conn_ms", ProviderKey: "microsoft", OrganizationID: "org_1",
		Status: "active", Capabilities: []string{"mail.read"},
	}
	st := &fakeStore{
		connections: []store.Connection{conn},
		states: map[string]store.EmailSyncState{
			"conn_ms": {ConnectionID: "conn_ms", ProviderKey: "microsoft", Cursor: "legacy-filtered-delta"},
		},
	}
	base := &fakeFetcher{results: map[string]FetchResult{
		"":           {NextCursor: "full-delta"},
		"full-delta": {NextCursor: "next-delta"},
	}}
	fetcher := &versionedFakeFetcher{fakeFetcher: base, version: graphFullBackfillCursorVersion}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}, Graph: fetcher}

	if _, err := w.RunOnce(context.Background()); err != nil {
		t.Fatalf("first RunOnce: %v", err)
	}
	if got := base.calls; len(got) != 1 || got[0] != "" {
		t.Fatalf("first cursors = %v, want one full bootstrap", got)
	}
	wantStored := graphFullBackfillCursorVersion + cursorVersionSeparator + "full-delta"
	if got := st.states["conn_ms"].Cursor; got != wantStored {
		t.Fatalf("stored cursor = %q, want %q", got, wantStored)
	}

	if _, err := w.RunOnce(context.Background()); err != nil {
		t.Fatalf("second RunOnce: %v", err)
	}
	if got := base.calls; len(got) != 2 || got[1] != "full-delta" {
		t.Fatalf("second cursors = %v, want decoded versioned delta", got)
	}
}

func TestDecodeFetcherCursorResetsVersionedCursorWhenModeIsDisabled(t *testing.T) {
	fetcher := &versionedFakeFetcher{fakeFetcher: &fakeFetcher{}, version: ""}
	stored := graphFullBackfillCursorVersion + cursorVersionSeparator + "https://graph.example/delta"

	if got := decodeFetcherCursor(fetcher, stored); got != "" {
		t.Fatalf("decoded cursor = %q, want reset for disabled versioned mode", got)
	}
}

func TestRunOnce_TokenFailureIsRecordedAndIsolated(t *testing.T) {
	st := &fakeStore{connections: []store.Connection{googleConnection("conn_1")}}
	w := Worker{Store: st, Tokens: fakeTokens{err: errors.New("refresh exploded")}, Ingest: &fakeIngestor{}, Gmail: &fakeFetcher{}, Graph: &fakeFetcher{}}

	_, err := w.RunOnce(context.Background())
	if err == nil {
		t.Fatal("expected token failure to surface")
	}
	state := st.states["conn_1"]
	if state.FailureCount != 1 || state.LastError == "" {
		t.Fatalf("token failure not recorded on state: %+v", state)
	}
}

// ── ingest client tests ──────────────────────────────────────────────────────

func TestIngestClient_PostsBridgeShape(t *testing.T) {
	var got map[string]any
	var gotHeaders http.Header
	var gotBody []byte
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotHeaders = r.Header.Clone()
		var err error
		gotBody, err = io.ReadAll(r.Body)
		if err != nil {
			t.Errorf("read body: %v", err)
		}
		if err := json.Unmarshal(gotBody, &got); err != nil {
			t.Errorf("decode body: %v", err)
		}
		w.WriteHeader(http.StatusAccepted)
		_, _ = w.Write([]byte(`{"data":{"detail":{"id":"conv-1"},"message":{"id":"msg-1"},"created":true}}`))
	}))
	defer server.Close()

	client := &IngestClient{
		BaseURL:      server.URL,
		ServiceToken: "0123456789abcdef0123456789abcdef",
		HTTP:         server.Client(),
		Now: func() time.Time {
			return time.Date(2026, 7, 13, 12, 0, 0, 0, time.UTC)
		},
		Nonce: func() string { return "fixed-nonce-1234567890" },
	}
	conn := googleConnection("conn_9")
	msg := EmailMessage{
		ProviderEventID:   "evt-1",
		ProviderMessageID: "msg-1",
		ProviderThreadID:  "thread-1",
		MessageIDHeader:   "<m@x>",
		Subject:           "Hei",
		From:              Participant{Name: "Kari", Email: "kari@x.no"},
		To:                []Participant{{Email: "support@velion.no"}},
		BodyText:          "Trenger hjelp",
		OccurredAt:        time.Date(2026, 7, 8, 10, 0, 0, 0, time.UTC),
	}
	if err := client.Ingest(context.Background(), conn, msg); err != nil {
		t.Fatalf("Ingest: %v", err)
	}
	if gotHeaders.Get("x-internal-api-key") != "" {
		t.Fatal("legacy internal key must not be sent")
	}
	if gotHeaders.Get("x-service-id") != "integration-email-worker" || gotHeaders.Get("x-org-id") != "org_1" {
		t.Fatalf("delegated service/org headers = %q/%q", gotHeaders.Get("x-service-id"), gotHeaders.Get("x-org-id"))
	}
	if gotHeaders.Get("x-delegation-timestamp") != "2026-07-13T12:00:00Z" || gotHeaders.Get("x-delegation-nonce") != "fixed-nonce-1234567890" {
		t.Fatalf("delegation time/nonce = %q/%q", gotHeaders.Get("x-delegation-timestamp"), gotHeaders.Get("x-delegation-nonce"))
	}
	if gotHeaders.Get("x-delegation-body-sha256") != bodyDigest(gotBody) {
		t.Fatal("delegation digest does not bind transmitted bytes")
	}
	if gotHeaders.Get("x-delegation-signature") == "" {
		t.Fatal("delegation signature is required")
	}
	for key, want := range map[string]string{
		"org_id":              "org_1",
		"connection_id":       "conn_9",
		"provider":            "google",
		"provider_event_id":   "evt-1",
		"provider_message_id": "msg-1",
		"provider_thread_id":  "thread-1",
		"direction":           "inbound",
		"subject":             "Hei",
		"body_text":           "Trenger hjelp",
	} {
		if got[key] != want {
			t.Errorf("payload[%s] = %v, want %s", key, got[key], want)
		}
	}
}

func TestConversationIngestDelegationMatchesRustFixture(t *testing.T) {
	body := []byte(`{"org_id":"org-1"}`)
	headers := conversationIngestDelegationHeaders(
		"0123456789abcdef0123456789abcdef",
		http.MethodPost,
		"/internal/ingest/email",
		body,
		"org-1",
		time.Date(2026, time.July, 13, 12, 0, 0, 0, time.UTC),
		"fixed-nonce-1234567890",
	)
	if headers["x-delegation-body-sha256"] != "YqOazKjaPktfdxVznPrrhX7qEbel9X3ciCClxMerpjg" {
		t.Fatalf("body digest = %q, want fixed cross-language fixture", headers["x-delegation-body-sha256"])
	}
	if headers["x-delegation-signature"] != "y4DwDDy0g0WbPNWH49I5lOiUQkKZbFOIci0B28Bt2BI" {
		t.Fatalf("signature = %q, want Rust-compatible fixed fixture", headers["x-delegation-signature"])
	}
}

func TestIngestClient_Non2xxIsError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error":{"code":"validation_error"}}`, http.StatusUnprocessableEntity)
	}))
	defer server.Close()

	client := &IngestClient{BaseURL: server.URL, ServiceToken: "0123456789abcdef0123456789abcdef", HTTP: server.Client()}
	err := client.Ingest(context.Background(), googleConnection("c"), EmailMessage{ProviderEventID: "e"})
	if err == nil {
		t.Fatal("expected non-2xx to be an error")
	}
}

func TestIngestClient_RejectsNonContractSuccess(t *testing.T) {
	tests := []struct {
		name   string
		status int
		body   string
	}{
		{name: "wrong status", status: http.StatusOK, body: `{"data":{"detail":{"id":"conv-1"},"message":{"id":"msg-1"},"created":true}}`},
		{name: "empty accepted", status: http.StatusAccepted},
		{name: "fabricated ok", status: http.StatusAccepted, body: `{"data":{"ok":true}}`},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(test.status)
				_, _ = w.Write([]byte(test.body))
			}))
			defer server.Close()
			client := &IngestClient{BaseURL: server.URL, ServiceToken: "0123456789abcdef0123456789abcdef", HTTP: server.Client()}
			if err := client.Ingest(t.Context(), googleConnection("c"), EmailMessage{ProviderEventID: "e"}); err == nil {
				t.Fatal("Ingest() error = nil, want strict acknowledgement rejection")
			}
		})
	}
}
