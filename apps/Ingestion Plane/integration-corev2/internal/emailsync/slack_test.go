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
	cursorState, err := decodeSlackCursor(result.NextCursor, 24*time.Hour)
	if err != nil {
		t.Fatalf("decode cursor: %v", err)
	}
	if cursorState.Channels["C1"] != "1752000200.000300" || cursorState.Channels["D1"] != "1752000100.000200" {
		t.Errorf("cursor = %+v, want independent per-channel watermarks", cursorState.Channels)
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

func TestSlack_StaleChannelDoesNotAbortHealthyChannels(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/users.conversations"):
			fmt.Fprint(w, `{"ok": true, "channels": [
				{"id": "C_STALE", "name": "archived-support"},
				{"id": "C_LIVE", "name": "support"}
			]}`)
		case strings.HasSuffix(r.URL.Path, "/conversations.history") && r.URL.Query().Get("channel") == "C_STALE":
			fmt.Fprint(w, `{"ok": false, "error": "channel_not_found"}`)
		case strings.HasSuffix(r.URL.Path, "/conversations.history") && r.URL.Query().Get("channel") == "C_LIVE":
			fmt.Fprint(w, `{"ok": true, "messages": [
				{"type": "message", "user": "U1", "text": "healthy channel message", "ts": "1752000200.000300"}
			]}`)
		case strings.HasSuffix(r.URL.Path, "/users.info"):
			fmt.Fprint(w, `{"ok": true, "user": {"name": "kari"}}`)
		default:
			t.Errorf("unexpected slack call: %s?%s", r.URL.Path, r.URL.RawQuery)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "1752000000.000100", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("a stale channel must not abort the connection: %v", err)
	}
	if len(result.Messages) != 1 || result.Messages[0].ProviderThreadID != "C_LIVE" {
		t.Fatalf("messages = %+v, want the healthy channel message", result.Messages)
	}
	cursorState, err := decodeSlackCursor(result.NextCursor, 24*time.Hour)
	if err != nil {
		t.Fatalf("decode cursor: %v", err)
	}
	if cursorState.Channels["C_STALE"] != "1752000000.000100" {
		t.Fatalf("stale cursor = %q, want the prior channel watermark", cursorState.Channels["C_STALE"])
	}
	if cursorState.Channels["C_LIVE"] != "1752000200.000300" {
		t.Fatalf("healthy cursor = %q, want independent progress", cursorState.Channels["C_LIVE"])
	}
}

