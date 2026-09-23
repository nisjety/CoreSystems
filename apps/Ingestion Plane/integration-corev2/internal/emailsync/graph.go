package emailsync

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

// GraphFetcher syncs an Outlook inbox with Microsoft Graph delta queries:
// initial call to /me/mailFolders/inbox/messages/delta (filtered to the
// backfill window), following @odata.nextLink pages until @odata.deltaLink
// arrives; the deltaLink is the persisted cursor for the next cycle. When the
// per-cycle cap interrupts a page walk, the pending nextLink is persisted
// instead — both link kinds are valid resumption points per the delta
// contract. 410 Gone (or a SyncStateNotFound 40x) → ErrCursorExpired.
type GraphFetcher struct {
	BaseURL      string // default https://graph.microsoft.com/v1.0
	HTTP         *http.Client
	FullBackfill bool // omit the initial receivedDateTime filter
}

const graphDefaultBaseURL = "https://graph.microsoft.com/v1.0"

const graphFullBackfillCursorVersion = "graph-full-v1"

// graphMaxPages bounds one cycle's pagination (the nextLink cursor carries
// the remainder into the next cycle).
const graphMaxPages = 10

func (f *GraphFetcher) CursorVersion() string {
	if f.FullBackfill {
		return graphFullBackfillCursorVersion
	}
	return ""
}

func (f *GraphFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	nextURL := cursor
	if nextURL == "" {
		query := url.Values{}
		query.Set("changeType", "created")
		// Graph returns internetMessageHeaders only when explicitly selected.
		// The selection is encoded into the delta token for later pages, so this
		// one initial request preserves the evidence contract for the full cycle.
		query.Set("$select", "id,conversationId,internetMessageId,internetMessageHeaders,receivedDateTime,subject,bodyPreview,body,from,toRecipients,isDraft")
		if !f.FullBackfill {
			since := time.Now().UTC().Add(-backfill).Format(time.RFC3339)
			query.Set("$filter", fmt.Sprintf("receivedDateTime ge %s", since))
		}
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

		// Both link kinds are provider-response-derived URLs that get
		// re-dialed (NextLink, on the next loop iteration) or persisted and
		// re-dialed later (either one, as the next cycle's starting cursor
		// on line ~43). Confining them to the configured Graph origin here —
		// once per page, immediately after decoding — covers every later use
		// in this function, including the cursor persisted when maxMessages
		// is reached below.
		nextLink, err := f.safeNextLink(response.NextLink)
		if err != nil {
			return FetchResult{}, fmt.Errorf("graph delta pagination: %w", err)
		}
		deltaLink, err := f.safeNextLink(response.DeltaLink)
		if err != nil {
			return FetchResult{}, fmt.Errorf("graph delta pagination: %w", err)
		}

		for _, raw := range response.Value {
			msg, ok := raw.normalize()
			if !ok {
				continue
			}
			messages = append(messages, msg)
		}

		// Graph cursors only identify page boundaries. Stopping in the middle
		// of response.Value and saving nextLink would permanently skip the
		// unprocessed remainder of this page. Finish the provider page, then
		// allow a bounded overshoot of maxMessages before resuming next cycle.
		if len(messages) >= maxMessages {
			cursorOut := nextLink
			if cursorOut == "" {
				cursorOut = deltaLink
			}
			if cursorOut == "" {
				cursorOut = nextURL
			}
			return FetchResult{Messages: messages, NextCursor: cursorOut}, nil
		}

		if deltaLink != "" {
			return FetchResult{Messages: messages, NextCursor: deltaLink}, nil
		}
		if nextLink == "" {
			// Defensive: a page without either link should not happen per the
			// contract; keep the current URL so the next cycle retries.
			return FetchResult{Messages: messages, NextCursor: nextURL}, nil
		}
		nextURL = nextLink
	}
	// Page budget exhausted: persist the pending nextLink and continue later.
	return FetchResult{Messages: messages, NextCursor: nextURL}, nil
}

// FetchConnection records the mailbox identity returned by Graph when the
// durable connection does not have one yet. It is intentionally best effort:
// a missing User.Read grant must not turn a successful Mail.Read sync into a
// failed one.
func (f *GraphFetcher) FetchConnection(ctx context.Context, conn store.Connection, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	result, err := f.Fetch(ctx, accessToken, cursor, backfill, maxMessages)
	if err != nil || !connectionNeedsMailboxAddress(conn) {
		return result, err
	}
	address, profileErr := f.mailboxAddress(ctx, accessToken)
	if profileErr == nil && address != "" {
		result.ProviderContextPatch = map[string]string{"mailbox_address": address}
	}
	return result, nil
}

