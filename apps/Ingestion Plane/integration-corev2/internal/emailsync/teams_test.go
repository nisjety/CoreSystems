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

func teamsMessageJSON(id, created, contentType, content, displayName string) string {
	from := fmt.Sprintf(`{"user": {"id": "u1", "displayName": %q}}`, displayName)
	if displayName == "" {
		from = `{"user": null}`
	}
	return fmt.Sprintf(`{
		"id": %q, "messageType": "message", "createdDateTime": %q,
		"from": %s,
		"body": {"contentType": %q, "content": %q}
	}`, id, created, from, contentType, content)
}

func TestTeams_OneOnOneResolvesCounterpartAndSelfDirection(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[{"id":"chat1","topic":"","chatType":"oneOnOne","members":[{"userId":"self-user","displayName":"Ima Fernandes Da Costa","email":"ima@coresystem.com"},{"userId":"other-user","displayName":"Robert Røsten","email":"robert@example.com"}]}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m2","messageType":"message","createdDateTime":"2026-07-18T10:20:00Z","from":{"user":{"id":"self-user","displayName":"Ima Fernandes Da Costa"}},"body":{"contentType":"text","content":"My reply"}},{"id":"m1","messageType":"message","createdDateTime":"2026-07-18T10:10:00Z","from":{"user":{"id":"other-user","displayName":"Robert Røsten"}},"body":{"contentType":"text","content":"Hello"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := fetcher.FetchConnection(context.Background(), store.Connection{
		ProviderAccountID: "self-user",
		DisplayName:       "Ima Fernandes Da Costa",
		UserEmail:         "ima@coresystem.com",
	}, "token", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2", len(result.Messages))
	}
	inbound, outbound := result.Messages[0], result.Messages[1]
	if inbound.Subject != "Robert Røsten" || inbound.Direction != "inbound" || inbound.From.Email != "robert@example.com" {
		t.Fatalf("inbound identity = %+v", inbound)
	}
	if outbound.Subject != "Robert Røsten" || outbound.Direction != "outbound" || outbound.From.Email != "ima@coresystem.com" {
		t.Fatalf("outbound identity = %+v", outbound)
	}
	if len(outbound.To) != 1 || outbound.To[0].Name != "Robert Røsten" || outbound.To[0].Email != "robert@example.com" {
		t.Fatalf("outbound counterpart = %+v", outbound.To)
	}
}

func TestTeams_OneOnOneUsesEmailFallbackToResolveSelfDirection(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[{"id":"chat1","topic":"","chatType":"oneOnOne","members":[{"userId":"self-user","displayName":"Ima","email":"ima@coresystem.com"},{"userId":"other-user","displayName":"Robert","email":"robert@example.com"}]}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m1","messageType":"message","createdDateTime":"2026-07-18T10:20:00Z","from":{"user":{"id":"self-user","displayName":"Ima"}},"body":{"contentType":"text","content":"My reply"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := fetcher.FetchConnection(context.Background(), store.Connection{
		DisplayName: "Ima",
		UserEmail:   "ima@coresystem.com",
	}, "token", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	if len(result.Messages) != 1 || result.Messages[0].Direction != "outbound" {
		t.Fatalf("legacy identity direction = %+v, want outbound", result.Messages)
	}
}

