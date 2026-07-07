package emailsync

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func b64url(s string) string {
	return base64.URLEncoding.WithPadding(base64.NoPadding).EncodeToString([]byte(s))
}

// ── Gmail ────────────────────────────────────────────────────────────────────

func gmailMessageJSON(id, thread, from, subject, bodyText string, labels ...string) string {
	labelJSON := `"INBOX"`
	if len(labels) > 0 {
		quoted := make([]string, len(labels))
		for i, l := range labels {
			quoted[i] = fmt.Sprintf("%q", l)
		}
		labelJSON = strings.Join(quoted, ",")
	}
	return fmt.Sprintf(`{
		"id": %q, "threadId": %q, "labelIds": [%s], "internalDate": "1751968800000",
		"payload": {
			"mimeType": "multipart/alternative",
			"headers": [
				{"name": "From", "value": %q},
				{"name": "To", "value": "Support <support@velion.no>"},
				{"name": "Subject", "value": %q},
				{"name": "Message-ID", "value": "<%s@mail.example>"},
				{"name": "In-Reply-To", "value": "<root@mail.example>"}
			],
			"parts": [
				{"mimeType": "text/plain", "body": {"data": %q}},
				{"mimeType": "text/html", "body": {"data": %q}}
			]
		}
	}`, id, thread, labelJSON, from, subject, id, b64url(bodyText), b64url("<p>"+bodyText+"</p>"))
}

func TestGmail_BootstrapPinsCursorAndBackfills(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/profile"):
			fmt.Fprint(w, `{"historyId": "4711", "emailAddress": "owner@example.com"}`)
		case strings.HasSuffix(r.URL.Path, "/messages"):
			if r.URL.Query().Get("labelIds") != "INBOX" || !strings.HasPrefix(r.URL.Query().Get("q"), "newer_than:") {
				t.Errorf("backfill query missing filters: %s", r.URL.RawQuery)
			}
			fmt.Fprint(w, `{"messages": [{"id": "new2"}, {"id": "new1"}]}`)
		case strings.Contains(r.URL.Path, "/messages/new1"):
			fmt.Fprint(w, gmailMessageJSON("new1", "t1", "Kari Norman <kari@x.no>", "Hjelp", "Første melding"))
		case strings.Contains(r.URL.Path, "/messages/new2"):
			fmt.Fprint(w, gmailMessageJSON("new2", "t1", "Kari Norman <kari@x.no>", "Re: Hjelp", "Andre melding"))
		default:
			t.Errorf("unexpected gmail call: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	f := &GmailFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if result.NextCursor != "4711" {
		t.Errorf("cursor = %q, want profile historyId 4711", result.NextCursor)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2", len(result.Messages))
	}
	// list is newest-first; ingest order must be oldest-first
	if result.Messages[0].ProviderEventID != "new1" || result.Messages[1].ProviderEventID != "new2" {
		t.Errorf("wrong order: %s, %s", result.Messages[0].ProviderEventID, result.Messages[1].ProviderEventID)
	}
	first := result.Messages[0]
	if first.From.Email != "kari@x.no" || first.From.Name != "Kari Norman" {
		t.Errorf("from = %+v", first.From)
	}
	if first.BodyText != "Første melding" || !strings.Contains(first.BodyHTML, "<p>") {
		t.Errorf("bodies not decoded: text=%q html=%q", first.BodyText, first.BodyHTML)
	}
	if first.ProviderThreadID != "t1" || first.MessageIDHeader != "<new1@mail.example>" || first.InReplyToHeader != "<root@mail.example>" {
		t.Errorf("threading fields wrong: %+v", first)
	}
	if !first.OccurredAt.Equal(time.UnixMilli(1751968800000).UTC()) {
		t.Errorf("occurredAt = %v", first.OccurredAt)
	}
}

func TestGmail_IncrementalSkipsSentAndAdvancesPerRecord(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, "/history"):
			if r.URL.Query().Get("startHistoryId") != "100" {
				t.Errorf("startHistoryId = %s", r.URL.Query().Get("startHistoryId"))
			}
			fmt.Fprint(w, `{
				"historyId": "300",
				"history": [
					{"id": "150", "messagesAdded": [{"message": {"id": "cust", "labelIds": ["INBOX"]}}]},
					{"id": "200", "messagesAdded": [{"message": {"id": "own", "labelIds": ["SENT"]}}]}
				]
			}`)
		case strings.Contains(r.URL.Path, "/messages/cust"):
			fmt.Fprint(w, gmailMessageJSON("cust", "t9", "c@x.no", "Sak", "Innhold"))
		default:
			t.Errorf("unexpected call %s (SENT message must not be fetched)", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	f := &GmailFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "100", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 1 || result.Messages[0].ProviderEventID != "cust" {
		t.Fatalf("messages: %+v", result.Messages)
	}
	if result.NextCursor != "300" {
		t.Errorf("cursor = %q, want drained mailbox historyId 300", result.NextCursor)
	}
}

