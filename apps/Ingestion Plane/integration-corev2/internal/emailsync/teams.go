package emailsync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"html"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

// TeamsFetcher polls Microsoft Teams chats and joined-team channels over
// delegated Graph (no protected-API app approval needed): GET /me/chats then
// /chats/{id}/messages per chat, plus /me/joinedTeams → /teams/{id}/channels
// → channel messages. The cursor keeps a watermark per chat/channel plus a
// bounded-scan position. Per-thread progress is required because a single
// global maximum can permanently skip an older chat when a batch or Graph-call
// cap is reached first. Duplicates from re-reads are absorbed by
// conversation-core's idempotency key.
type TeamsFetcher struct {
	BaseURL string // default https://graph.microsoft.com/v1.0
	HTTP    *http.Client
}

const (
	teamsMemberCursorVersion = "teams-members-v4"
	teamsBootstrapBackfill   = 30 * 24 * time.Hour
)

func (f *TeamsFetcher) CursorVersion() string { return teamsMemberCursorVersion }

// teamsMaxGraphCalls caps the Graph requests one cycle may issue across chat
// and channel listing + message pages, so a member of many chats/teams cannot
// drive the worker into Graph throttling; whatever the cap leaves unvisited
// is picked up on a later cycle (the watermark only advances past emitted
// messages).
const teamsMaxGraphCalls = 30

// teamsPageSize is the $top applied to chat/channel listings and message pages.
const teamsPageSize = 50

func (f *TeamsFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	return f.fetch(ctx, accessToken, cursor, backfill, maxMessages, teamsIdentity{})
}

func (f *TeamsFetcher) FetchConnection(ctx context.Context, conn store.Connection, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	if strings.TrimSpace(cursor) == "" && backfill < teamsBootstrapBackfill {
		backfill = teamsBootstrapBackfill
	}
	return f.fetch(ctx, accessToken, cursor, backfill, maxMessages, teamsIdentity{
		userID: conn.ProviderAccountID,
		name:   conn.DisplayName,
		email:  conn.UserEmail,
	})
}