func TestTeams_PerThreadCursorDoesNotSkipLaterChatsAtBatchCap(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[{"id":"chat-newer","topic":"Newer"},{"id":"chat-older","topic":"Older"}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat-newer/messages"):
			fmt.Fprint(w, `{"value":[{"id":"newer","messageType":"message","createdDateTime":"2026-07-18T10:30:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"newer"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat-older/messages"):
			fmt.Fprint(w, `{"value":[{"id":"older","messageType":"message","createdDateTime":"2026-07-18T10:15:00Z","from":{"user":{"id":"u2","displayName":"Two"}},"body":{"contentType":"text","content":"older"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	first, err := fetcher.Fetch(context.Background(), "token", "2026-07-18T10:00:00Z", 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("first Fetch: %v", err)
	}
	if len(first.Messages) != 1 || first.Messages[0].ProviderEventID != "newer" {
		t.Fatalf("first messages = %+v", first.Messages)
	}
	second, err := fetcher.Fetch(context.Background(), "token", first.NextCursor, 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("second Fetch: %v", err)
	}
	if len(second.Messages) != 1 || second.Messages[0].ProviderEventID != "older" {
		t.Fatalf("second messages = %+v, want later chat's older message", second.Messages)
	}
}

func TestTeams_PaginatesChatCollection(t *testing.T) {
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprintf(w, `{"value":[{"id":"chat1","topic":"One"}],"@odata.nextLink":%q}`, server.URL+"/chat-page-2")
		case strings.HasSuffix(r.URL.Path, "/chat-page-2"):
			fmt.Fprint(w, `{"value":[{"id":"chat2","topic":"Two"}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m1","messageType":"message","createdDateTime":"2026-07-18T10:10:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"one"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat2/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m2","messageType":"message","createdDateTime":"2026-07-18T10:20:00Z","from":{"user":{"id":"u2","displayName":"Two"}},"body":{"contentType":"text","content":"two"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := fetcher.Fetch(context.Background(), "token", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 2 || result.Messages[0].ProviderEventID != "m1" || result.Messages[1].ProviderEventID != "m2" {
		t.Fatalf("paginated chat messages = %+v", result.Messages)
	}
}

func TestTeams_PaginatesMessageHistoryOldestFirstAcrossCycles(t *testing.T) {
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[{"id":"chat1","topic":"History"}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			fmt.Fprintf(w, `{"value":[{"id":"newer","messageType":"message","createdDateTime":"2026-07-18T10:20:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"newer"}}],"@odata.nextLink":%q}`, server.URL+"/message-page-2")
		case strings.HasSuffix(r.URL.Path, "/message-page-2"):
			fmt.Fprint(w, `{"value":[{"id":"older","messageType":"message","createdDateTime":"2026-07-18T10:10:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"older"}},{"id":"stale","messageType":"message","createdDateTime":"2026-07-18T09:50:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"stale"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	first, err := fetcher.Fetch(context.Background(), "token", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("first Fetch: %v", err)
	}
	if len(first.Messages) != 0 {
		t.Fatalf("first cycle = %+v, want scan progress before emitting newer history", first.Messages)
	}
	second, err := fetcher.Fetch(context.Background(), "token", first.NextCursor, 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("second Fetch: %v", err)
	}
	if len(second.Messages) != 1 || second.Messages[0].ProviderEventID != "older" {
		t.Fatalf("second cycle = %+v, want oldest unseen message", second.Messages)
	}
	third, err := fetcher.Fetch(context.Background(), "token", second.NextCursor, 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("third Fetch: %v", err)
	}
	if len(third.Messages) != 1 || third.Messages[0].ProviderEventID != "newer" {
		t.Fatalf("third cycle = %+v, want remaining newer message", third.Messages)
	}
}

func TestTeams_PaginatesJoinedTeamsAndChannels(t *testing.T) {
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprintf(w, `{"value":[{"id":"team1"}],"@odata.nextLink":%q}`, server.URL+"/teams-page-2")
		case strings.HasSuffix(r.URL.Path, "/teams-page-2"):
			fmt.Fprint(w, `{"value":[{"id":"team2"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels"):
			fmt.Fprintf(w, `{"value":[{"id":"ch1","displayName":"one"}],"@odata.nextLink":%q}`, server.URL+"/channels-page-2")
		case strings.HasSuffix(r.URL.Path, "/channels-page-2"):
			fmt.Fprint(w, `{"value":[{"id":"ch2","displayName":"two"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team2/channels"):
			fmt.Fprint(w, `{"value":[{"id":"ch3","displayName":"three"}]}`)
		case strings.Contains(r.URL.Path, "/messages"):
			channelID := strings.Split(r.URL.Path, "/")[4]
			fmt.Fprintf(w, `{"value":[{"id":%q,"messageType":"message","createdDateTime":"2026-07-18T10:10:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"message"}}]}`, "m-"+channelID)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := fetcher.Fetch(context.Background(), "token", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 3 {
		t.Fatalf("messages = %+v, want channels from every teams/channels page", result.Messages)
	}
}

func TestTeams_ChannelContinuationStaysBoundToTeamAfterListReorder(t *testing.T) {
	joinedCalls := 0
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			joinedCalls++
			if joinedCalls == 1 {
				fmt.Fprint(w, `{"value":[{"id":"team1"},{"id":"team2"}]}`)
				return
			}
			fmt.Fprint(w, `{"value":[{"id":"team2"},{"id":"team1"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels"):
			fmt.Fprintf(w, `{"value":[{"id":"ch1","displayName":"one"}],"@odata.nextLink":%q}`, server.URL+"/team1-channels-page-2")
		case strings.HasSuffix(r.URL.Path, "/team1-channels-page-2"):
			fmt.Fprint(w, `{"value":[{"id":"ch2","displayName":"two"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team2/channels"):
			fmt.Fprint(w, `{"value":[]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels/ch1/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m1","messageType":"message","createdDateTime":"2026-07-18T10:10:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"one"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels/ch2/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m2","messageType":"message","createdDateTime":"2026-07-18T10:20:00Z","from":{"user":{"id":"u1","displayName":"One"}},"body":{"contentType":"text","content":"two"}}]}`)
		default:
			t.Fatalf("unexpected Graph request after team reorder: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	first, err := fetcher.Fetch(context.Background(), "token", "2026-07-18T10:00:00Z", 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("first Fetch: %v", err)
	}
	if len(first.Messages) != 1 || first.Messages[0].ProviderEventID != "m1" {
		t.Fatalf("first messages = %+v", first.Messages)
	}
	second, err := fetcher.Fetch(context.Background(), "token", first.NextCursor, 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("second Fetch: %v", err)
	}
	if len(second.Messages) != 1 || second.Messages[0].ProviderEventID != "m2" {
		t.Fatalf("second messages = %+v, want continuation under stable team1", second.Messages)
	}
}

func TestTeams_ConnectionBootstrapIncludesExistingChatHistory(t *testing.T) {
	messageTime := time.Now().UTC().Add(-7 * 24 * time.Hour).Format(time.RFC3339)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[{"id":"chat1","topic":"","chatType":"oneOnOne","members":[{"userId":"self-user","displayName":"Ima","email":"ima@coresystem.com"},{"userId":"other-user","displayName":"Robert","email":"robert@example.com"}]}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			fmt.Fprintf(w, `{"value":[{"id":"m1","messageType":"message","createdDateTime":%q,"from":{"user":{"id":"other-user","displayName":"Robert"}},"body":{"contentType":"text","content":"Existing conversation"}}]}`, messageTime)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[]}`)
		default:
			t.Fatalf("unexpected Graph request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := fetcher.FetchConnection(context.Background(), store.Connection{
		ProviderAccountID: "self-user",
		DisplayName:       "Ima",
		UserEmail:         "ima@coresystem.com",
	}, "token", "", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("FetchConnection: %v", err)
	}
	if len(result.Messages) != 1 {
		t.Fatalf("messages = %d, want existing Teams history during bootstrap", len(result.Messages))
	}
}

func newTeamsServer(t *testing.T) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value": [{"id": "chat1", "topic": "Prosjekt Alfa", "chatType": "group"}, {"id": "chat2", "topic": "", "chatType": "oneOnOne"}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			// Newest-first, like Graph: fresh html message, a system event
			// (from.user null), a stale message behind the watermark, and an
			// empty body.
			fmt.Fprintf(w, `{"value": [%s, %s, %s, %s]}`,
				teamsMessageJSON("t-msg-2", "2026-07-18T10:30:00.5Z", "html", "<p>Hei &amp; hallo</p>", "Kari Norman"),
				`{"id": "t-sys", "messageType": "message", "createdDateTime": "2026-07-18T10:20:00Z", "from": {"user": null}, "body": {"contentType": "text", "content": "system"}}`,
				teamsMessageJSON("t-old", "2026-07-18T09:00:00Z", "text", "for gammel", "Kari Norman"),
				teamsMessageJSON("t-empty", "2026-07-18T10:25:00Z", "text", "", "Kari Norman"))
		case strings.HasSuffix(r.URL.Path, "/chats/chat2/messages"):
			fmt.Fprintf(w, `{"value": [%s, %s]}`,
				teamsMessageJSON("t-msg-1", "2026-07-18T10:15:00Z", "text", "ren tekst", "Ola Hansen"),
				`{"id": "t-event", "messageType": "systemEventMessage", "createdDateTime": "2026-07-18T10:16:00Z", "from": {"user": {"id": "u2", "displayName": "Ola"}}, "body": {"contentType": "text", "content": "ble med"}}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value": [{"id": "team1", "displayName": "Verevon"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels"):
			fmt.Fprint(w, `{"value": [{"id": "ch1", "displayName": "general"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels/ch1/messages"):
			fmt.Fprintf(w, `{"value": [%s]}`,
				teamsMessageJSON("t-msg-3", "2026-07-18T11:00:00Z", "text", "kanalmelding", "Per Olsen"))
		default:
			t.Errorf("unexpected graph call: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
}

func TestTeams_ChatsAndChannelsAdvanceWatermark(t *testing.T) {
	server := newTeamsServer(t)
	defer server.Close()

	f := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 3 {
		t.Fatalf("messages = %d, want 3 (system events, stale, and empty skipped): %+v", len(result.Messages), result.Messages)
	}
	// Sorted oldest-first across chats and channels.
	for i, want := range []string{"t-msg-1", "t-msg-2", "t-msg-3"} {
		if result.Messages[i].ProviderEventID != want {
			t.Errorf("messages[%d] = %s, want %s", i, result.Messages[i].ProviderEventID, want)
		}
	}
	cursor := decodeTeamsCursor(result.NextCursor, 24*time.Hour)
	if got := cursor.Threads["chat:chat1"]; got != "2026-07-18T10:30:00.5Z" {
		t.Errorf("chat1 cursor = %q", got)
	}
	if got := cursor.Threads["chat:chat2"]; got != "2026-07-18T10:15:00Z" {
		t.Errorf("chat2 cursor = %q", got)
	}
	if got := cursor.Threads["channel:team1:ch1"]; got != "2026-07-18T11:00:00Z" {
		t.Errorf("channel cursor = %q", got)
	}

	htmlMsg := result.Messages[1]
	if htmlMsg.BodyHTML != "<p>Hei &amp; hallo</p>" {
		t.Errorf("html body = %q", htmlMsg.BodyHTML)
	}
	if htmlMsg.BodyText != "Hei & hallo" {
		t.Errorf("stripped body text = %q, want entities unescaped and tags dropped", htmlMsg.BodyText)
	}
	if htmlMsg.Subject != "Prosjekt Alfa" || htmlMsg.From.Name != "Kari Norman" || htmlMsg.ProviderThreadID != "chat1" {
		t.Errorf("chat mapping: %+v", htmlMsg)
	}
	if !htmlMsg.OccurredAt.Equal(time.Date(2026, 7, 18, 10, 30, 0, 500_000_000, time.UTC)) {
		t.Errorf("occurredAt = %v", htmlMsg.OccurredAt)
	}

	dmMsg := result.Messages[0]
	if dmMsg.Subject != "(chat)" || dmMsg.ProviderThreadID != "chat2" {
		t.Errorf("topicless chat mapping: %+v", dmMsg)
	}
	channelMsg := result.Messages[2]
	if channelMsg.Subject != "#general" || channelMsg.ProviderThreadID != "ch1" {
		t.Errorf("channel mapping: %+v", channelMsg)
	}
}

func TestTeams_EmptyCycleKeepsWatermark(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		fmt.Fprint(w, `{"value": []}`)
	}))
	defer server.Close()

	f := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 0 {
		t.Fatalf("messages = %d, want 0", len(result.Messages))
	}
	cursor := decodeTeamsCursor(result.NextCursor, 24*time.Hour)
	if cursor.Bootstrap != "2026-07-18T10:00:00Z" || len(cursor.Threads) != 0 {
		t.Errorf("cursor = %+v, must preserve the incoming baseline on an empty cycle", cursor)
	}
}

func TestTeams_MaxMessagesRespected(t *testing.T) {
	server := newTeamsServer(t)
	defer server.Close()

	f := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "2026-07-18T10:00:00Z", 24*time.Hour, 1)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 1 {
		t.Fatalf("messages = %d, want capped 1", len(result.Messages))
	}
	// Only the emitted message's thread may advance its watermark.
	cursor := decodeTeamsCursor(result.NextCursor, 24*time.Hour)
	if got := cursor.Threads["chat:chat1"]; got != result.Messages[0].OccurredAt.Format(time.RFC3339Nano) {
		t.Errorf("chat1 cursor = %q, want the emitted message's createdDateTime", got)
	}
	if len(cursor.Threads) != 1 {
		t.Errorf("thread cursors = %+v, want only the emitted thread", cursor.Threads)
	}
}

func TestTeams_AuthFailureIsTypedError(t *testing.T) {
	tests := []struct {
		name   string
		status int
	}{
		{name: "401 unauthorized", status: http.StatusUnauthorized},
		{name: "403 forbidden", status: http.StatusForbidden},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				http.Error(w, `{"error": {"code": "InvalidAuthenticationToken"}}`, test.status)
			}))
			defer server.Close()

			f := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
			_, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25)
			var httpErr *providerHTTPError
			if !asProviderHTTPError(err, &httpErr) || httpErr.Status != test.status {
				t.Fatalf("err = %v, want typed provider error carrying %d", err, test.status)
			}
			if errors.Is(err, ErrCursorExpired) {
				t.Fatal("auth failures must not masquerade as cursor expiry")
			}
		})
	}
}

func TestTeams_ForbiddenChannelDoesNotDiscardPrivateChats(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/me/chats"):
			fmt.Fprint(w, `{"value":[{"id":"chat1","topic":"Robert"}]}`)
		case strings.HasSuffix(r.URL.Path, "/chats/chat1/messages"):
			fmt.Fprint(w, `{"value":[{"id":"m1","messageType":"message","createdDateTime":"2026-07-18T10:10:00Z","from":{"user":{"id":"u1","displayName":"Robert"}},"body":{"contentType":"text","content":"Hello"}}]}`)
		case strings.HasSuffix(r.URL.Path, "/me/joinedTeams"):
			fmt.Fprint(w, `{"value":[{"id":"team1"}]}`)
		case strings.HasSuffix(r.URL.Path, "/teams/team1/channels"):
			fmt.Fprint(w, `{"value":[{"id":"private-channel","displayName":"Private"}]}`)
		case strings.Contains(r.URL.Path, "/channels/private-channel/messages"):
			http.Error(w, `{"error":{"code":"Forbidden"}}`, http.StatusForbidden)
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()

	fetcher := &TeamsFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := fetcher.Fetch(t.Context(), "token", "2026-07-18T10:00:00Z", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 1 || result.Messages[0].ProviderEventID != "m1" {
		t.Fatalf("messages = %#v, want private chat m1", result.Messages)
	}
}

func TestDecodeTeamsCursor_ExtendsHistoryInThirtyDayStepsOnce(t *testing.T) {
	now := time.Date(2026, 7, 19, 12, 0, 0, 0, time.UTC)
	existing := teamsCursorState{
		Bootstrap:   now.Add(-30 * 24 * time.Hour).Format(time.RFC3339Nano),
		HistoryDays: 30,
		Threads:     map[string]string{"chat:chat1": now.Add(-time.Hour).Format(time.RFC3339Nano)},
		Phase:       "channels",
		ChatIndex:   4,
	}

	extended := decodeTeamsCursorAt(encodeTeamsCursor(existing), 60*24*time.Hour, now)
	if extended.HistoryDays != 60 {
		t.Fatalf("history days = %d, want 60", extended.HistoryDays)
	}
	if got := extended.Bootstrap; got != now.Add(-60*24*time.Hour).Format(time.RFC3339Nano) {
		t.Fatalf("bootstrap = %q, want 60-day floor", got)
	}
	if len(extended.Threads) != 0 || extended.Phase != "chats" || extended.ChatIndex != 0 {
		t.Fatalf("extended cursor retained completed scan state: %#v", extended)
	}

	encoded := encodeTeamsCursor(extended)
	resumed := decodeTeamsCursorAt(encoded, 60*24*time.Hour, now.Add(time.Hour))
	if resumed.Bootstrap != extended.Bootstrap {
		t.Fatalf("same history request reset twice: bootstrap %q -> %q", extended.Bootstrap, resumed.Bootstrap)
	}
}

func TestTrustedTeamsEndpointRejectsOffOriginAndSchemeDowngrade(t *testing.T) {
	base := "https://graph.microsoft.com/v1.0"
	for _, candidate := range []string{
		"https://attacker.example/messages?page=2",
		"http://graph.microsoft.com/v1.0/messages?page=2",
		"https://token@example.com/messages",
	} {
		if _, err := trustedTeamsEndpoint(base, candidate); err == nil {
			t.Fatalf("trustedTeamsEndpoint(%q) succeeded, want rejection", candidate)
		}
	}
	want := "https://graph.microsoft.com/v1.0/messages?page=2"
	if got, err := trustedTeamsEndpoint(base, want); err != nil || got != want {
		t.Fatalf("same-origin endpoint = (%q, %v), want %q", got, err, want)
	}
}
