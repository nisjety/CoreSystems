package emailsync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/store"
)

// DiscordFetcher polls guild text channels over the Discord REST API using
// the app-level bot token (DISCORD_BOT_TOKEN) — the connection's OAuth token
// only proves the install, the bot reads the messages. The guild to poll
// comes exclusively from the connection's provider_context["guild_id"]. A
// connection without a verified guild fails closed: polling every guild visible
// to a shared bot could attribute another tenant's messages to this connection.
// The durable cursor keeps an independent snowflake watermark per channel and
// a bounded backward-scan checkpoint for large backlogs.
type DiscordFetcher struct {
	BaseURL  string // default https://discord.com/api/v10
	BotToken string
	HTTP     *http.Client
}

const discordDefaultBaseURL = "https://discord.com/api/v10"

// discordPageSize is the limit applied to channel message calls.
const discordPageSize = 100

// discordMaxPages bounds provider work per cycle. A scan checkpoint is
// persisted when the bound is reached, so the next cycle resumes safely.
const discordMaxPages = 100

// discordEpochMS is the Discord snowflake epoch (2015-01-01T00:00:00Z).
const discordEpochMS = 1420070400000

// discordGuildTextChannel is Discord channel type 0 (GUILD_TEXT).
const discordGuildTextChannel = 0

// errDiscordBotNotConfigured marks a missing app-level bot token; the worker
// records it once per cycle on the connection's sync state without spamming.
var errDiscordBotNotConfigured = errors.New("discord bot token not configured (set DISCORD_BOT_TOKEN)")

// errDiscordGuildNotConfigured prevents a shared bot installation from being
// used without an explicit tenant-owned guild binding.
var errDiscordGuildNotConfigured = errors.New("discord connection guild not configured")

// errDiscordGuildNotAuthorized means the OAuth user cannot administer the
// requested guild. The shared bot token is never used for a guild until this
// user-bound authorization check succeeds.
var errDiscordGuildNotAuthorized = errors.New("discord connection guild is not authorized by the OAuth user")

const discordManageGuildPermission uint64 = 1 << 5

// Fetch satisfies Fetcher for interface compatibility. Discord requires the
// ConnectionFetcher path so provider_context can enforce the guild boundary.
func (f *DiscordFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	return f.FetchConnection(ctx, store.Connection{}, accessToken, cursor, backfill, maxMessages)
}

// FetchConnection implements ConnectionFetcher: the connection carries the
// guild scoping. The OAuth access token is ignored — all reads use the bot
// token.
func (f *DiscordFetcher) FetchConnection(ctx context.Context, conn store.Connection, accessToken string, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	if strings.TrimSpace(f.BotToken) == "" {
		return FetchResult{}, errDiscordBotNotConfigured
	}
	needsGuildBinding := strings.TrimSpace(conn.ProviderContext["guild_id"]) == ""
	guildIDs, err := f.guildIDs(ctx, conn, accessToken)
	if err != nil {
		return FetchResult{}, err
	}

	cursorState, err := decodeDiscordCursor(cursor, backfill)
	if err != nil {
		return FetchResult{}, fmt.Errorf("discord cursor: %w", err)
	}

	var messages []EmailMessage
	for _, guildID := range guildIDs {
		if len(messages) >= maxMessages {
			break
		}
		var channels []struct {
			ID   string `json:"id"`
			Name string `json:"name"`
			Type int    `json:"type"`
		}
		if err := f.getJSON(ctx, "/guilds/"+url.PathEscape(guildID)+"/channels", &channels); err != nil {
			return FetchResult{}, fmt.Errorf("discord guild %s channels: %w", guildID, err)
		}
		for _, channel := range channels {
			if channel.Type != discordGuildTextChannel {
				continue
			}
			if len(messages) >= maxMessages {
				break
			}
			watermark := cursorState.Channels[channel.ID]
			if watermark == "" {
				watermark = cursorState.Default
			}
			cursorState.Channels[channel.ID] = watermark
			before := cursorState.ScanBefore[channel.ID]
			var oldestPage []discordMessage
			reachedOldest := false
			for pageNumber := 0; pageNumber < discordMaxPages; pageNumber++ {
				query := url.Values{}
				if before != "" {
					query.Set("before", before)
				} else {
					query.Set("after", watermark)
				}
				query.Set("limit", strconv.Itoa(discordPageSize))
				var page []discordMessage
				if err := f.getJSON(ctx, "/channels/"+url.PathEscape(channel.ID)+"/messages?"+query.Encode(), &page); err != nil {
					return FetchResult{}, fmt.Errorf("discord channel %s messages: %w", channel.ID, err)
				}
				oldestPage = messagesAfterDiscordWatermark(page, watermark)
				if len(oldestPage) == 0 || len(page) < discordPageSize || len(oldestPage) < len(page) {
					reachedOldest = true
					break
				}
				nextBefore := minimumDiscordMessageID(page)
				if nextBefore == "" || nextBefore == before {
					return FetchResult{}, fmt.Errorf("discord channel %s pagination did not progress", channel.ID)
				}
				before = nextBefore
			}
			if !reachedOldest {
				cursorState.ScanBefore[channel.ID] = before
				continue
			}
			delete(cursorState.ScanBefore, channel.ID)

			// Emit oldest-first regardless of the API's ordering so hitting
			// maxMessages never advances the per-channel watermark past an
			// unemitted message in this or any other channel.
			slices.SortFunc(oldestPage, func(a, b discordMessage) int {
				if a.ID == b.ID {
					return 0
				}
				if numericIDLess(a.ID, b.ID) {
					return -1
				}
				return 1
			})
			for _, raw := range oldestPage {
				if len(messages) >= maxMessages {
					break
				}
				if cursorState.Channels[channel.ID] == "" || numericIDLess(cursorState.Channels[channel.ID], raw.ID) {
					cursorState.Channels[channel.ID] = raw.ID
				}
				msg, ok := raw.normalize(channel.ID, channel.Name)
				if !ok {
					continue
				}
				messages = append(messages, msg)
			}
		}
	}
	result := FetchResult{Messages: messages, NextCursor: encodeDiscordCursor(cursorState)}
	if needsGuildBinding {
		result.ProviderContextPatch = map[string]string{"guild_id": guildIDs[0]}
	}
	return result, nil
}

