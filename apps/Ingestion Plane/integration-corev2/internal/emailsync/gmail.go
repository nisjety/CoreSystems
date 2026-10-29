package emailsync

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/mail"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

// GmailFetcher syncs a Gmail inbox using the documented client-sync model:
// bootstrap = users.getProfile (fresh historyId) + a bounded messages.list
// backfill; incremental = users.history.list?startHistoryId=<cursor> with
// historyTypes=messageAdded&labelId=INBOX, fetching each new message via
// users.messages.get?format=full. A 404 from history.list means the stored
// historyId is older than Gmail's retained history (~1 week) → ErrCursorExpired.
type GmailFetcher struct {
	BaseURL string // default https://gmail.googleapis.com
	HTTP    *http.Client
}

const gmailDefaultBaseURL = "https://gmail.googleapis.com"

// gmailMaxHistoryPages bounds one cycle's history pagination so a huge burst
// cannot wedge a cycle; the cursor advances per processed record, so the
// remainder is picked up next cycle.
const gmailMaxHistoryPages = 10

func (f *GmailFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	if cursor == "" {
		return f.bootstrap(ctx, accessToken, backfill, maxMessages)
	}
	return f.incremental(ctx, accessToken, cursor, maxMessages)
}

// FetchConnection fills a missing mailbox label from Google's profile endpoint
// while it already has an authorized mailbox token. Failure to enrich the
// operator label never blocks mail ingestion: the fetch result remains the
// authoritative sync outcome.
func (f *GmailFetcher) FetchConnection(ctx context.Context, conn store.Connection, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	result, err := f.Fetch(ctx, accessToken, cursor, backfill, maxMessages)
	if err != nil || !connectionNeedsMailboxAddress(conn) || result.ProviderContextPatch["mailbox_address"] != "" {
		return result, err
	}
	address, profileErr := f.mailboxAddress(ctx, accessToken)
	if profileErr == nil && address != "" {
		result.ProviderContextPatch = map[string]string{"mailbox_address": address}
	}
	return result, nil
}

// bootstrap pins the cursor to the mailbox's CURRENT historyId first, then
// backfills recent inbox messages. Ordering matters: pinning first means any
// message arriving during the backfill is replayed by the next incremental
// pass instead of being lost (duplicates are absorbed by ingest idempotency).
func (f *GmailFetcher) bootstrap(ctx context.Context, accessToken string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	var profile struct {
		EmailAddress string `json:"emailAddress"`
		HistoryID    string `json:"historyId"`
	}
	if err := f.getJSON(ctx, accessToken, "/gmail/v1/users/me/profile", &profile); err != nil {
		return FetchResult{}, fmt.Errorf("gmail profile: %w", err)
	}
	if profile.HistoryID == "" {
		return FetchResult{}, fmt.Errorf("gmail profile returned no historyId")
	}

	days := max(int(backfill.Hours()/24)+1, 1)
	query := url.Values{}
	query.Set("labelIds", "INBOX")
	query.Set("maxResults", strconv.Itoa(maxMessages))
	query.Set("q", fmt.Sprintf("newer_than:%dd", days))
	var list struct {
		Messages []struct {
			ID string `json:"id"`
		} `json:"messages"`
	}
	if err := f.getJSON(ctx, accessToken, "/gmail/v1/users/me/messages?"+query.Encode(), &list); err != nil {
		return FetchResult{}, fmt.Errorf("gmail backfill list: %w", err)
	}

	messages := make([]EmailMessage, 0, len(list.Messages))
	// messages.list returns newest-first; ingest oldest-first so conversation
	// previews and last_message_at land in natural order.
	for i := len(list.Messages) - 1; i >= 0; i-- {
		msg, ok, err := f.fetchMessage(ctx, accessToken, list.Messages[i].ID)
		if err != nil {
			return FetchResult{}, err
		}
		if ok {
			messages = append(messages, msg)
		}
	}
	return FetchResult{
		Messages:             messages,
		NextCursor:           profile.HistoryID,
		ProviderContextPatch: mailboxAddressPatch(profile.EmailAddress),
	}, nil
}

func (f *GmailFetcher) mailboxAddress(ctx context.Context, accessToken string) (string, error) {
	var profile struct {
		EmailAddress string `json:"emailAddress"`
	}
	if err := f.getJSON(ctx, accessToken, "/gmail/v1/users/me/profile", &profile); err != nil {
		return "", err
	}
	return normalizeMailboxAddress(profile.EmailAddress), nil
}

func connectionNeedsMailboxAddress(conn store.Connection) bool {
	return strings.TrimSpace(conn.ProviderContext["mailbox_address"]) == ""
}

func mailboxAddressPatch(value string) map[string]string {
	if address := normalizeMailboxAddress(value); address != "" {
		return map[string]string{"mailbox_address": address}
	}
	return nil
}

func normalizeMailboxAddress(value string) string {
	address, err := mail.ParseAddress(strings.TrimSpace(value))
	if err != nil {
		return ""
	}
	return strings.ToLower(strings.TrimSpace(address.Address))
}

