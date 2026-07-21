package emailsync

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

// fakeConnFetcher records the connections handed to FetchConnection.
type fakeConnFetcher struct {
	fakeFetcher
	conns []store.Connection
}

func (f *fakeConnFetcher) FetchConnection(ctx context.Context, conn store.Connection, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	f.conns = append(f.conns, conn)
	return f.Fetch(ctx, accessToken, cursor, backfill, maxMessages)
}

func planConnection(id, providerKey string, capabilities ...string) store.Connection {
	return store.Connection{
		ID: id, ProviderKey: providerKey, OrganizationID: "org_1",
		Status: "active", Capabilities: capabilities,
	}
}

func chatMessage(id string) EmailMessage {
	return EmailMessage{ProviderEventID: id, From: Participant{Name: "Kari"}, BodyText: "hei"}
}

// TestRunOnce_PlanStateKeys locks the plan table's cursor-key contract:
// email plans keep the bare connection id (backward compatible with existing
// email_sync_state rows) while every chat plan gets its own suffixed row.
func TestRunOnce_PlanStateKeys(t *testing.T) {
	tests := []struct {
		name         string
		conn         store.Connection
		assign       func(w *Worker, f Fetcher)
		wantStateKey string
		wantProvider string
	}{
		{
			name:         "gmail keeps bare connection id",
			conn:         planConnection("conn_g", "google", "gmail.read"),
			assign:       func(w *Worker, f Fetcher) { w.Gmail = f },
			wantStateKey: "conn_g",
			wantProvider: "google",
		},
		{
			name:         "outlook keeps bare connection id",
			conn:         planConnection("conn_ms", "microsoft", "mail.read"),
			assign:       func(w *Worker, f Fetcher) { w.Graph = f },
			wantStateKey: "conn_ms",
			wantProvider: "microsoft",
		},
		{
			name:         "teams plan suffixes the state key",
			conn:         planConnection("conn_ms", "microsoft", "teams.messages.read"),
			assign:       func(w *Worker, f Fetcher) { w.Teams = f },
			wantStateKey: "conn_ms:teams",
			wantProvider: "teams",
		},
		{
			name:         "slack plan suffixes the state key",
			conn:         planConnection("conn_sl", "slack", "channels.history"),
			assign:       func(w *Worker, f Fetcher) { w.Slack = f },
			wantStateKey: "conn_sl:slack",
			wantProvider: "slack",
		},
		{
			name:         "x dm plan suffixes the state key",
			conn:         planConnection("conn_x", "x", "social.inbox.read"),
			assign:       func(w *Worker, f Fetcher) { w.XDM = f },
			wantStateKey: "conn_x:xdm",
			wantProvider: "x",
		},
		{
			name:         "discord plan suffixes the state key",
			conn:         planConnection("conn_dc", "discord", "messages.read"),
			assign:       func(w *Worker, f Fetcher) { w.Discord = f },
			wantStateKey: "conn_dc:discord",
			wantProvider: "discord",
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			st := &fakeStore{connections: []store.Connection{test.conn}}
			fetcher := &fakeFetcher{results: map[string]FetchResult{
				"": {Messages: []EmailMessage{chatMessage("m1")}, NextCursor: "cur-1"},
			}}
			w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}}
			test.assign(&w, fetcher)

			n, err := w.RunOnce(context.Background())
			if err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if n != 1 {
				t.Fatalf("ingested = %d, want 1", n)
			}
			state, ok := st.states[test.wantStateKey]
			if !ok {
				t.Fatalf("no sync state under key %q, states=%v", test.wantStateKey, st.states)
			}
			if state.Cursor != "cur-1" {
				t.Errorf("cursor = %q, want cur-1", state.Cursor)
			}
			if state.ProviderKey != test.wantProvider {
				t.Errorf("state provider = %q, want %q", state.ProviderKey, test.wantProvider)
			}
		})
	}
}

// TestRunOnce_MailAndTeamsPlansShareOneConnection proves a Microsoft
// connection holding both capabilities syncs under both plans with isolated
// cursor rows.
func TestRunOnce_MailAndTeamsPlansShareOneConnection(t *testing.T) {
	conn := planConnection("conn_ms", "microsoft", "mail.read", "teams.messages.read")
	st := &fakeStore{connections: []store.Connection{conn}}
	graph := &fakeFetcher{results: map[string]FetchResult{
		"": {Messages: []EmailMessage{chatMessage("mail-1")}, NextCursor: "delta-1"},
	}}
	teams := &fakeFetcher{results: map[string]FetchResult{
		"": {Messages: []EmailMessage{chatMessage("chat-1")}, NextCursor: "2026-07-18T10:00:00Z"},
	}}
	ing := &fakeIngestor{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Graph: graph, Teams: teams}

	n, err := w.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if n != 2 {
		t.Fatalf("ingested = %d, want 2 (one per plan)", n)
	}
	if got := st.states["conn_ms"].Cursor; got != "delta-1" {
		t.Errorf("mail cursor = %q, want delta-1", got)
	}
	if got := st.states["conn_ms:teams"].Cursor; got != "2026-07-18T10:00:00Z" {
		t.Errorf("teams cursor = %q, want the teams watermark", got)
	}
	if len(ing.conns) != 2 {
		t.Fatalf("deliveries = %d, want 2", len(ing.conns))
	}
	// Delivery order follows the plan table: mail first, teams second.
	if ing.conns[0].ProviderKey != "microsoft" || ing.conns[1].ProviderKey != "teams" {
		t.Errorf("delivery providers = %q, %q, want microsoft then teams", ing.conns[0].ProviderKey, ing.conns[1].ProviderKey)
	}
}

