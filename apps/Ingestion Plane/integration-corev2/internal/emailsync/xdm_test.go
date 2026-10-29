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

func TestXDM_MessageCreateEventsAdvanceWatermark(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.HasSuffix(r.URL.Path, "/2/dm_events") {
			t.Errorf("unexpected x call: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
			return
		}
		if !strings.Contains(r.URL.Query().Get("dm_event.fields"), "dm_conversation_id") {
			t.Errorf("dm_event.fields = %s", r.URL.Query().Get("dm_event.fields"))
		}
		// Newest-first: two fresh MessageCreate events (note "1000" > "999"
		// numerically but not lexicographically), a participant event, and one
		// already behind the watermark.
		fmt.Fprint(w, `{"data": [
			{"id": "1000", "event_type": "MessageCreate", "text": "andre melding", "created_at": "2026-07-18T10:05:00.000Z", "sender_id": "u-77", "dm_conversation_id": "dm-1"},
			{"id": "999", "event_type": "MessageCreate", "text": "første melding", "created_at": "2026-07-18T10:00:00.000Z", "sender_id": "u-77", "dm_conversation_id": "dm-1"},
			{"id": "998", "event_type": "ParticipantsJoin", "text": "", "created_at": "2026-07-18T09:59:00.000Z", "sender_id": "u-77", "dm_conversation_id": "dm-1"},
			{"id": "500", "event_type": "MessageCreate", "text": "gammel", "created_at": "2026-07-18T08:00:00.000Z", "sender_id": "u-77", "dm_conversation_id": "dm-1"}
		]}`)
	}))
	defer server.Close()

	f := &XDMFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "500", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2 (participant event and watermarked skipped): %+v", len(result.Messages), result.Messages)
	}
	// Numeric id ordering: oldest-first despite "999" > "1000" lexicographically.
	if result.Messages[0].ProviderEventID != "999" || result.Messages[1].ProviderEventID != "1000" {
		t.Errorf("order: %s, %s", result.Messages[0].ProviderEventID, result.Messages[1].ProviderEventID)
	}
	if result.NextCursor != "1000" {
		t.Errorf("cursor = %q, want max event id by numeric compare", result.NextCursor)
	}
	msg := result.Messages[0]
	if msg.ProviderThreadID != "dm-1" || msg.Subject != "(direct message)" {
		t.Errorf("thread mapping: %+v", msg)
	}
	if msg.From.Name != "u-77" {
		t.Errorf("from = %q, want the raw sender id", msg.From.Name)
	}
	if msg.BodyText != "første melding" {
		t.Errorf("body = %q", msg.BodyText)
	}
	if !msg.OccurredAt.Equal(time.Date(2026, 7, 18, 10, 0, 0, 0, time.UTC)) {
		t.Errorf("occurredAt = %v", msg.OccurredAt)
	}
}

func TestXDM_TierRejectionIsTypedError(t *testing.T) {
	tests := []struct {
		name   string
		status int
	}{
		{name: "402 payment required", status: http.StatusPaymentRequired},
		{name: "403 forbidden", status: http.StatusForbidden},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				http.Error(w, `{"title": "Client Forbidden"}`, test.status)
			}))
			defer server.Close()

			f := &XDMFetcher{BaseURL: server.URL, HTTP: server.Client()}
			_, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25)
			if !errors.Is(err, errXDMNotLicensed) {
				t.Fatalf("err = %v, want errXDMNotLicensed", err)
			}
		})
	}
}

func TestXDM_BootstrapBoundsByBackfillWindow(t *testing.T) {
	old := time.Now().UTC().Add(-48 * time.Hour).Format(time.RFC3339)
	fresh := time.Now().UTC().Add(-time.Hour).Format(time.RFC3339)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		fmt.Fprintf(w, `{"data": [
			{"id": "20", "event_type": "MessageCreate", "text": "fersk", "created_at": %q, "sender_id": "u-1", "dm_conversation_id": "dm-1"},
			{"id": "10", "event_type": "MessageCreate", "text": "utenfor vinduet", "created_at": %q, "sender_id": "u-1", "dm_conversation_id": "dm-1"}
		]}`, fresh, old)
	}))
	defer server.Close()

	f := &XDMFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 1 || result.Messages[0].ProviderEventID != "20" {
		t.Fatalf("messages = %+v, want only the in-window event", result.Messages)
	}
	if result.NextCursor != "20" {
		t.Errorf("cursor = %q, want 20", result.NextCursor)
	}
}