type discordCursorState struct {
	Version    int               `json:"version"`
	Default    string            `json:"default"`
	Channels   map[string]string `json:"channels"`
	ScanBefore map[string]string `json:"scanBefore,omitempty"`
}

func decodeDiscordCursor(raw string, backfill time.Duration) (discordCursorState, error) {
	state := discordCursorState{
		Version:    1,
		Default:    discordSnowflakeForTime(time.Now().UTC().Add(-backfill)),
		Channels:   map[string]string{},
		ScanBefore: map[string]string{},
	}
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return state, nil
	}
	if !strings.HasPrefix(trimmed, "{") {
		state.Default = trimmed
		return state, nil
	}
	if err := json.Unmarshal([]byte(trimmed), &state); err != nil {
		return discordCursorState{}, err
	}
	if state.Version != 1 {
		return discordCursorState{}, fmt.Errorf("unsupported version %d", state.Version)
	}
	if state.Default == "" {
		state.Default = discordSnowflakeForTime(time.Now().UTC().Add(-backfill))
	}
	if state.Channels == nil {
		state.Channels = map[string]string{}
	}
	if state.ScanBefore == nil {
		state.ScanBefore = map[string]string{}
	}
	return state, nil
}

func encodeDiscordCursor(state discordCursorState) string {
	encoded, err := json.Marshal(state)
	if err != nil {
		return state.Default
	}
	return string(encoded)
}

func messagesAfterDiscordWatermark(page []discordMessage, watermark string) []discordMessage {
	filtered := make([]discordMessage, 0, len(page))
	for _, message := range page {
		if message.ID != "" && numericIDLess(watermark, message.ID) {
			filtered = append(filtered, message)
		}
	}
	return filtered
}

func minimumDiscordMessageID(page []discordMessage) string {
	minimum := ""
	for _, message := range page {
		if message.ID != "" && (minimum == "" || numericIDLess(message.ID, minimum)) {
			minimum = message.ID
		}
	}
	return minimum
}

