package emailsync

import (
	"context"
	"fmt"
	"html"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"
)

// TeamsFetcher polls Microsoft Teams chats and joined-team channels over
// delegated Graph (no protected-API app approval needed): GET /me/chats then
// /chats/{id}/messages per chat, plus /me/joinedTeams → /teams/{id}/channels
// → channel messages. The cursor is a single RFC3339 watermark — the max
// createdDateTime emitted across all chats and channels — so it never
// "expires" (ErrCursorExpired does not apply; a corrupt cursor just falls
// back to the backfill window). Duplicates from watermark re-reads are
// absorbed by conversation-core's idempotency key.
type TeamsFetcher struct {
	BaseURL string // default https://graph.microsoft.com/v1.0
	HTTP    *http.Client
}

// teamsMaxGraphCalls caps the Graph requests one cycle may issue across chat
// and channel listing + message pages, so a member of many chats/teams cannot
// drive the worker into Graph throttling; whatever the cap leaves unvisited
// is picked up on a later cycle (the watermark only advances past emitted
// messages).
const teamsMaxGraphCalls = 30

// teamsPageSize is the $top applied to chat/channel listings and message pages.
const teamsPageSize = 50

func (f *TeamsFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	watermark := teamsWatermark(cursor, backfill)
	sync := &teamsSync{
		fetcher:     f,
		accessToken: accessToken,
		watermark:   watermark,
		// nextWatermark never regresses: an empty cycle persists the incoming
		// watermark unchanged.
		nextWatermark: watermark,
		maxMessages:   maxMessages,
		budget:        teamsMaxGraphCalls,
	}
	if err := sync.pollChats(ctx); err != nil {
		return FetchResult{}, err
	}
	if err := sync.pollChannels(ctx); err != nil {
		return FetchResult{}, err
	}
	slices.SortFunc(sync.messages, func(a, b EmailMessage) int {
		return a.OccurredAt.Compare(b.OccurredAt)
	})
	return FetchResult{Messages: sync.messages, NextCursor: sync.nextWatermark.Format(time.RFC3339Nano)}, nil
}

// teamsWatermark parses the stored RFC3339 watermark; an empty (bootstrap) or
// unparseable cursor falls back to the backfill window.
func teamsWatermark(cursor string, backfill time.Duration) time.Time {
	if cursor != "" {
		if parsed, err := time.Parse(time.RFC3339, cursor); err == nil {
			return parsed.UTC()
		}
	}
	return time.Now().UTC().Add(-backfill)
}

// teamsSync carries one cycle's state: the shared watermark, the message cap,
// and the remaining Graph call budget.
type teamsSync struct {
	fetcher       *TeamsFetcher
	accessToken   string
	watermark     time.Time
	nextWatermark time.Time
	maxMessages   int
	budget        int
	messages      []EmailMessage
}

func (s *teamsSync) pollChats(ctx context.Context) error {
	if s.done() {
		return nil
	}
	var chats struct {
		Value []struct {
			ID       string `json:"id"`
			Topic    string `json:"topic"`
			ChatType string `json:"chatType"`
		} `json:"value"`
	}
	if err := s.getJSON(ctx, fmt.Sprintf("/me/chats?$top=%d", teamsPageSize), &chats); err != nil {
		return fmt.Errorf("teams chats list: %w", err)
	}
	for _, chat := range chats.Value {
		if s.done() {
			return nil
		}
		subject := strings.TrimSpace(chat.Topic)
		if subject == "" {
			subject = "(chat)"
		}
		path := fmt.Sprintf("/chats/%s/messages?$top=%d", url.PathEscape(chat.ID), teamsPageSize)
		if err := s.collectMessages(ctx, path, chat.ID, subject); err != nil {
			return fmt.Errorf("teams chat %s messages: %w", chat.ID, err)
		}
	}
	return nil
}

func (s *teamsSync) pollChannels(ctx context.Context) error {
	if s.done() {
		return nil
	}
	var teams struct {
		Value []struct {
			ID string `json:"id"`
		} `json:"value"`
	}
	if err := s.getJSON(ctx, "/me/joinedTeams", &teams); err != nil {
		return fmt.Errorf("teams joined list: %w", err)
	}
	for _, team := range teams.Value {
		if s.done() {
			return nil
		}
		var channels struct {
			Value []struct {
				ID          string `json:"id"`
				DisplayName string `json:"displayName"`
			} `json:"value"`
		}
		if err := s.getJSON(ctx, "/teams/"+url.PathEscape(team.ID)+"/channels", &channels); err != nil {
			return fmt.Errorf("teams %s channels list: %w", team.ID, err)
		}
		for _, channel := range channels.Value {
			if s.done() {
				return nil
			}
			subject := "(channel)"
			if strings.TrimSpace(channel.DisplayName) != "" {
				subject = "#" + strings.TrimSpace(channel.DisplayName)
			}
			path := fmt.Sprintf("/teams/%s/channels/%s/messages?$top=%d", url.PathEscape(team.ID), url.PathEscape(channel.ID), teamsPageSize)
			if err := s.collectMessages(ctx, path, channel.ID, subject); err != nil {
				return fmt.Errorf("teams channel %s messages: %w", channel.ID, err)
			}
		}
	}
	return nil
}