func TestRunOnce_TeamsKeepsConnectedUsersOutboundMessages(t *testing.T) {
	conn := planConnection("conn_ms", "microsoft", "teams.messages.read")
	conn.UserEmail = "ima@aquatiq.com"
	st := &fakeStore{connections: []store.Connection{conn}}
	teams := &fakeFetcher{results: map[string]FetchResult{
		"": {
			Messages: []EmailMessage{{
				ProviderEventID: "chat-self-1",
				Direction:       "outbound",
				From:            Participant{Name: "Ima", Email: "ima@aquatiq.com"},
				To:              []Participant{{Name: "Robert", Email: "robert@example.com"}},
				BodyText:        "My reply",
			}},
			NextCursor: "2026-07-18T10:00:00Z",
		},
	}}
	ing := &fakeIngestor{}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing, Teams: teams}

	n, err := w.RunOnce(context.Background())
	if err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if n != 1 || len(ing.events) != 1 {
		t.Fatalf("ingested = %d, events = %d, want the outbound Teams message", n, len(ing.events))
	}
	if ing.events[0].Direction != "outbound" {
		t.Fatalf("direction = %q, want outbound", ing.events[0].Direction)
	}
}

// TestRunOnce_ChannelProviderStampedOnDelivery locks the outbound provider
// override: chat plans deliver under their channel provider while email plans
// keep the connection's own provider key.
func TestRunOnce_ChannelProviderStampedOnDelivery(t *testing.T) {
	tests := []struct {
		name         string
		conn         store.Connection
		assign       func(w *Worker, f Fetcher)
		wantProvider string
	}{
		{name: "gmail unchanged", conn: planConnection("c1", "google", "gmail.read"), assign: func(w *Worker, f Fetcher) { w.Gmail = f }, wantProvider: "google"},
		{name: "teams stamped", conn: planConnection("c2", "microsoft", "teams.messages.read"), assign: func(w *Worker, f Fetcher) { w.Teams = f }, wantProvider: "teams"},
		{name: "x stamped", conn: planConnection("c3", "x", "social.inbox.read"), assign: func(w *Worker, f Fetcher) { w.XDM = f }, wantProvider: "x"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			st := &fakeStore{connections: []store.Connection{test.conn}}
			fetcher := &fakeFetcher{results: map[string]FetchResult{
				"": {Messages: []EmailMessage{chatMessage("m1")}, NextCursor: "cur"},
			}}
			ing := &fakeIngestor{}
			w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: ing}
			test.assign(&w, fetcher)

			if _, err := w.RunOnce(context.Background()); err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if len(ing.conns) != 1 || ing.conns[0].ProviderKey != test.wantProvider {
				t.Fatalf("delivered provider = %+v, want %q", ing.conns, test.wantProvider)
			}
		})
	}
}

// TestRunOnce_ConnectionFetcherReceivesConnection proves the optional
// ConnectionFetcher upgrade hands the fetcher the connection it is syncing
// (Discord needs provider_context["guild_id"]).
func TestRunOnce_ConnectionFetcherReceivesConnection(t *testing.T) {
	conn := planConnection("conn_dc", "discord", "messages.read")
	conn.ProviderContext = map[string]string{"guild_id": "g1"}
	st := &fakeStore{connections: []store.Connection{conn}}
	fetcher := &fakeConnFetcher{fakeFetcher: fakeFetcher{results: map[string]FetchResult{
		"": {Messages: []EmailMessage{chatMessage("m1")}, NextCursor: "42"},
	}}}
	w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}, Discord: fetcher}

	if _, err := w.RunOnce(context.Background()); err != nil {
		t.Fatalf("RunOnce: %v", err)
	}
	if len(fetcher.conns) != 1 || fetcher.conns[0].ID != "conn_dc" {
		t.Fatalf("FetchConnection connections = %+v, want conn_dc", fetcher.conns)
	}
	if fetcher.conns[0].ProviderContext["guild_id"] != "g1" {
		t.Fatalf("provider context not threaded through: %+v", fetcher.conns[0].ProviderContext)
	}
}

// TestRunOnce_ChatScopeFallbackAdmitsLegacyConnections mirrors the email
// scope fallback for the chat plans: connections predating the capability
// rows still sync when the raw OAuth scope is present.
func TestRunOnce_ChatScopeFallbackAdmitsLegacyConnections(t *testing.T) {
	tests := []struct {
		name   string
		conn   store.Connection
		scope  string
		assign func(w *Worker, f Fetcher)
	}{
		{name: "teams", conn: planConnection("c1", "microsoft"), scope: "ChannelMessage.Read.All", assign: func(w *Worker, f Fetcher) { w.Teams = f }},
		{name: "slack", conn: planConnection("c2", "slack"), scope: "channels:history", assign: func(w *Worker, f Fetcher) { w.Slack = f }},
		{name: "x", conn: planConnection("c3", "x"), scope: "dm.read", assign: func(w *Worker, f Fetcher) { w.XDM = f }},
		{name: "discord", conn: planConnection("c4", "discord"), scope: "bot", assign: func(w *Worker, f Fetcher) { w.Discord = f }},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			conn := test.conn
			conn.Scopes = []string{test.scope}
			st := &fakeStore{connections: []store.Connection{conn}}
			fetcher := &fakeFetcher{results: map[string]FetchResult{"": {NextCursor: "c"}}}
			w := Worker{Store: st, Tokens: fakeTokens{}, Ingest: &fakeIngestor{}}
			test.assign(&w, fetcher)

			if _, err := w.RunOnce(context.Background()); err != nil {
				t.Fatalf("RunOnce: %v", err)
			}
			if len(fetcher.calls) != 1 {
				t.Fatalf("legacy scope connection should sync, calls=%v", fetcher.calls)
			}
		})
	}
}
