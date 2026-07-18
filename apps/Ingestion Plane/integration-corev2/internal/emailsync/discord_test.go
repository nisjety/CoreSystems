package emailsync

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

func discordConnection(guildID string) store.Connection {
	conn := store.Connection{
		ID: "conn_dc", ProviderKey: "discord", OrganizationID: "org_1",
		Status: "active", Capabilities: []string{"messages.read"},
	}
	if guildID != "" {
		conn.ProviderContext = map[string]string{"guild_id": guildID}
	}
	return conn
}

func newDiscordServer(t *testing.T, wantAfter string) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("Authorization"); got != "Bot app-bot-token" {
			t.Errorf("Authorization = %q, want the app-level bot token", got)
		}
		switch {
		case strings.HasSuffix(r.URL.Path, "/users/@me/guilds"):
			fmt.Fprint(w, `[{"id": "g1", "name": "Velion"}]`)
		case strings.HasSuffix(r.URL.Path, "/guilds/g1/channels"):
			fmt.Fprint(w, `[
				{"id": "ch1", "name": "general", "type": 0},
				{"id": "v1", "name": "stemme", "type": 2}
			]`)
		case strings.HasSuffix(r.URL.Path, "/channels/ch1/messages"):
			if wantAfter != "" && r.URL.Query().Get("after") != wantAfter {
				t.Errorf("after = %s, want %s", r.URL.Query().Get("after"), wantAfter)
			}
			fmt.Fprint(w, `[
				{"id": "1100", "content": "andre", "timestamp": "2026-07-18T10:05:00.000000+00:00", "author": {"id": "u1", "username": "kari", "global_name": "Kari"}},
				{"id": "999", "content": "første", "timestamp": "2026-07-18T10:00:00.000000+00:00", "author": {"id": "u1", "username": "kari", "global_name": "Kari"}},
				{"id": "1050", "content": "bot svar", "timestamp": "2026-07-18T10:02:00.000000+00:00", "author": {"id": "b1", "username": "velion-bot", "bot": true}},
				{"id": "1060", "content": "", "timestamp": "2026-07-18T10:03:00.000000+00:00", "author": {"id": "u2", "username": "ola"}}
			]`)
		default:
			t.Errorf("unexpected discord call: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
}

func TestDiscord_GuildChannelsAfterCursor(t *testing.T) {
	server := newDiscordServer(t, "900")
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "app-bot-token", HTTP: server.Client()}
	result, err := f.FetchConnection(context.Background(), discordConnection("g1"), "ignored-oauth-token", "900", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2 (bot author and empty content skipped): %+v", len(result.Messages), result.Messages)
	}
	// Snowflake ordering: oldest-first despite "999" > "1100" lexicographically.
	if result.Messages[0].ProviderEventID != "999" || result.Messages[1].ProviderEventID != "1100" {
		t.Errorf("order: %s, %s", result.Messages[0].ProviderEventID, result.Messages[1].ProviderEventID)
	}
	if result.NextCursor != "1100" {
		t.Errorf("cursor = %q, want max snowflake", result.NextCursor)
	}
	msg := result.Messages[0]
	if msg.ProviderThreadID != "ch1" || msg.Subject != "#general" {
		t.Errorf("thread mapping: %+v", msg)
	}
	if msg.From.Name != "Kari" {
		t.Errorf("from = %q, want global_name over username", msg.From.Name)
	}
	if msg.BodyText != "første" {
		t.Errorf("body = %q", msg.BodyText)
	}
	if !msg.OccurredAt.Equal(time.Date(2026, 7, 18, 10, 0, 0, 0, time.UTC)) {
		t.Errorf("occurredAt = %v", msg.OccurredAt)
	}
}

func TestDiscord_FallsBackToBotGuildsWithoutGuildContext(t *testing.T) {
	server := newDiscordServer(t, "")
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "app-bot-token", HTTP: server.Client()}
	result, err := f.FetchConnection(context.Background(), discordConnection(""), "ignored", "900", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2 via /users/@me/guilds fallback", len(result.Messages))
	}
}

func TestDiscord_BootstrapAfterIsBackfillSnowflake(t *testing.T) {
	var gotAfter string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/guilds/g1/channels"):
			fmt.Fprint(w, `[{"id": "ch1", "name": "general", "type": 0}]`)
		case strings.HasSuffix(r.URL.Path, "/channels/ch1/messages"):
			gotAfter = r.URL.Query().Get("after")
			fmt.Fprint(w, `[]`)
		default:
			t.Errorf("unexpected discord call: %s", r.URL.Path)
		}
	}))
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "app-bot-token", HTTP: server.Client()}
	result, err := f.FetchConnection(context.Background(), discordConnection("g1"), "ignored", "", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	want := discordSnowflakeForTime(time.Now().UTC().Add(-24 * time.Hour))
	if gotAfter == "" || len(gotAfter) != len(want) {
		t.Fatalf("bootstrap after = %q, want a snowflake near %q", gotAfter, want)
	}
	if result.NextCursor != "" {
		t.Errorf("cursor = %q, empty cycle must not fabricate a cursor", result.NextCursor)
	}
}

func TestDiscord_MissingBotTokenIsTypedError(t *testing.T) {
	f := &DiscordFetcher{BaseURL: "http://unused", BotToken: "  "}
	_, err := f.FetchConnection(context.Background(), discordConnection("g1"), "ignored", "", 24*time.Hour, 25)
	if !errors.Is(err, errDiscordBotNotConfigured) {
		t.Fatalf("err = %v, want errDiscordBotNotConfigured", err)
	}
}
