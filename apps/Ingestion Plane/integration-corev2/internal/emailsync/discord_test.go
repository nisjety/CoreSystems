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
		switch {
		case strings.HasSuffix(r.URL.Path, "/users/@me/guilds"):
			switch {
			case strings.HasPrefix(r.Header.Get("Authorization"), "Bearer "):
				fmt.Fprint(w, `[{"id": "g1", "name": "Verevon", "permissions": "32"}]`)
			case r.Header.Get("Authorization") == "Bot app-bot-token":
				fmt.Fprint(w, `[{"id": "g1", "name": "Verevon"}]`)
			default:
				t.Errorf("Authorization = %q, want OAuth user or app bot token", r.Header.Get("Authorization"))
				w.WriteHeader(http.StatusUnauthorized)
			}
		case strings.HasSuffix(r.URL.Path, "/guilds/g1/channels"):
			if got := r.Header.Get("Authorization"); got != "Bot app-bot-token" {
				t.Errorf("Authorization = %q, want the app-level bot token", got)
			}
			fmt.Fprint(w, `[
				{"id": "ch1", "name": "general", "type": 0},
				{"id": "v1", "name": "stemme", "type": 2}
			]`)
		case strings.HasSuffix(r.URL.Path, "/channels/ch1/messages"):
			if got := r.Header.Get("Authorization"); got != "Bot app-bot-token" {
				t.Errorf("Authorization = %q, want the app-level bot token", got)
			}
			if wantAfter != "" && r.URL.Query().Get("after") != wantAfter {
				t.Errorf("after = %s, want %s", r.URL.Query().Get("after"), wantAfter)
			}
			fmt.Fprint(w, `[
				{"id": "1100", "content": "andre", "timestamp": "2026-07-18T10:05:00.000000+00:00", "author": {"id": "u1", "username": "kari", "global_name": "Kari"}},
				{"id": "999", "content": "første", "timestamp": "2026-07-18T10:00:00.000000+00:00", "author": {"id": "u1", "username": "kari", "global_name": "Kari"}},
				{"id": "1050", "content": "bot svar", "timestamp": "2026-07-18T10:02:00.000000+00:00", "author": {"id": "b1", "username": "verevon-bot", "bot": true}},
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
	cursorState, err := decodeDiscordCursor(result.NextCursor, 24*time.Hour)
	if err != nil {
		t.Fatalf("decode cursor: %v", err)
	}
	if cursorState.Channels["ch1"] != "1100" {
		t.Errorf("cursor = %+v, want per-channel max snowflake", cursorState.Channels)
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

func TestDiscord_DerivesSingleManagedBotGuildWithoutContext(t *testing.T) {
	server := newDiscordServer(t, "900")
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "app-bot-token", HTTP: server.Client()}
	result, err := f.FetchConnection(context.Background(), discordConnection(""), "user-oauth-token", "900", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want the uniquely authorized guild", len(result.Messages))
	}
	if got := result.ProviderContextPatch["guild_id"]; got != "g1" {
		t.Fatalf("provider context guild_id = %q, want the derived guild to be persisted", got)
	}
}

func TestDiscord_RejectsGuildNotManagedByOAuthUser(t *testing.T) {
	botRead := false
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/users/@me/guilds"):
			fmt.Fprint(w, `[{"id": "g1", "name": "Other server", "permissions": "0"}]`)
		default:
			botRead = true
			w.WriteHeader(http.StatusForbidden)
		}
	}))
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "app-bot-token", HTTP: server.Client()}
	_, err := f.FetchConnection(context.Background(), discordConnection("g1"), "user-oauth-token", "900", 24*time.Hour, 25)
	if !errors.Is(err, errDiscordGuildNotAuthorized) {
		t.Fatalf("err = %v, want errDiscordGuildNotAuthorized", err)
	}
	if botRead {
		t.Fatal("bot-scoped guild reads must not run before OAuth-user authorization")
	}
}

func TestDiscord_AmbiguousManagedBotGuildsRequireSelection(t *testing.T) {
	botRead := false
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasSuffix(r.URL.Path, "/users/@me/guilds") {
			botRead = true
			w.WriteHeader(http.StatusForbidden)
			return
		}
		if strings.HasPrefix(r.Header.Get("Authorization"), "Bearer ") {
			fmt.Fprint(w, `[
				{"id": "g1", "permissions": "32"},
				{"id": "g2", "permissions": "32"}
			]`)
			return
		}
		fmt.Fprint(w, `[{"id": "g1"}, {"id": "g2"}]`)
	}))
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "app-bot-token", HTTP: server.Client()}
	_, err := f.FetchConnection(context.Background(), discordConnection(""), "user-oauth-token", "900", 24*time.Hour, 25)
	if !errors.Is(err, errDiscordGuildNotConfigured) {
		t.Fatalf("err = %v, want explicit guild selection for an ambiguous install", err)
	}
	if botRead {
		t.Fatal("channel reads must not run until an ambiguous guild is selected")
	}
}

func TestDiscord_BootstrapAfterIsBackfillSnowflake(t *testing.T) {
	var gotAfter string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/users/@me/guilds"):
			fmt.Fprint(w, `[{"id": "g1", "permissions": "32"}]`)
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
	cursorState, decodeErr := decodeDiscordCursor(result.NextCursor, 24*time.Hour)
	if decodeErr != nil {
		t.Fatalf("decode cursor: %v", decodeErr)
	}
	if cursorState.Channels["ch1"] != gotAfter {
		t.Errorf("cursor = %+v, empty cycle must retain the stable backfill watermark", cursorState.Channels)
	}
}

func TestDiscord_PerChannelCursorDoesNotSkipLaterChannelAtCycleCap(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/users/@me/guilds"):
			fmt.Fprint(w, `[{"id":"g1","permissions":"32"}]`)
		case strings.HasSuffix(r.URL.Path, "/guilds/g1/channels"):
			fmt.Fprint(w, `[{"id":"ch1","name":"one","type":0},{"id":"ch2","name":"two","type":0}]`)
		case strings.HasSuffix(r.URL.Path, "/channels/ch1/messages"):
			if r.URL.Query().Get("after") == "1100" {
				fmt.Fprint(w, `[]`)
				return
			}
			fmt.Fprint(w, `[{"id":"1100","content":"first","author":{"id":"u1","username":"one"}}]`)
		case strings.HasSuffix(r.URL.Path, "/channels/ch2/messages"):
			fmt.Fprint(w, `[{"id":"1050","content":"second","author":{"id":"u2","username":"two"}}]`)
		default:
			t.Fatalf("unexpected request: %s", r.URL.Path)
		}
	}))
	defer server.Close()

	f := &DiscordFetcher{BaseURL: server.URL, BotToken: "bot", HTTP: server.Client()}
	first, err := f.FetchConnection(context.Background(), discordConnection("g1"), "oauth", "900", 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("first FetchConnection: %v", err)
	}
	if len(first.Messages) != 1 || first.Messages[0].ProviderThreadID != "ch1" {
		t.Fatalf("first messages = %+v", first.Messages)
	}
	firstState, err := decodeDiscordCursor(first.NextCursor, 24*time.Hour)
	if err != nil || firstState.Channels["ch1"] != "1100" || firstState.Channels["ch2"] != "" {
		t.Fatalf("first cursor = %+v, err=%v", firstState, err)
	}

	second, err := f.FetchConnection(context.Background(), discordConnection("g1"), "oauth", first.NextCursor, 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("second FetchConnection: %v", err)
	}
	if len(second.Messages) != 1 || second.Messages[0].ProviderThreadID != "ch2" {
		t.Fatalf("second messages = %+v", second.Messages)
	}
}

func TestDiscord_MissingBotTokenIsTypedError(t *testing.T) {
	f := &DiscordFetcher{BaseURL: "http://unused", BotToken: "  "}
	_, err := f.FetchConnection(context.Background(), discordConnection("g1"), "ignored", "", 24*time.Hour, 25)
	if !errors.Is(err, errDiscordBotNotConfigured) {
		t.Fatalf("err = %v, want errDiscordBotNotConfigured", err)
	}
}