func (f *GmailFetcher) incremental(ctx context.Context, accessToken, cursor string, maxMessages int) (FetchResult, error) {
	nextCursor := cursor
	seen := map[string]bool{}
	var messages []EmailMessage
	pageToken := ""

	for range gmailMaxHistoryPages {
		query := url.Values{}
		query.Set("startHistoryId", cursor)
		query.Set("historyTypes", "messageAdded")
		query.Set("labelId", "INBOX")
		if pageToken != "" {
			query.Set("pageToken", pageToken)
		}
		var history struct {
			History []struct {
				ID            string `json:"id"`
				MessagesAdded []struct {
					Message struct {
						ID       string   `json:"id"`
						LabelIDs []string `json:"labelIds"`
					} `json:"message"`
				} `json:"messagesAdded"`
			} `json:"history"`
			NextPageToken string `json:"nextPageToken"`
			HistoryID     string `json:"historyId"`
		}
		if err := f.getJSON(ctx, accessToken, "/gmail/v1/users/me/history?"+query.Encode(), &history); err != nil {
			var httpErr *providerHTTPError
			if asProviderHTTPError(err, &httpErr) && httpErr.Status == http.StatusNotFound {
				// startHistoryId is outside Gmail's retained window.
				return FetchResult{}, fmt.Errorf("gmail history expired: %w", ErrCursorExpired)
			}
			return FetchResult{}, fmt.Errorf("gmail history list: %w", err)
		}

		for _, record := range history.History {
			for _, added := range record.MessagesAdded {
				id := added.Message.ID
				if id == "" || seen[id] {
					continue
				}
				seen[id] = true
				if hasLabel(added.Message.LabelIDs, "SENT") || hasLabel(added.Message.LabelIDs, "DRAFT") {
					continue
				}
				msg, ok, err := f.fetchMessage(ctx, accessToken, id)
				if err != nil {
					return FetchResult{Messages: messages, NextCursor: nextCursor}, err
				}
				if ok {
					messages = append(messages, msg)
				}
			}
			// Each history record id is a valid startHistoryId: advancing per
			// record means an interrupted cycle resumes exactly where it
			// stopped instead of replaying the whole window.
			if record.ID != "" {
				nextCursor = record.ID
			}
			if len(messages) >= maxMessages {
				return FetchResult{Messages: messages, NextCursor: nextCursor}, nil
			}
		}

		if history.NextPageToken == "" {
			// Fully drained: pin to the mailbox-current historyId when given.
			if history.HistoryID != "" {
				nextCursor = history.HistoryID
			}
			break
		}
		pageToken = history.NextPageToken
	}
	return FetchResult{Messages: messages, NextCursor: nextCursor}, nil
}

// fetchMessage retrieves and parses one full message. ok=false (without
// error) means the message should be skipped (sent/draft/chat mail).
func (f *GmailFetcher) fetchMessage(ctx context.Context, accessToken, id string) (EmailMessage, bool, error) {
	var raw struct {
		ID           string    `json:"id"`
		ThreadID     string    `json:"threadId"`
		LabelIDs     []string  `json:"labelIds"`
		InternalDate string    `json:"internalDate"`
		Payload      gmailPart `json:"payload"`
	}
	if err := f.getJSON(ctx, accessToken, "/gmail/v1/users/me/messages/"+url.PathEscape(id)+"?format=full", &raw); err != nil {
		return EmailMessage{}, false, fmt.Errorf("gmail message get %s: %w", id, err)
	}
	if hasLabel(raw.LabelIDs, "SENT") || hasLabel(raw.LabelIDs, "DRAFT") {
		return EmailMessage{}, false, nil
	}

	headers := map[string]string{}
	for _, h := range raw.Payload.Headers {
		headers[strings.ToLower(h.Name)] = h.Value
	}

	fromName, fromEmail := parseAddress(headers["from"])
	msg := EmailMessage{
		ProviderEventID:       raw.ID,
		ProviderMessageID:     raw.ID,
		ProviderThreadID:      raw.ThreadID,
		MessageIDHeader:       headers["message-id"],
		ReferencesHeader:      headers["references"],
		InReplyToHeader:       headers["in-reply-to"],
		AutoSubmitted:         headers["auto-submitted"],
		ContentType:           headers["content-type"],
		OutboundCorrelationID: findGmailHeader(raw.Payload, "x-verevon-outbound-intent"),
		Subject:               headers["subject"],
		From:                  Participant{Name: fromName, Email: fromEmail},
		To:                    parseAddressList(headers["to"]),
		OccurredAt:            gmailInternalDate(raw.InternalDate),
	}
	msg.BodyText, msg.BodyHTML = extractGmailBodies(raw.Payload)
	return msg, true, nil
}

// findGmailHeader searches the complete MIME tree. A delivery-status report
// commonly encapsulates the original RFC 822 message as a nested part, where
// Verevon's opaque outbound correlation header resides.
func findGmailHeader(part gmailPart, name string) string {
	name = strings.ToLower(strings.TrimSpace(name))
	for _, header := range part.Headers {
		if strings.ToLower(strings.TrimSpace(header.Name)) == name {
			return strings.TrimSpace(header.Value)
		}
	}
	for _, nested := range part.Parts {
		if value := findGmailHeader(nested, name); value != "" {
			return value
		}
	}
	return ""
}