// collectMessages fetches one thread's newest page and appends qualifying
// messages oldest-first, so hitting maxMessages mid-thread never advances the
// watermark past an unemitted message in that thread.
func (s *teamsSync) collectMessages(ctx context.Context, path, threadID, subject string) error {
	var page struct {
		Value []teamsMessage `json:"value"`
	}
	if err := s.getJSON(ctx, path, &page); err != nil {
		return err
	}
	// Graph returns messages newest-first; walk in reverse for natural order.
	for i := len(page.Value) - 1; i >= 0; i-- {
		if s.done() {
			return nil
		}
		msg, ok := page.Value[i].normalize(threadID, subject, s.watermark)
		if !ok {
			continue
		}
		s.messages = append(s.messages, msg)
		if msg.OccurredAt.After(s.nextWatermark) {
			s.nextWatermark = msg.OccurredAt
		}
	}
	return nil
}

func (s *teamsSync) done() bool {
	return s.budget <= 0 || len(s.messages) >= s.maxMessages
}

func (s *teamsSync) getJSON(ctx context.Context, path string, out any) error {
	s.budget--
	base := s.fetcher.BaseURL
	if base == "" {
		base = graphDefaultBaseURL
	}
	// 401/403 (revoked consent, missing Teams scopes) surface as the typed
	// *providerHTTPError from providerGetJSON; the worker records them on the
	// connection's sync state via recordFailure.
	return providerGetJSON(ctx, s.fetcher.HTTP, s.accessToken, strings.TrimRight(base, "/")+path, out)
}

type teamsMessage struct {
	ID              string `json:"id"`
	MessageType     string `json:"messageType"`
	CreatedDateTime string `json:"createdDateTime"`
	From            *struct {
		User *struct {
			ID          string `json:"id"`
			DisplayName string `json:"displayName"`
		} `json:"user"`
	} `json:"from"`
	Body struct {
		ContentType string `json:"contentType"`
		Content     string `json:"content"`
	} `json:"body"`
}

// normalize maps one Teams message to the bridge shape. ok=false means skip:
// system events (messageType != "message" or from.user null), empty bodies,
// and messages at or before the watermark. The bridge requires body_text, so
// HTML content also produces a tag-stripped plain-text rendering.
func (m teamsMessage) normalize(threadID, subject string, watermark time.Time) (EmailMessage, bool) {
	if m.ID == "" || m.MessageType != "message" {
		return EmailMessage{}, false
	}
	if m.From == nil || m.From.User == nil {
		return EmailMessage{}, false
	}
	content := strings.TrimSpace(m.Body.Content)
	if content == "" {
		return EmailMessage{}, false
	}
	createdAt, err := time.Parse(time.RFC3339, m.CreatedDateTime)
	if err != nil || !createdAt.After(watermark) {
		return EmailMessage{}, false
	}
	msg := EmailMessage{
		ProviderEventID:   m.ID,
		ProviderMessageID: m.ID,
		ProviderThreadID:  threadID,
		Subject:           subject,
		From:              Participant{Name: m.From.User.DisplayName},
		OccurredAt:        createdAt.UTC(),
	}
	if strings.EqualFold(m.Body.ContentType, "html") {
		msg.BodyHTML = content
		msg.BodyText = stripHTMLTags(content)
	} else {
		msg.BodyText = content
	}
	if msg.BodyText == "" {
		return EmailMessage{}, false
	}
	return msg, true
}

// stripHTMLTags is a minimal HTML→text rendering for the required body_text
// field: drop tags, unescape entities, collapse surrounding whitespace.
func stripHTMLTags(s string) string {
	var b strings.Builder
	inTag := false
	for _, r := range s {
		switch {
		case r == '<':
			inTag = true
		case r == '>':
			inTag = false
		case !inTag:
			b.WriteRune(r)
		}
	}
	return strings.TrimSpace(html.UnescapeString(b.String()))
}
