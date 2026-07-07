package emailsync

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// GraphFetcher syncs an Outlook inbox with Microsoft Graph delta queries:
// initial call to /me/mailFolders/inbox/messages/delta (filtered to the
// backfill window), following @odata.nextLink pages until @odata.deltaLink
// arrives; the deltaLink is the persisted cursor for the next cycle. When the
// per-cycle cap interrupts a page walk, the pending nextLink is persisted
// instead — both link kinds are valid resumption points per the delta
// contract. 410 Gone (or a SyncStateNotFound 40x) → ErrCursorExpired.
type GraphFetcher struct {
	BaseURL string // default https://graph.microsoft.com/v1.0
	HTTP    *http.Client
}

const graphDefaultBaseURL = "https://graph.microsoft.com/v1.0"

// graphMaxPages bounds one cycle's pagination (the nextLink cursor carries
// the remainder into the next cycle).
const graphMaxPages = 10

func (f *GraphFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	nextURL := cursor
	if nextURL == "" {
		since := time.Now().UTC().Add(-backfill).Format(time.RFC3339)
		query := url.Values{}
		query.Set("changeType", "created")
		query.Set("$filter", fmt.Sprintf("receivedDateTime ge %s", since))
		nextURL = f.baseURL() + "/me/mailFolders/inbox/messages/delta?" + query.Encode()
	}

	var messages []EmailMessage
	for range graphMaxPages {
		var response struct {
			Value     []graphMessage `json:"value"`
			NextLink  string         `json:"@odata.nextLink"`
			DeltaLink string         `json:"@odata.deltaLink"`
		}
		if err := providerGetJSON(ctx, f.HTTP, accessToken, nextURL, &response); err != nil {
			var httpErr *providerHTTPError
			if asProviderHTTPError(err, &httpErr) {
				if httpErr.Status == http.StatusGone || strings.Contains(httpErr.Body, "SyncStateNotFound") || strings.Contains(httpErr.Body, "syncStateNotFound") {
					return FetchResult{}, fmt.Errorf("graph delta state reset: %w", ErrCursorExpired)
				}
			}
			return FetchResult{}, fmt.Errorf("graph delta page: %w", err)
		}

		for _, raw := range response.Value {
			msg, ok := raw.normalize()
			if !ok {
				continue
			}
			messages = append(messages, msg)
			if len(messages) >= maxMessages {
				// Interrupted mid-walk: resume from the pending link next
				// cycle. Prefer nextLink; if this was the final page the
				// deltaLink is already in hand.
				cursorOut := response.NextLink
				if cursorOut == "" {
					cursorOut = response.DeltaLink
				}
				if cursorOut == "" {
					cursorOut = nextURL
				}
				return FetchResult{Messages: messages, NextCursor: cursorOut}, nil
			}
		}

		if response.DeltaLink != "" {
			return FetchResult{Messages: messages, NextCursor: response.DeltaLink}, nil
		}
		if response.NextLink == "" {
			// Defensive: a page without either link should not happen per the
			// contract; keep the current URL so the next cycle retries.
			return FetchResult{Messages: messages, NextCursor: nextURL}, nil
		}
		nextURL = response.NextLink
	}
	// Page budget exhausted: persist the pending nextLink and continue later.
	return FetchResult{Messages: messages, NextCursor: nextURL}, nil
}

func (f *GraphFetcher) baseURL() string {
	if f.BaseURL == "" {
		return graphDefaultBaseURL
	}
	return strings.TrimRight(f.BaseURL, "/")
}

type graphMessage struct {
	ID                string    `json:"id"`
	Removed           *struct{} `json:"@removed"`
	IsDraft           bool      `json:"isDraft"`
	Subject           string    `json:"subject"`
	ConversationID    string    `json:"conversationId"`
	InternetMessageID string    `json:"internetMessageId"`
	ReceivedDateTime  string    `json:"receivedDateTime"`
	BodyPreview       string    `json:"bodyPreview"`
	Body              struct {
		ContentType string `json:"contentType"`
		Content     string `json:"content"`
	} `json:"body"`
	From struct {
		EmailAddress graphEmailAddress `json:"emailAddress"`
	} `json:"from"`
	ToRecipients []struct {
		EmailAddress graphEmailAddress `json:"emailAddress"`
	} `json:"toRecipients"`
}

type graphEmailAddress struct {
	Name    string `json:"name"`
	Address string `json:"address"`
}

// normalize maps a Graph message to the bridge shape. ok=false means skip
// (tombstones from the delta feed, drafts, or bodiless items).
func (m graphMessage) normalize() (EmailMessage, bool) {
	if m.Removed != nil || m.IsDraft || m.ID == "" {
		return EmailMessage{}, false
	}
	msg := EmailMessage{
		ProviderEventID:   m.ID,
		ProviderMessageID: m.ID,
		ProviderThreadID:  m.ConversationID,
		MessageIDHeader:   m.InternetMessageID,
		Subject:           m.Subject,
		From:              Participant{Name: m.From.EmailAddress.Name, Email: m.From.EmailAddress.Address},
		OccurredAt:        graphReceivedAt(m.ReceivedDateTime),
	}
	for _, r := range m.ToRecipients {
		if r.EmailAddress.Name == "" && r.EmailAddress.Address == "" {
			continue
		}
		msg.To = append(msg.To, Participant{Name: r.EmailAddress.Name, Email: r.EmailAddress.Address})
	}
	switch strings.ToLower(m.Body.ContentType) {
	case "html":
		msg.BodyHTML = m.Body.Content
		msg.BodyText = m.BodyPreview
	default:
		msg.BodyText = m.Body.Content
	}
	if msg.BodyText == "" && msg.BodyHTML == "" {
		msg.BodyText = m.BodyPreview
	}
	if msg.BodyText == "" && msg.BodyHTML == "" {
		return EmailMessage{}, false
	}
	return msg, true
}

func graphReceivedAt(value string) time.Time {
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return time.Now().UTC()
	}
	return parsed.UTC()
}