type gmailPart struct {
	MimeType string `json:"mimeType"`
	Headers  []struct {
		Name  string `json:"name"`
		Value string `json:"value"`
	} `json:"headers"`
	Body struct {
		Data string `json:"data"`
	} `json:"body"`
	Parts []gmailPart `json:"parts"`
}

// extractGmailBodies walks the MIME tree for the first text/plain and
// text/html leaves (attachments have no inline data and are skipped).
func extractGmailBodies(part gmailPart) (text, html string) {
	var walk func(p gmailPart)
	walk = func(p gmailPart) {
		if text != "" && html != "" {
			return
		}
		switch {
		case strings.HasPrefix(p.MimeType, "text/plain") && text == "":
			text = decodeGmailBody(p.Body.Data)
		case strings.HasPrefix(p.MimeType, "text/html") && html == "":
			html = decodeGmailBody(p.Body.Data)
		}
		for _, child := range p.Parts {
			walk(child)
		}
	}
	walk(part)
	return text, html
}

func decodeGmailBody(data string) string {
	if data == "" {
		return ""
	}
	decoded, err := base64.URLEncoding.WithPadding(base64.NoPadding).DecodeString(strings.TrimRight(data, "="))
	if err != nil {
		return ""
	}
	return string(decoded)
}

func gmailInternalDate(ms string) time.Time {
	epoch, err := strconv.ParseInt(ms, 10, 64)
	if err != nil || epoch <= 0 {
		return time.Now().UTC()
	}
	return time.UnixMilli(epoch).UTC()
}

func hasLabel(labels []string, label string) bool {
	return slices.Contains(labels, label)
}

func parseAddress(value string) (name, email string) {
	value = strings.TrimSpace(value)
	if value == "" {
		return "", ""
	}
	addr, err := mail.ParseAddress(value)
	if err != nil {
		return "", strings.Trim(value, "<> ")
	}
	return addr.Name, addr.Address
}

func parseAddressList(value string) []Participant {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil
	}
	addrs, err := mail.ParseAddressList(value)
	if err != nil {
		name, email := parseAddress(value)
		if name == "" && email == "" {
			return nil
		}
		return []Participant{{Name: name, Email: email}}
	}
	out := make([]Participant, 0, len(addrs))
	for _, a := range addrs {
		out = append(out, Participant{Name: a.Name, Email: a.Address})
	}
	return out
}

func (f *GmailFetcher) getJSON(ctx context.Context, accessToken, path string, out any) error {
	base := f.BaseURL
	if base == "" {
		base = gmailDefaultBaseURL
	}
	return providerGetJSON(ctx, f.HTTP, accessToken, strings.TrimRight(base, "/")+path, out)
}

// providerHTTPError carries the status code so callers can map provider
// status semantics (Gmail 404 / Graph 410) to ErrCursorExpired.
type providerHTTPError struct {
	Status int
	Body   string
}

func (e *providerHTTPError) Error() string {
	return fmt.Sprintf("provider returned %d: %s", e.Status, e.Body)
}

func asProviderHTTPError(err error, target **providerHTTPError) bool {
	for err != nil {
		if typed, ok := err.(*providerHTTPError); ok {
			*target = typed
			return true
		}
		err = unwrapOnce(err)
	}
	return false
}

func unwrapOnce(err error) error {
	type unwrapper interface{ Unwrap() error }
	if u, ok := err.(unwrapper); ok {
		return u.Unwrap()
	}
	return nil
}

func providerGetJSON(ctx context.Context, client *http.Client, accessToken, fullURL string, out any) error {
	return providerGetJSONAuth(ctx, client, "Bearer "+accessToken, fullURL, out)
}

// providerGetJSONAuth is providerGetJSON with a caller-supplied Authorization
// value (Discord bot calls use "Bot <token>" instead of Bearer).
func providerGetJSONAuth(ctx context.Context, client *http.Client, authorization, fullURL string, out any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, fullURL, nil)
	if err != nil {
		return fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Authorization", authorization)
	req.Header.Set("Accept", "application/json")
	if client == nil {
		client = &http.Client{Timeout: 20 * time.Second}
	}
	resp, err := client.Do(req)
	if err != nil {
		return fmt.Errorf("request %s: %w", fullURL, err)
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return fmt.Errorf("read response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		return &providerHTTPError{Status: resp.StatusCode, Body: truncateForError(body)}
	}
	if out == nil {
		return nil
	}
	if err := json.Unmarshal(body, out); err != nil {
		return fmt.Errorf("decode response: %w", err)
	}
	return nil
}

func truncateForError(body []byte) string {
	const cap = 512
	trimmed := strings.TrimSpace(string(body))
	if len(trimmed) > cap {
		return trimmed[:cap] + "…"
	}
	return trimmed
}