func (f *TeamsFetcher) fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int, identity teamsIdentity) (FetchResult, error) {
	cursorState := decodeTeamsCursor(cursor, backfill)
	sync := &teamsSync{
		fetcher:     f,
		accessToken: accessToken,
		cursor:      cursorState,
		maxMessages: maxMessages,
		budget:      teamsMaxGraphCalls,
		identity:    identity,
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
	return FetchResult{Messages: sync.messages, NextCursor: encodeTeamsCursor(sync.cursor)}, nil
}

type teamsIdentity struct {
	userID string
	name   string
	email  string
}

type teamsChatMember struct {
	UserID      string `json:"userId"`
	DisplayName string `json:"displayName"`
	Email       string `json:"email"`
}

type teamsCursorState struct {
	Bootstrap      string              `json:"bootstrap"`
	HistoryDays    int                 `json:"history_days,omitempty"`
	Threads        map[string]string   `json:"threads,omitempty"`
	MessagePages   map[string]string   `json:"message_pages,omitempty"`
	MessageStacks  map[string][]string `json:"message_stacks,omitempty"`
	MessageReplays map[string]bool     `json:"message_replays,omitempty"`
	Phase          string              `json:"phase,omitempty"`
	ChatPage       string              `json:"chat_page,omitempty"`
	ChatIndex      int                 `json:"chat_index,omitempty"`
	TeamPage       string              `json:"team_page,omitempty"`
	TeamIndex      int                 `json:"team_index,omitempty"`
	ChannelPage    string              `json:"channel_page,omitempty"`
	ChannelTeamID  string              `json:"channel_team_id,omitempty"`
	ChannelIndex   int                 `json:"channel_index,omitempty"`
}

func decodeTeamsCursor(cursor string, backfill time.Duration) teamsCursorState {
	return decodeTeamsCursorAt(cursor, backfill, time.Now().UTC())
}

func decodeTeamsCursorAt(cursor string, backfill time.Duration, now time.Time) teamsCursorState {
	requestedDays := max(int((backfill+24*time.Hour-1)/(24*time.Hour)), int(teamsBootstrapBackfill/(24*time.Hour)))
	fallback := now.UTC().Add(-time.Duration(requestedDays) * 24 * time.Hour)
	state := teamsCursorState{
		Bootstrap:      fallback.Format(time.RFC3339Nano),
		HistoryDays:    requestedDays,
		Threads:        map[string]string{},
		MessagePages:   map[string]string{},
		MessageStacks:  map[string][]string{},
		MessageReplays: map[string]bool{},
		Phase:          "chats",
	}
	trimmed := strings.TrimSpace(cursor)
	if trimmed == "" {
		return state
	}
	if parsed, err := time.Parse(time.RFC3339, trimmed); err == nil {
		state.Bootstrap = parsed.UTC().Format(time.RFC3339Nano)
		state.HistoryDays = int(teamsBootstrapBackfill / (24 * time.Hour))
		if requestedDays > state.HistoryDays {
			return newTeamsCursor(now, requestedDays)
		}
		return state
	}
	var decoded teamsCursorState
	if err := json.Unmarshal([]byte(trimmed), &decoded); err != nil {
		return state
	}
	state.HistoryDays = max(decoded.HistoryDays, int(teamsBootstrapBackfill/(24*time.Hour)))
	if parsed, err := time.Parse(time.RFC3339, decoded.Bootstrap); err == nil {
		state.Bootstrap = parsed.UTC().Format(time.RFC3339Nano)
	}
	if decoded.Threads != nil {
		state.Threads = make(map[string]string, len(decoded.Threads))
		for key, value := range decoded.Threads {
			if parsed, err := time.Parse(time.RFC3339, value); err == nil {
				state.Threads[key] = parsed.UTC().Format(time.RFC3339Nano)
			}
		}
	}
	for key, value := range decoded.MessagePages {
		if strings.TrimSpace(value) != "" {
			state.MessagePages[key] = value
		}
	}
	for key, values := range decoded.MessageStacks {
		state.MessageStacks[key] = append([]string(nil), values...)
	}
	for key, value := range decoded.MessageReplays {
		if value {
			state.MessageReplays[key] = true
		}
	}
	if decoded.Phase == "channels" {
		state.Phase = "channels"
	}
	state.ChatPage = decoded.ChatPage
	state.ChatIndex = max(decoded.ChatIndex, 0)
	state.TeamPage = decoded.TeamPage
	state.TeamIndex = max(decoded.TeamIndex, 0)
	state.ChannelPage = decoded.ChannelPage
	state.ChannelTeamID = decoded.ChannelTeamID
	state.ChannelIndex = max(decoded.ChannelIndex, 0)
	if requestedDays > state.HistoryDays {
		return newTeamsCursor(now, requestedDays)
	}
	return state
}

func newTeamsCursor(now time.Time, historyDays int) teamsCursorState {
	return teamsCursorState{
		Bootstrap:      now.UTC().Add(-time.Duration(historyDays) * 24 * time.Hour).Format(time.RFC3339Nano),
		HistoryDays:    historyDays,
		Threads:        map[string]string{},
		MessagePages:   map[string]string{},
		MessageStacks:  map[string][]string{},
		MessageReplays: map[string]bool{},
		Phase:          "chats",
	}
}

func encodeTeamsCursor(state teamsCursorState) string {
	encoded, err := json.Marshal(state)
	if err != nil {
		return state.Bootstrap
	}
	return string(encoded)
}

func (s teamsCursorState) watermark(threadKey string) time.Time {
	value := s.Threads[threadKey]
	if value == "" {
		value = s.Bootstrap
	}
	parsed, err := time.Parse(time.RFC3339, value)
	if err != nil {
		return time.Time{}
	}
	return parsed.UTC()
}

func (s *teamsCursorState) advance(threadKey string, occurredAt time.Time) {
	if !occurredAt.After(s.watermark(threadKey)) {
		return
	}
	if s.Threads == nil {
		s.Threads = map[string]string{}
	}
	s.Threads[threadKey] = occurredAt.UTC().Format(time.RFC3339Nano)
}

// teamsSync carries one cycle's state: the shared watermark, the message cap,
// and the remaining Graph call budget.
type teamsSync struct {
	fetcher     *TeamsFetcher
	accessToken string
	cursor      teamsCursorState
	maxMessages int
	budget      int
	messages    []EmailMessage
	identity    teamsIdentity
}

func (s *teamsSync) pollChats(ctx context.Context) error {
	if s.done() || s.cursor.Phase == "channels" {
		return nil
	}
	for !s.done() {
		var chats struct {
			Value []struct {
				ID       string            `json:"id"`
				Topic    string            `json:"topic"`
				ChatType string            `json:"chatType"`
				Members  []teamsChatMember `json:"members"`
			} `json:"value"`
			NextLink string `json:"@odata.nextLink"`
		}
		pagePath := s.cursor.ChatPage
		if pagePath == "" {
			pagePath = fmt.Sprintf("/me/chats?$top=%d&$expand=members", teamsPageSize)
		}
		if err := s.getJSON(ctx, pagePath, &chats); err != nil {
			return fmt.Errorf("teams chats list: %w", err)
		}
		if s.cursor.ChatIndex >= len(chats.Value) {
			s.cursor.ChatIndex = 0
		}
		for index := s.cursor.ChatIndex; index < len(chats.Value); index++ {
			if s.done() {
				return nil
			}
			chat := chats.Value[index]
			members := s.resolveChatMembers(chat.Members)
			subject := teamsChatSubject(chat.Topic, members.counterparts)
			path := fmt.Sprintf("/chats/%s/messages?$top=%d", url.PathEscape(chat.ID), teamsPageSize)
			complete, err := s.collectMessages(ctx, path, "chat:"+chat.ID, chat.ID, subject, members)
			if err != nil {
				return fmt.Errorf("teams chat %s messages: %w", chat.ID, err)
			}
			if !complete {
				s.cursor.ChatIndex = index
				return nil
			}
			s.cursor.ChatIndex = index + 1
		}
		s.cursor.ChatIndex = 0
		if strings.TrimSpace(chats.NextLink) != "" {
			s.cursor.ChatPage = chats.NextLink
			continue
		}
		s.cursor.ChatPage = ""
		s.cursor.Phase = "channels"
		return nil
	}
	return nil
}

func (s *teamsSync) pollChannels(ctx context.Context) error {
	if s.done() {
		return nil
	}
	if s.cursor.Phase != "channels" {
		return nil
	}
	for !s.done() {
		var teams struct {
			Value []struct {
				ID string `json:"id"`
			} `json:"value"`
			NextLink string `json:"@odata.nextLink"`
		}
		teamPagePath := s.cursor.TeamPage
		if teamPagePath == "" {
			teamPagePath = "/me/joinedTeams"
		}
		if err := s.getJSON(ctx, teamPagePath, &teams); err != nil {
			return fmt.Errorf("teams joined list: %w", err)
		}
		if s.cursor.TeamIndex >= len(teams.Value) {
			s.cursor.TeamIndex = 0
			s.cursor.ChannelIndex = 0
			s.cursor.ChannelPage = ""
			s.cursor.ChannelTeamID = ""
		}
		for teamIndex := s.cursor.TeamIndex; teamIndex < len(teams.Value); teamIndex++ {
			if s.done() {
				return nil
			}
			team := teams.Value[teamIndex]
			if s.cursor.ChannelPage != "" && s.cursor.ChannelTeamID != "" {
				team.ID = s.cursor.ChannelTeamID
			}
			for !s.done() {
				var channels struct {
					Value []struct {
						ID          string `json:"id"`
						DisplayName string `json:"displayName"`
					} `json:"value"`
					NextLink string `json:"@odata.nextLink"`
				}
				channelPagePath := s.cursor.ChannelPage
				if channelPagePath == "" {
					channelPagePath = "/teams/" + url.PathEscape(team.ID) + "/channels"
				}
				if err := s.getJSON(ctx, channelPagePath, &channels); err != nil {
					return fmt.Errorf("teams %s channels list: %w", team.ID, err)
				}
				if s.cursor.ChannelIndex >= len(channels.Value) {
					s.cursor.ChannelIndex = 0
				}
				for channelIndex := s.cursor.ChannelIndex; channelIndex < len(channels.Value); channelIndex++ {
					if s.done() {
						return nil
					}
					channel := channels.Value[channelIndex]
					subject := "(channel)"
					if strings.TrimSpace(channel.DisplayName) != "" {
						subject = "#" + strings.TrimSpace(channel.DisplayName)
					}
					path := fmt.Sprintf("/teams/%s/channels/%s/messages?$top=%d", url.PathEscape(team.ID), url.PathEscape(channel.ID), teamsPageSize)
					threadKey := "channel:" + team.ID + ":" + channel.ID
					complete, err := s.collectMessages(ctx, path, threadKey, channel.ID, subject, resolvedTeamsMembers{})
					if err != nil {
						var providerErr *providerHTTPError
						if errors.As(err, &providerErr) && (providerErr.Status == http.StatusForbidden || providerErr.Status == http.StatusNotFound) {
							// Graph may list private/shared channels whose messages this
							// delegated user cannot read. Isolate that channel instead of
							// discarding already-fetched private chats for the whole cycle.
							s.cursor.TeamIndex = teamIndex
							s.cursor.ChannelIndex = channelIndex + 1
							continue
						}
						return fmt.Errorf("teams channel %s messages: %w", channel.ID, err)
					}
					if !complete {
						s.cursor.TeamIndex = teamIndex
						s.cursor.ChannelIndex = channelIndex
						return nil
					}
					s.cursor.TeamIndex = teamIndex
					s.cursor.ChannelIndex = channelIndex + 1
				}
				s.cursor.ChannelIndex = 0
				if strings.TrimSpace(channels.NextLink) != "" {
					s.cursor.ChannelPage = channels.NextLink
					s.cursor.ChannelTeamID = team.ID
					continue
				}
				s.cursor.ChannelPage = ""
				s.cursor.ChannelTeamID = ""
				break
			}
			if s.cursor.ChannelPage != "" {
				return nil
			}
			s.cursor.TeamIndex = teamIndex + 1
		}
		s.cursor.TeamIndex = 0
		if strings.TrimSpace(teams.NextLink) != "" {
			s.cursor.TeamPage = teams.NextLink
			continue
		}
		s.cursor.TeamPage = ""
		s.cursor.Phase = "chats"
		return nil
	}
	return nil
}

// collectMessages fetches one thread's newest page and appends qualifying
// messages oldest-first, so hitting maxMessages mid-thread never advances the
// watermark past an unemitted message in that thread.
func (s *teamsSync) collectMessages(ctx context.Context, path, threadKey, threadID, subject string, members resolvedTeamsMembers) (bool, error) {
	var page struct {
		Value    []teamsMessage `json:"value"`
		NextLink string         `json:"@odata.nextLink"`
	}
	pagePath := s.cursor.MessagePages[threadKey]
	if pagePath == "" {
		pagePath = path
	}
	if err := s.getJSON(ctx, pagePath, &page); err != nil {
		return false, err
	}
	watermark := s.cursor.watermark(threadKey)
	if !s.cursor.MessageReplays[threadKey] && pageEntirelyAfter(page.Value, watermark) && strings.TrimSpace(page.NextLink) != "" {
		s.cursor.MessageStacks[threadKey] = append(s.cursor.MessageStacks[threadKey], pagePath)
		s.cursor.MessagePages[threadKey] = page.NextLink
		return false, nil
	}
	// Graph returns messages newest-first; walk in reverse for natural order.
	for i := len(page.Value) - 1; i >= 0; i-- {
		if s.done() {
			s.cursor.MessagePages[threadKey] = pagePath
			s.cursor.MessageReplays[threadKey] = true
			return false, nil
		}
		msg, ok := page.Value[i].normalize(threadID, subject, watermark, s.identity, members)
		if !ok {
			continue
		}
		s.messages = append(s.messages, msg)
		s.cursor.advance(threadKey, msg.OccurredAt)
	}
	stack := s.cursor.MessageStacks[threadKey]
	if len(stack) > 0 {
		nextIndex := len(stack) - 1
		s.cursor.MessagePages[threadKey] = stack[nextIndex]
		s.cursor.MessageStacks[threadKey] = append([]string(nil), stack[:nextIndex]...)
		s.cursor.MessageReplays[threadKey] = true
		return false, nil
	}
	delete(s.cursor.MessagePages, threadKey)
	delete(s.cursor.MessageStacks, threadKey)
	delete(s.cursor.MessageReplays, threadKey)
	return true, nil
}

func pageEntirelyAfter(messages []teamsMessage, watermark time.Time) bool {
	for _, message := range messages {
		occurredAt, err := time.Parse(time.RFC3339, message.CreatedDateTime)
		if err == nil && !occurredAt.After(watermark) {
			return false
		}
	}
	return true
}

type resolvedTeamsMembers struct {
	byID         map[string]Participant
	selfIDs      map[string]struct{}
	counterparts []Participant
}

func (s *teamsSync) resolveChatMembers(members []teamsChatMember) resolvedTeamsMembers {
	resolved := resolvedTeamsMembers{byID: make(map[string]Participant), selfIDs: make(map[string]struct{})}
	for _, member := range members {
		participant := Participant{Name: strings.TrimSpace(member.DisplayName), Email: strings.TrimSpace(member.Email)}
		resolved.byID[member.UserID] = participant
		if sameTeamsUser(member, s.identity) {
			resolved.selfIDs[member.UserID] = struct{}{}
			continue
		}
		if participant.Name != "" || participant.Email != "" {
			resolved.counterparts = append(resolved.counterparts, participant)
		}
	}
	return resolved
}

func sameTeamsUser(member teamsChatMember, identity teamsIdentity) bool {
	return identity.userID != "" && strings.EqualFold(member.UserID, identity.userID) ||
		identity.email != "" && strings.EqualFold(member.Email, identity.email) ||
		identity.userID == "" && identity.email == "" && identity.name != "" && strings.EqualFold(member.DisplayName, identity.name)
}

func teamsChatSubject(topic string, counterparts []Participant) string {
	if subject := strings.TrimSpace(topic); subject != "" {
		return subject
	}
	names := make([]string, 0, len(counterparts))
	for _, counterpart := range counterparts {
		name := strings.TrimSpace(counterpart.Name)
		if name == "" {
			name = strings.TrimSpace(counterpart.Email)
		}
		if name != "" {
			names = append(names, name)
		}
	}
	if len(names) > 0 {
		return strings.Join(names, ", ")
	}
	return "(chat)"
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
	endpoint, err := trustedTeamsEndpoint(base, path)
	if err != nil {
		return err
	}
	client := s.fetcher.HTTP
	if client == nil {
		client = &http.Client{Timeout: 20 * time.Second}
	}
	redirectSafeClient := *client
	redirectSafeClient.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	// 401/403 (revoked consent, missing Teams scopes) surface as the typed
	// *providerHTTPError from providerGetJSON; the worker records them on the
	// connection's sync state via recordFailure.
	return providerGetJSON(ctx, &redirectSafeClient, s.accessToken, endpoint, out)
}

func trustedTeamsEndpoint(base, path string) (string, error) {
	const maxTeamsURLLength = 8 * 1024
	if len(path) == 0 || len(path) > maxTeamsURLLength {
		return "", errors.New("reject Teams pagination URL: invalid length")
	}
	baseURL, err := url.Parse(base)
	if err != nil || baseURL.Scheme == "" || baseURL.Host == "" || baseURL.User != nil {
		return "", errors.New("reject Teams Graph base URL")
	}
	endpoint := path
	if !strings.HasPrefix(path, "https://") && !strings.HasPrefix(path, "http://") {
		endpoint = strings.TrimRight(base, "/") + path
	}
	endpointURL, err := url.Parse(endpoint)
	if err != nil || endpointURL.User != nil || endpointURL.Fragment != "" ||
		!strings.EqualFold(endpointURL.Scheme, baseURL.Scheme) || !strings.EqualFold(endpointURL.Host, baseURL.Host) {
		return "", errors.New("reject off-origin Teams pagination URL")
	}
	return endpointURL.String(), nil
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
func (m teamsMessage) normalize(threadID, subject string, watermark time.Time, identity teamsIdentity, members resolvedTeamsMembers) (EmailMessage, bool) {
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
	from := Participant{Name: m.From.User.DisplayName}
	if participant, ok := members.byID[m.From.User.ID]; ok {
		from = participant
	}
	direction := "inbound"
	to := []Participant(nil)
	_, resolvedAsSelf := members.selfIDs[m.From.User.ID]
	if resolvedAsSelf || identity.userID != "" && strings.EqualFold(m.From.User.ID, identity.userID) {
		direction = "outbound"
		from = Participant{Name: identity.name, Email: identity.email}
		to = append(to, members.counterparts...)
	}
	msg := EmailMessage{
		ProviderEventID:   m.ID,
		ProviderMessageID: m.ID,
		ProviderThreadID:  threadID,
		Subject:           subject,
		Direction:         direction,
		From:              from,
		To:                to,
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