func (f *GraphFetcher) mailboxAddress(ctx context.Context, accessToken string) (string, error) {
	var profile struct {
		Mail              string `json:"mail"`
		UserPrincipalName string `json:"userPrincipalName"`
	}
	if err := providerGetJSON(ctx, f.HTTP, accessToken, f.baseURL()+"/me?$select=mail,userPrincipalName", &profile); err != nil {
		return "", err
	}
	if address := normalizeMailboxAddress(profile.Mail); address != "" {
		return address, nil
	}
	return normalizeMailboxAddress(profile.UserPrincipalName), nil
}

func (f *GraphFetcher) baseURL() string {
	if f.BaseURL == "" {
		return graphDefaultBaseURL
	}
	return strings.TrimRight(f.BaseURL, "/")
}

// safeNextLink confines a Graph-returned @odata.nextLink/@odata.deltaLink to
// the configured Graph origin before it is dialed on a later call. Graph is
// expected to hand back pagination links on the same host it was called on;
// a value that is not an absolute URL on that same scheme+host — or isn't a
// valid URL at all — is refused here instead of being handed to
// providerGetJSON, which would otherwise dial whatever host the response
// named. An empty link (nothing to follow) passes through unchanged.
func (f *GraphFetcher) safeNextLink(raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", nil
	}
	base, err := url.Parse(f.baseURL())
	if err != nil {
		return "", fmt.Errorf("parse Graph base URL: %w", err)
	}
	next, err := url.Parse(raw)
	if err != nil || !next.IsAbs() || next.Scheme != base.Scheme || next.Host != base.Host || next.User != nil {
		return "", fmt.Errorf("graph returned an off-origin pagination URL")
	}
	next.Fragment = ""
	return next.String(), nil
}

type graphMessage struct {
	ID                     string                       `json:"id"`
	Removed                *struct{}                    `json:"@removed"`
	IsDraft                bool                         `json:"isDraft"`
	Subject                string                       `json:"subject"`
	ConversationID         string                       `json:"conversationId"`
	InternetMessageID      string                       `json:"internetMessageId"`
	InternetMessageHeaders []graphInternetMessageHeader `json:"internetMessageHeaders"`
	ReceivedDateTime       string                       `json:"receivedDateTime"`
	BodyPreview            string                       `json:"bodyPreview"`
	Body                   struct {
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

type graphInternetMessageHeader struct {
	Name  string `json:"name"`
	Value string `json:"value"`
}

type graphEmailAddress struct {
	Name    string `json:"name"`
	Address string `json:"address"`
}

// normalize maps a Graph message to the bridge shape. ok=false means skip
// tombstones from the delta feed or drafts.
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
	headers := graphHeaderMap(m.InternetMessageHeaders)
	msg.ReferencesHeader = headers["references"]
	msg.InReplyToHeader = headers["in-reply-to"]
	msg.AutoSubmitted = headers["auto-submitted"]
	msg.ContentType = headers["content-type"]
	msg.OutboundCorrelationID = headers["x-verevon-outbound-intent"]
	for _, r := range m.ToRecipients {
		if r.EmailAddress.Name == "" && r.EmailAddress.Address == "" {
			continue
		}
		msg.To = append(msg.To, Participant{Name: r.EmailAddress.Name, Email: r.EmailAddress.Address})
	}
	switch strings.ToLower(m.Body.ContentType) {
	case "html":
		msg.BodyHTML = m.Body.Content
		// Render the real body rather than storing Graph's bodyPreview:
		// Microsoft caps that field at 255 characters, so using it made
		// body_text a PREVIEW for every HTML mail while the full content
		// sat in body_html. Everything that reads text instead of markup --
		// search, classification, draft generation, the chat inbox tools --
		// then saw only the opening sentence of each message, with no way
		// to tell that it had. The Teams path in this package already
		// rendered its HTML this way; only the mail path did not.
		msg.BodyText = stripHTMLTags(m.Body.Content)
		if msg.BodyText == "" {
			// An image-only or empty-markup body renders to nothing. The
			// preview is then the best text that exists, and is not a
			// truncation of anything.
			msg.BodyText = m.BodyPreview
		}
	default:
		msg.BodyText = m.Body.Content
	}
	if msg.BodyText == "" && msg.BodyHTML == "" {
		msg.BodyText = m.BodyPreview
	}
	if msg.BodyText == "" && msg.BodyHTML == "" {
		// Valid Outlook items can be attachment-only or calendar/system messages.
		// Keep them in the inbox and satisfy the canonical bridge's body contract.
		msg.BodyText = "(No message body)"
	}
	return msg, true
}

func graphHeaderMap(headers []graphInternetMessageHeader) map[string]string {
	values := make(map[string]string, len(headers))
	for _, header := range headers {
		name := strings.ToLower(strings.TrimSpace(header.Name))
		if name == "" {
			continue
		}
		if _, exists := values[name]; !exists {
			values[name] = strings.TrimSpace(header.Value)
		}
	}
	return values
}

func graphReceivedAt(value string) time.Time {
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return time.Now().UTC()
	}
	return parsed.UTC()
}
