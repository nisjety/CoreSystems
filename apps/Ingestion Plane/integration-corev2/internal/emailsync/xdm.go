package emailsync

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"
)

// XDMFetcher polls the connected account's direct messages via X API v2
// GET /2/dm_events using the connection's OAuth2 user token. The cursor is a
// single watermark — the max MessageCreate event id emitted (numeric-string
// compare: length, then lexicographic) — so it never expires. DM access
// requires a licensed API tier (Pro or above); 402/403 replies surface as the
// typed errXDMNotLicensed so the worker records the failure once per cycle on
// the connection's sync state without crashing.
type XDMFetcher struct {
	BaseURL string // default https://api.x.com
	HTTP    *http.Client
}

const xdmDefaultBaseURL = "https://api.x.com"

// xdmMaxResults is the max_results applied to the dm_events call.
const xdmMaxResults = 100

// errXDMNotLicensed marks 402/403 tier rejections from /2/dm_events.
var errXDMNotLicensed = errors.New("x dm access requires a licensed api tier (pro or above)")

func (f *XDMFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	base := f.BaseURL
	if base == "" {
		base = xdmDefaultBaseURL
	}
	query := url.Values{}
	query.Set("dm_event.fields", "id,text,event_type,created_at,sender_id,dm_conversation_id")
	query.Set("max_results", fmt.Sprintf("%d", xdmMaxResults))

	var response struct {
		Data []xdmEvent `json:"data"`
	}
	fullURL := strings.TrimRight(base, "/") + "/2/dm_events?" + query.Encode()
	if err := providerGetJSON(ctx, f.HTTP, accessToken, fullURL, &response); err != nil {
		var httpErr *providerHTTPError
		if asProviderHTTPError(err, &httpErr) && (httpErr.Status == http.StatusPaymentRequired || httpErr.Status == http.StatusForbidden) {
			return FetchResult{}, fmt.Errorf("x dm_events returned %d: %w", httpErr.Status, errXDMNotLicensed)
		}
		return FetchResult{}, fmt.Errorf("x dm_events: %w", err)
	}

	since := time.Now().UTC().Add(-backfill)
	var events []xdmEvent
	for _, event := range response.Data {
		if !event.qualifies(cursor, since) {
			continue
		}
		events = append(events, event)
	}
	// dm_events returns newest-first; emit oldest-first and let the watermark
	// carry the remainder into the next cycle when the cap interrupts.
	slices.SortFunc(events, func(a, b xdmEvent) int {
		if a.ID == b.ID {
			return 0
		}
		if numericIDLess(a.ID, b.ID) {
			return -1
		}
		return 1
	})
	if len(events) > maxMessages {
		events = events[:maxMessages]
	}

	nextCursor := cursor
	messages := make([]EmailMessage, 0, len(events))
	for _, event := range events {
		messages = append(messages, event.normalize())
		if nextCursor == "" || numericIDLess(nextCursor, event.ID) {
			nextCursor = event.ID
		}
	}
	return FetchResult{Messages: messages, NextCursor: nextCursor}, nil
}

type xdmEvent struct {
	ID               string `json:"id"`
	EventType        string `json:"event_type"`
	Text             string `json:"text"`
	CreatedAt        string `json:"created_at"`
	SenderID         string `json:"sender_id"`
	DMConversationID string `json:"dm_conversation_id"`
}

// qualifies filters to MessageCreate events newer than the watermark; on
// bootstrap (empty cursor) the backfill window bounds the initial pull.
func (e xdmEvent) qualifies(cursor string, since time.Time) bool {
	if e.ID == "" || e.EventType != "MessageCreate" || strings.TrimSpace(e.Text) == "" {
		return false
	}
	if cursor == "" {
		return !e.occurredAt().Before(since)
	}
	return numericIDLess(cursor, e.ID)
}

func (e xdmEvent) normalize() EmailMessage {
	return EmailMessage{
		ProviderEventID:   e.ID,
		ProviderMessageID: e.ID,
		ProviderThreadID:  e.DMConversationID,
		Subject:           "(direct message)",
		From:              Participant{Name: e.SenderID},
		BodyText:          strings.TrimSpace(e.Text),
		OccurredAt:        e.occurredAt(),
	}
}

func (e xdmEvent) occurredAt() time.Time {
	parsed, err := time.Parse(time.RFC3339, e.CreatedAt)
	if err != nil {
		return time.Now().UTC()
	}
	return parsed.UTC()
}

// numericIDLess compares numeric-string ids (X event ids, Discord snowflakes)
// without overflow: shorter means smaller, equal length falls back to lex.
func numericIDLess(a, b string) bool {
	if len(a) != len(b) {
		return len(a) < len(b)
	}
	return a < b
}
