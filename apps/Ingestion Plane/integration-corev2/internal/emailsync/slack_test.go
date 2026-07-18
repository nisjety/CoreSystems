package emailsync

import (
	"context"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func newSlackServer(t *testing.T) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/users.conversations"):
			if r.URL.Query().Get("types") != "public_channel,private_channel,im,mpim" {
				t.Errorf("conversation types = %s", r.URL.Query().Get("types"))
			}
			fmt.Fprint(w, `{"ok": true, "channels": [
				{"id": "C1", "name": "support"},
				{"id": "D1", "name": "", "is_im": true}
			]}`)
		case strings.HasSuffix(r.URL.Path, "/conversations.history"):
			if r.URL.Query().Get("inclusive") != "false" {
				t.Errorf("history must be exclusive of the watermark: %s", r.URL.RawQuery)
			}
			switch r.URL.Query().Get("channel") {
			case "C1":
				if r.URL.Query().Get("oldest") != "1752000000.000100" {
					t.Errorf("oldest = %s, want the stored cursor", r.URL.Query().Get("oldest"))
				}
				// Newest-first: a fresh message, a join event, and a bot message.
				fmt.Fprint(w, `{"ok": true, "messages": [
					{"type": "message", "user": "U2", "text": "trenger hjelp", "ts": "1752000200.000300", "client_msg_id": "cm-2"},
					{"type": "message", "subtype": "channel_join", "user": "U3", "text": "<@U3> ble med", "ts": "1752000150.000000"},
					{"type": "message", "bot_id": "B1", "text": "bot støy", "ts": "1752000140.000000"}
				]}`)
			case "D1":
				fmt.Fprint(w, `{"ok": true, "messages": [
					{"type": "message", "user": "U2", "text": "hei direkte", "ts": "1752000100.000200", "client_msg_id": "cm-1"}
				]}`)
			default:
				t.Errorf("unexpected history channel %s", r.URL.Query().Get("channel"))
				fmt.Fprint(w, `{"ok": false, "error": "channel_not_found"}`)
			}
		case strings.HasSuffix(r.URL.Path, "/users.info"):
			if r.URL.Query().Get("user") != "U2" {
				t.Errorf("users.info for %s, want U2 only (cache misses)", r.URL.Query().Get("user"))
			}
			fmt.Fprint(w, `{"ok": true, "user": {"name": "kari", "real_name": "Kari Norman", "profile": {"display_name": "Kari"}}}`)
		default:
			t.Errorf("unexpected slack call: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
}

func TestSlack_HistorySkipsAndAdvancesTSWatermark(t *testing.T) {
	server := newSlackServer(t)
	defer server.Close()

	f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "xoxb-tok", "1752000000.000100", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2 (join event and bot skipped): %+v", len(result.Messages), result.Messages)
	}
	if result.NextCursor != "1752000200.000300" {
		t.Errorf("cursor = %q, want the max ts", result.NextCursor)
	}

	channelMsg := result.Messages[0]
	if channelMsg.ProviderEventID != "C1:1752000200.000300" || channelMsg.ProviderMessageID != "cm-2" {
		t.Errorf("ids: %+v", channelMsg)
	}
	if channelMsg.ProviderThreadID != "C1" || channelMsg.Subject != "#support" {
		t.Errorf("thread mapping: %+v", channelMsg)
	}
	if channelMsg.From.Name != "Kari" {
		t.Errorf("from = %q, want resolved display name", channelMsg.From.Name)
	}
	if channelMsg.BodyText != "trenger hjelp" {
		t.Errorf("body = %q", channelMsg.BodyText)
	}
	if !channelMsg.OccurredAt.Equal(time.Unix(1752000200, 300_000).UTC()) {
		t.Errorf("occurredAt = %v", channelMsg.OccurredAt)
	}

	dmMsg := result.Messages[1]
	if dmMsg.Subject != "(dm)" || dmMsg.ProviderThreadID != "D1" {
		t.Errorf("dm mapping: %+v", dmMsg)
	}
}

func TestSlack_ErrorEnvelopeIsError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		fmt.Fprint(w, `{"ok": false, "error": "invalid_auth"}`)
	}))
	defer server.Close()

	f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
	_, err := f.Fetch(context.Background(), "bad-tok", "", 24*time.Hour, 25)
	if err == nil || !strings.Contains(err.Error(), "invalid_auth") {
		t.Fatalf("err = %v, want the slack error envelope surfaced", err)
	}
}

func TestSlack_RateLimitStopsCycleGracefully(t *testing.T) {
	tests := []struct {
		name    string
		handler http.HandlerFunc
	}{
		{
			name: "envelope ratelimited",
			handler: func(w http.ResponseWriter, r *http.Request) {
				if strings.HasSuffix(r.URL.Path, "/users.conversations") {
					fmt.Fprint(w, `{"ok": true, "channels": [{"id": "C1", "name": "support"}]}`)
					return
				}
				fmt.Fprint(w, `{"ok": false, "error": "ratelimited"}`)
			},
		},
		{
			name: "http 429",
			handler: func(w http.ResponseWriter, r *http.Request) {
				if strings.HasSuffix(r.URL.Path, "/users.conversations") {
					fmt.Fprint(w, `{"ok": true, "channels": [{"id": "C1", "name": "support"}]}`)
					return
				}
				http.Error(w, `{"ok": false}`, http.StatusTooManyRequests)
			},
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(test.handler)
			defer server.Close()

			f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
			result, err := f.Fetch(context.Background(), "tok", "1752000000.000100", 24*time.Hour, 25)
			if err != nil {
				t.Fatalf("rate limiting must end the cycle gracefully, got %v", err)
			}
			if len(result.Messages) != 0 {
				t.Fatalf("messages = %d, want 0", len(result.Messages))
			}
			if result.NextCursor != "1752000000.000100" {
				t.Errorf("cursor = %q, must not move past unfetched messages", result.NextCursor)
			}
		})
	}
}

func TestSlack_BootstrapUsesBackfillWindowAsOldest(t *testing.T) {
	var gotOldest string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, "/users.conversations") {
			fmt.Fprint(w, `{"ok": true, "channels": [{"id": "C1", "name": "support"}]}`)
			return
		}
		gotOldest = r.URL.Query().Get("oldest")
		fmt.Fprint(w, `{"ok": true, "messages": []}`)
	}))
	defer server.Close()

	f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
	before := time.Now().UTC().Add(-24 * time.Hour)
	if _, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25); err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if !strings.HasSuffix(gotOldest, ".000000") {
		t.Fatalf("bootstrap oldest = %q, want a slack ts string", gotOldest)
	}
	if slackTSLess(gotOldest, fmt.Sprintf("%d.000000", before.Add(-time.Minute).Unix())) {
		t.Errorf("bootstrap oldest = %q, want ~24h ago", gotOldest)
	}
}
