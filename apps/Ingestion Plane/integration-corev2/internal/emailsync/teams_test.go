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
			fmt.Fprint(w, `{"value": [{"id": "team1", "displayName": "Velion"}]}`)
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
	if result.NextCursor != "2026-07-18T11:00:00Z" {
		t.Errorf("cursor = %q, want the max createdDateTime", result.NextCursor)
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
	if result.NextCursor != "2026-07-18T10:00:00Z" {
		t.Errorf("cursor = %q, must not regress on an empty cycle", result.NextCursor)
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
	// Only the emitted message may advance the watermark.
	if result.NextCursor != result.Messages[0].OccurredAt.Format(time.RFC3339Nano) {
		t.Errorf("cursor = %q, want the emitted message's createdDateTime", result.NextCursor)
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