// guildIDs resolves the single guild this connection is authorized to poll.
// provider_context is only a selection hint: the connection's OAuth identity
// must own or hold Manage Guild permission for the selected guild before the
// app-level bot credential may access it.
func (f *DiscordFetcher) guildIDs(ctx context.Context, conn store.Connection, accessToken string) ([]string, error) {
	if strings.TrimSpace(accessToken) == "" {
		return nil, errDiscordGuildNotAuthorized
	}

	var guilds []struct {
		ID          string `json:"id"`
		Owner       bool   `json:"owner"`
		Permissions string `json:"permissions"`
	}
	base := f.BaseURL
	if base == "" {
		base = discordDefaultBaseURL
	}
	if err := providerGetJSONAuth(
		ctx,
		f.HTTP,
		"Bearer "+strings.TrimSpace(accessToken),
		strings.TrimRight(base, "/")+"/users/@me/guilds",
		&guilds,
	); err != nil {
		return nil, fmt.Errorf("discord OAuth user guilds: %w", err)
	}

	managedGuilds := make(map[string]struct{}, len(guilds))
	for _, guild := range guilds {
		permissions, _ := strconv.ParseUint(guild.Permissions, 10, 64)
		if guild.Owner || permissions&discordManageGuildPermission != 0 {
			managedGuilds[guild.ID] = struct{}{}
		}
	}

	if guildID := strings.TrimSpace(conn.ProviderContext["guild_id"]); guildID != "" {
		if _, authorized := managedGuilds[guildID]; authorized {
			return []string{guildID}, nil
		}
		return nil, errDiscordGuildNotAuthorized
	}

	// The normal OAuth flow does not yet include a guild picker. It is safe to
	// derive a guild only when the OAuth user can administer exactly one guild
	// that the configured bot is also installed in; ambiguous installs fail
	// closed until the user explicitly selects a guild.
	var botGuilds []struct {
		ID string `json:"id"`
	}
	if err := providerGetJSONAuth(
		ctx,
		f.HTTP,
		"Bot "+strings.TrimSpace(f.BotToken),
		strings.TrimRight(base, "/")+"/users/@me/guilds",
		&botGuilds,
	); err != nil {
		return nil, fmt.Errorf("discord bot guilds: %w", err)
	}
	candidates := make([]string, 0, 1)
	for _, guild := range botGuilds {
		if _, authorized := managedGuilds[guild.ID]; authorized {
			candidates = append(candidates, guild.ID)
		}
	}
	if len(candidates) == 1 {
		return candidates, nil
	}
	return nil, errDiscordGuildNotConfigured
}

func (f *DiscordFetcher) getJSON(ctx context.Context, path string, out any) error {
	base := f.BaseURL
	if base == "" {
		base = discordDefaultBaseURL
	}
	return providerGetJSONAuth(ctx, f.HTTP, "Bot "+strings.TrimSpace(f.BotToken), strings.TrimRight(base, "/")+path, out)
}

type discordMessage struct {
	ID        string `json:"id"`
	Content   string `json:"content"`
	Timestamp string `json:"timestamp"`
	Author    struct {
		ID         string `json:"id"`
		Username   string `json:"username"`
		GlobalName string `json:"global_name"`
		Bot        bool   `json:"bot"`
	} `json:"author"`
}

// normalize maps one Discord message to the bridge shape. ok=false means
// skip: bot authors (including our own bot's replies) and empty content.
func (m discordMessage) normalize(channelID, channelName string) (EmailMessage, bool) {
	if m.ID == "" || m.Author.Bot {
		return EmailMessage{}, false
	}
	content := strings.TrimSpace(m.Content)
	if content == "" {
		return EmailMessage{}, false
	}
	subject := "(channel)"
	if strings.TrimSpace(channelName) != "" {
		subject = "#" + strings.TrimSpace(channelName)
	}
	name := m.Author.GlobalName
	if name == "" {
		name = m.Author.Username
	}
	return EmailMessage{
		ProviderEventID:   m.ID,
		ProviderMessageID: m.ID,
		ProviderThreadID:  channelID,
		Subject:           subject,
		From:              Participant{Name: name},
		BodyText:          content,
		OccurredAt:        discordOccurredAt(m.Timestamp, m.ID),
	}, true
}

// discordOccurredAt parses the message timestamp, deriving it from the
// snowflake when the field is absent or malformed.
func discordOccurredAt(timestamp, snowflake string) time.Time {
	if parsed, err := time.Parse(time.RFC3339, timestamp); err == nil {
		return parsed.UTC()
	}
	if id, err := strconv.ParseUint(snowflake, 10, 64); err == nil {
		return time.UnixMilli(int64(id>>22) + discordEpochMS).UTC()
	}
	return time.Now().UTC()
}

// discordSnowflakeForTime builds the synthetic snowflake bounding a backfill
// window (timestamp bits shifted into place, worker/process/sequence zeroed).
func discordSnowflakeForTime(t time.Time) string {
	ms := t.UnixMilli() - discordEpochMS
	if ms < 0 {
		ms = 0
	}
	return strconv.FormatUint(uint64(ms)<<22, 10)
}