func TestSlack_UnavailableChannelErrorsAreExplicitlyClassified(t *testing.T) {
	for _, code := range []string{"channel_not_found", "not_in_channel", "is_archived"} {
		t.Run(code, func(t *testing.T) {
			if !isSlackUnavailableChannel(&slackAPIError{Method: "conversations.history", Code: code}) {
				t.Fatalf("%s must be isolated to its channel", code)
			}
		})
	}
	if isSlackUnavailableChannel(&slackAPIError{Method: "conversations.history", Code: "invalid_auth"}) {
		t.Fatal("authentication errors must fail the connection")
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
			cursorState, decodeErr := decodeSlackCursor(result.NextCursor, 24*time.Hour)
			if decodeErr != nil {
				t.Fatalf("decode cursor: %v", decodeErr)
			}
			if cursorState.Channels["C1"] != "1752000000.000100" {
				t.Errorf("cursor = %+v, must not move the rate-limited channel", cursorState.Channels)
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
	result, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if !strings.HasSuffix(gotOldest, ".000000") {
		t.Fatalf("bootstrap oldest = %q, want a slack ts string", gotOldest)
	}
	if slackTSLess(gotOldest, fmt.Sprintf("%d.000000", before.Add(-time.Minute).Unix())) {
		t.Errorf("bootstrap oldest = %q, want ~24h ago", gotOldest)
	}
	cursorState, err := decodeSlackCursor(result.NextCursor, 24*time.Hour)
	if err != nil {
		t.Fatalf("decode cursor: %v", err)
	}
	if cursorState.Channels["C1"] != gotOldest {
		t.Errorf("stored channel cursor = %q, want stable bootstrap watermark %q", cursorState.Channels["C1"], gotOldest)
	}
}

func TestSlack_PaginatesConversationListAndFullChannelHistory(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/users.conversations"):
			if r.URL.Query().Get("cursor") == "list-page-2" {
				fmt.Fprint(w, `{"ok": true, "channels": [{"id": "C2", "name": "sales"}]}`)
				return
			}
			fmt.Fprint(w, `{
				"ok": true,
				"channels": [{"id": "C1", "name": "support"}],
				"response_metadata": {"next_cursor": "list-page-2"}
			}`)
		case strings.HasSuffix(r.URL.Path, "/conversations.history"):
			channel := r.URL.Query().Get("channel")
			if channel == "C1" && r.URL.Query().Get("oldest") == "1752000100.000100" {
				fmt.Fprint(w, `{"ok": true, "messages": [
					{"type": "message", "text": "newest", "ts": "1752000300.000300"},
					{"type": "message", "text": "middle", "ts": "1752000200.000200"}
				]}`)
				return
			}
			if channel == "C1" && r.URL.Query().Get("latest") == "1752000200.000200" {
				fmt.Fprint(w, `{"ok": true, "messages": [
					{"type": "message", "text": "oldest", "ts": "1752000100.000100"}
				]}`)
				return
			}
			if channel == "C1" {
				fmt.Fprint(w, `{
					"ok": true,
					"has_more": true,
					"messages": [
						{"type": "message", "text": "newest", "ts": "1752000300.000300"},
						{"type": "message", "text": "middle", "ts": "1752000200.000200"}
					],
					"response_metadata": {"next_cursor": "history-page-2"}
				}`)
				return
			}
			if r.URL.Query().Get("oldest") == "1752000400.000400" {
				fmt.Fprint(w, `{"ok": true, "messages": []}`)
				return
			}
			fmt.Fprint(w, `{"ok": true, "messages": [
				{"type": "message", "text": "sales", "ts": "1752000400.000400"}
			]}`)
		case strings.HasSuffix(r.URL.Path, "/users.info"):
			fmt.Fprint(w, `{"ok": true, "user": {}}`)
		default:
			t.Errorf("unexpected slack call: %s?%s", r.URL.Path, r.URL.RawQuery)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "1752000000.000000", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want the oldest C1 page and C2", len(result.Messages))
	}
	if result.Messages[0].BodyText != "oldest" || result.Messages[1].ProviderThreadID != "C2" {
		t.Fatalf("paged order/coverage = %+v", result.Messages)
	}
	cursorState, err := decodeSlackCursor(result.NextCursor, 24*time.Hour)
	if err != nil {
		t.Fatalf("decode cursor: %v", err)
	}
	if cursorState.Channels["C1"] != "1752000100.000100" || cursorState.Channels["C2"] != "1752000400.000400" {
		t.Fatalf("paged cursors = %+v", cursorState.Channels)
	}
	resumed, err := f.Fetch(context.Background(), "tok", result.NextCursor, 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("resumed Fetch: %v", err)
	}
	if len(resumed.Messages) != 2 || resumed.Messages[0].BodyText != "middle" || resumed.Messages[1].BodyText != "newest" {
		t.Fatalf("resumed coverage = %+v", resumed.Messages)
	}
	resumedState, err := decodeSlackCursor(resumed.NextCursor, 24*time.Hour)
	if err != nil || resumedState.Channels["C1"] != "1752000300.000300" {
		t.Fatalf("resumed cursor = %+v, err=%v", resumedState, err)
	}
}

func TestSlack_LargeBacklogPersistsBoundedScanAndResumes(t *testing.T) {
	requestCount := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, "/users.conversations") {
			fmt.Fprint(w, `{"ok": true, "channels": [{"id": "C1", "name": "support"}]}`)
			return
		}
		if !strings.HasSuffix(r.URL.Path, "/conversations.history") {
			t.Fatalf("unexpected request: %s", r.URL.Path)
		}
		requestCount++
		latest := r.URL.Query().Get("latest")
		if requestCount <= slackMaxPages {
			ts := fmt.Sprintf("175200%04d.000000", slackMaxPages-requestCount+1)
			fmt.Fprintf(w, `{"ok":true,"has_more":true,"messages":[{"type":"message","text":"scan","ts":%q}]}`, ts)
			return
		}
		if latest == "" {
			t.Fatal("resumed request must carry the durable scan boundary")
		}
		fmt.Fprint(w, `{"ok":true,"messages":[{"type":"message","text":"oldest","ts":"1752000001.000000"}]}`)
	}))
	defer server.Close()

	f := &SlackFetcher{BaseURL: server.URL, HTTP: server.Client()}
	first, err := f.Fetch(context.Background(), "tok", "1752000000.000000", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("first Fetch: %v", err)
	}
	if len(first.Messages) != 0 {
		t.Fatalf("first cycle messages = %d, want scan-only progress", len(first.Messages))
	}
	firstState, err := decodeSlackCursor(first.NextCursor, 24*time.Hour)
	if err != nil || firstState.ScanBefore["C1"] == "" {
		t.Fatalf("durable scan state = %+v, err=%v", firstState, err)
	}

	second, err := f.Fetch(context.Background(), "tok", first.NextCursor, 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("second Fetch: %v", err)
	}
	if len(second.Messages) != 1 || second.Messages[0].BodyText != "oldest" {
		t.Fatalf("resumed messages = %+v", second.Messages)
	}
	secondState, err := decodeSlackCursor(second.NextCursor, 24*time.Hour)
	if err != nil || secondState.ScanBefore["C1"] != "" || secondState.Channels["C1"] != "1752000001.000000" {
		t.Fatalf("resumed cursor = %+v, err=%v", secondState, err)
	}
}