func TestGmail_History404IsCursorExpired(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error": {"code": 404, "message": "Requested entity was not found."}}`, http.StatusNotFound)
	}))
	defer server.Close()

	f := &GmailFetcher{BaseURL: server.URL, HTTP: server.Client()}
	_, err := f.Fetch(context.Background(), "tok", "ancient", 24*time.Hour, 25)
	if !errors.Is(err, ErrCursorExpired) {
		t.Fatalf("err = %v, want ErrCursorExpired", err)
	}
}

// ── Graph ────────────────────────────────────────────────────────────────────

func graphMessageJSON(id, subject, fromEmail, bodyHTML string, extra string) string {
	return fmt.Sprintf(`{
		"id": %q, "subject": %q, "conversationId": "conv-1",
		"internetMessageId": "<%s@outlook>", "receivedDateTime": "2026-07-08T09:00:00Z",
		"bodyPreview": "preview text",
		"body": {"contentType": "html", "content": %q},
		"from": {"emailAddress": {"name": "Ola", "address": %q}},
		"toRecipients": [{"emailAddress": {"name": "Support", "address": "support@velion.no"}}]%s
	}`, id, subject, id, bodyHTML, fromEmail, extra)
}

func TestGraph_InitialDeltaWalksToDeltaLink(t *testing.T) {
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.Contains(r.URL.RawQuery, "changeType=created"):
			if !strings.Contains(r.URL.RawQuery, "%24filter=receivedDateTime") && !strings.Contains(r.URL.RawQuery, "$filter=receivedDateTime") {
				t.Errorf("initial delta missing receivedDateTime filter: %s", r.URL.RawQuery)
			}
			fmt.Fprintf(w, `{"value": [%s], "@odata.nextLink": %q}`,
				graphMessageJSON("g1", "Sak 1", "ola@x.no", "<p>hei</p>", ""),
				server.URL+"/page2")
		case strings.HasSuffix(r.URL.Path, "/page2"):
			fmt.Fprintf(w, `{"value": [%s, %s, %s], "@odata.deltaLink": %q}`,
				graphMessageJSON("g2", "Sak 2", "ola@x.no", "<p>mer</p>", ""),
				`{"id": "gone", "@removed": {"reason": "deleted"}}`,
				`{"id": "draft1", "isDraft": true, "subject": "utkast", "body": {"contentType": "text", "content": "x"}, "from": {"emailAddress": {"address": "me@x.no"}}}`,
				server.URL+"/delta-final")
		default:
			t.Errorf("unexpected graph call: %s?%s", r.URL.Path, r.URL.RawQuery)
			w.WriteHeader(http.StatusNotFound)
		}
	}))
	defer server.Close()

	f := &GraphFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 25)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want 2 (tombstone + draft skipped): %+v", len(result.Messages), result.Messages)
	}
	if result.NextCursor != server.URL+"/delta-final" {
		t.Errorf("cursor = %q, want the deltaLink", result.NextCursor)
	}
	msg := result.Messages[0]
	if msg.ProviderThreadID != "conv-1" || msg.MessageIDHeader != "<g1@outlook>" {
		t.Errorf("threading fields: %+v", msg)
	}
	if msg.BodyHTML != "<p>hei</p>" || msg.BodyText != "preview text" {
		t.Errorf("bodies: text=%q html=%q", msg.BodyText, msg.BodyHTML)
	}
	if msg.From.Email != "ola@x.no" || len(msg.To) != 1 || msg.To[0].Email != "support@velion.no" {
		t.Errorf("participants: %+v", msg)
	}
}

func TestGraph_CapMidWalkPersistsNextLink(t *testing.T) {
	var server *httptest.Server
	server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, "/page2") {
			t.Error("page2 must not be fetched once the cap is hit")
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		fmt.Fprintf(w, `{"value": [%s, %s], "@odata.nextLink": %q}`,
			graphMessageJSON("g1", "S1", "a@x.no", "<p>1</p>", ""),
			graphMessageJSON("g2", "S2", "b@x.no", "<p>2</p>", ""),
			server.URL+"/page2")
	}))
	defer server.Close()

	f := &GraphFetcher{BaseURL: server.URL, HTTP: server.Client()}
	result, err := f.Fetch(context.Background(), "tok", "", 24*time.Hour, 2)
	if err != nil {
		t.Fatalf("Fetch: %v", err)
	}
	if len(result.Messages) != 2 {
		t.Fatalf("messages = %d, want capped 2", len(result.Messages))
	}
	if result.NextCursor != server.URL+"/page2" {
		t.Errorf("cursor = %q, want pending nextLink for resumption", result.NextCursor)
	}
}

func TestGraph_410GoneIsCursorExpired(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusGone)
		fmt.Fprint(w, `{"error": {"code": "syncStateNotFound"}}`)
	}))
	defer server.Close()

	f := &GraphFetcher{BaseURL: server.URL, HTTP: server.Client()}
	_, err := f.Fetch(context.Background(), "tok", server.URL+"/stale-delta", 24*time.Hour, 25)
	if !errors.Is(err, ErrCursorExpired) {
		t.Fatalf("err = %v, want ErrCursorExpired", err)
	}
}
