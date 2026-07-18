package emailsync

import (
	"context"
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
// comes from the connection's provider_context["guild_id"]; when absent the
// fetcher falls back to every guild the bot is installed in (GET
// /users/@me/guilds) — an accepted v1 simplification: all bot guilds are
// polled and the events are attributed to the connection being iterated. The
// cursor is a single watermark — the max message snowflake emitted (uint64
// compare) — passed as `after` on each channel's message call, so it never
// expires.
type DiscordFetcher struct {
	BaseURL  string // default https://discord.com/api/v10
	BotToken string
	HTTP     *http.Client
}

const discordDefaultBaseURL = "https://discord.com/api/v10"

// discordPageSize is the limit applied to channel message calls.
const discordPageSize = 100

// discordEpochMS is the Discord snowflake epoch (2015-01-01T00:00:00Z).
const discordEpochMS = 1420070400000

// discordGuildTextChannel is Discord channel type 0 (GUILD_TEXT).
const discordGuildTextChannel = 0

// errDiscordBotNotConfigured marks a missing app-level bot token; the worker
// records it once per cycle on the connection's sync state without spamming.
var errDiscordBotNotConfigured = errors.New("discord bot token not configured (set DISCORD_BOT_TOKEN)")

// Fetch satisfies Fetcher for callers without a connection at hand; guild
// resolution then relies entirely on the bot-guild fallback.
func (f *DiscordFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	return f.FetchConnection(ctx, store.Connection{}, accessToken, cursor, backfill, maxMessages)
}

// FetchConnection implements ConnectionFetcher: the connection carries the
// guild scoping. The OAuth access token is ignored — all reads use the bot
// token.
func (f *DiscordFetcher) FetchConnection(ctx context.Context, conn store.Connection, _ string, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	if strings.TrimSpace(f.BotToken) == "" {
		return FetchResult{}, errDiscordBotNotConfigured
	}
	guildIDs, err := f.guildIDs(ctx, conn)
	if err != nil {
		return FetchResult{}, err
	}

	after := cursor
	if after == "" {
		after = discordSnowflakeForTime(time.Now().UTC().Add(-backfill))
	}
	nextCursor := cursor

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
			query := url.Values{}
			query.Set("after", after)
			query.Set("limit", strconv.Itoa(discordPageSize))
			var page []discordMessage
			if err := f.getJSON(ctx, "/channels/"+url.PathEscape(channel.ID)+"/messages?"+query.Encode(), &page); err != nil {
				return FetchResult{}, fmt.Errorf("discord channel %s messages: %w", channel.ID, err)
			}
			// Emit oldest-first regardless of the API's ordering so hitting
			// maxMessages never advances the watermark past unemitted messages.
			slices.SortFunc(page, func(a, b discordMessage) int {
				if a.ID == b.ID {
					return 0
				}
				if numericIDLess(a.ID, b.ID) {
					return -1
				}
				return 1
			})
			for _, raw := range page {
				if len(messages) >= maxMessages {
					break
				}
				msg, ok := raw.normalize(channel.ID, channel.Name)
				if !ok {
					continue
				}
				messages = append(messages, msg)
				if nextCursor == "" || numericIDLess(nextCursor, raw.ID) {
					nextCursor = raw.ID
				}
			}
		}
	}
	return FetchResult{Messages: messages, NextCursor: nextCursor}, nil
}

// guildIDs resolves which guilds to poll: the connection's stored guild_id
// when present, otherwise every guild the bot is installed in.
func (f *DiscordFetcher) guildIDs(ctx context.Context, conn store.Connection) ([]string, error) {
	if guildID := strings.TrimSpace(conn.ProviderContext["guild_id"]); guildID != "" {
		return []string{guildID}, nil
	}
	var guilds []struct {
		ID string `json:"id"`
	}
	if err := f.getJSON(ctx, "/users/@me/guilds", &guilds); err != nil {
		return nil, fmt.Errorf("discord bot guilds list: %w", err)
	}
	ids := make([]string, 0, len(guilds))
	for _, guild := range guilds {
		if guild.ID != "" {
			ids = append(ids, guild.ID)
		}
	}
	return ids, nil
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
