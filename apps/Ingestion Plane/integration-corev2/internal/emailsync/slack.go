package emailsync

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

// SlackFetcher polls the conversations the workspace bot is a member of
// (public/private channels, DMs, group DMs) via the Slack Web API using the
// connection's OAuth access token. The durable cursor carries an independent
// timestamp per Slack conversation, preventing one unavailable or high-volume
// channel from blocking or skipping another. Conversation and history pages
// are exhausted before watermarks advance; partial replays are absorbed by
// conversation-core's idempotency key.
type SlackFetcher struct {
	BaseURL string // default https://slack.com/api
	HTTP    *http.Client
}

const slackDefaultBaseURL = "https://slack.com/api"

// slackPageSize is the limit applied to conversation listing and history calls.
const slackPageSize = 100

// slackMaxPages bounds one provider cycle. Hitting it fails the cycle before
// the worker persists any new cursor, preferring a safe replay over data loss.
const slackMaxPages = 100

// errSlackRateLimited stops the cycle gracefully (partial result, no failure).
var errSlackRateLimited = errors.New("slack api rate limited")

func (f *SlackFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	cursorState, err := decodeSlackCursor(cursor, backfill)
	if err != nil {
		return FetchResult{}, fmt.Errorf("slack cursor: %w", err)
	}

	conversations := make([]slackConversation, 0, slackPageSize)
	listCursor := ""
	for pageNumber := 0; pageNumber < slackMaxPages; pageNumber++ {
		listParams := url.Values{}
		listParams.Set("types", "public_channel,private_channel,im,mpim")
		listParams.Set("limit", strconv.Itoa(slackPageSize))
		if listCursor != "" {
			listParams.Set("cursor", listCursor)
		}
		var page slackConversationsPage
		if err := f.callAPI(ctx, accessToken, "users.conversations", listParams, &page); err != nil {
			if errors.Is(err, errSlackRateLimited) {
				return FetchResult{NextCursor: cursor}, nil
			}
			return FetchResult{}, fmt.Errorf("slack conversations list: %w", err)
		}
		conversations = append(conversations, page.Channels...)
		next := strings.TrimSpace(page.ResponseMetadata.NextCursor)
		if next == "" {
			break
		}
		if next == listCursor || pageNumber == slackMaxPages-1 {
			return FetchResult{}, errors.New("slack conversations pagination did not terminate")
		}
		listCursor = next
	}

	names := map[string]string{} // in-cycle users.info cache
	var messages []EmailMessage
channelLoop:
	for _, channel := range conversations {
		if len(messages) >= maxMessages {
			break
		}
		subject := "(dm)"
		if !channel.IsIM && strings.TrimSpace(channel.Name) != "" {
			subject = "#" + strings.TrimSpace(channel.Name)
		}

		oldest := cursorState.Channels[channel.ID]
		if oldest == "" {
			oldest = cursorState.Default
		}
		// Persist the inherited/default watermark for every seen channel, even
		// when it is empty or temporarily unavailable. This keeps backfill
		// stable and lets healthy channels progress independently.
		cursorState.Channels[channel.ID] = oldest
		// Slack returns the newest messages first. Walk backward with a time
		// boundary while retaining only the current page, then emit the oldest
		// terminal page first. ScanBefore is durable, so a backlog larger than
		// the per-cycle page budget progresses without skipping or OOMing.
		latest := cursorState.ScanBefore[channel.ID]
		var oldestPage []slackMessage
		reachedOldest := false
		for pageNumber := 0; pageNumber < slackMaxPages; pageNumber++ {
			historyParams := url.Values{}
			historyParams.Set("channel", channel.ID)
			historyParams.Set("oldest", oldest)
			historyParams.Set("limit", strconv.Itoa(slackPageSize))
			historyParams.Set("inclusive", "false")
			if latest != "" {
				historyParams.Set("latest", latest)
			}
			var page slackHistoryPage
			if err := f.callAPI(ctx, accessToken, "conversations.history", historyParams, &page); err != nil {
				if errors.Is(err, errSlackRateLimited) {
					return FetchResult{Messages: messages, NextCursor: encodeSlackCursor(cursorState)}, nil
				}
				if isSlackUnavailableChannel(err) {
					continue channelLoop
				}
				return FetchResult{}, fmt.Errorf("slack history %s: %w", channel.ID, err)
			}
			oldestPage = page.Messages
			if len(page.Messages) == 0 {
				reachedOldest = true
				break
			}
			if !page.HasMore && strings.TrimSpace(page.ResponseMetadata.NextCursor) == "" {
				reachedOldest = true
				break
			}
			nextLatest := strings.TrimSpace(page.Messages[len(page.Messages)-1].TS)
			if nextLatest == "" || nextLatest == latest {
				return FetchResult{}, fmt.Errorf("slack history %s pagination did not progress", channel.ID)
			}
			latest = nextLatest
		}
		if !reachedOldest {
			cursorState.ScanBefore[channel.ID] = latest
			continue
		}
		delete(cursorState.ScanBefore, channel.ID)

		// The terminal page is newest-first; reverse it so the per-cycle cap
		// advances from the oldest unseen event. Intentionally filtered events
		// still advance the watermark so bot noise cannot stall a channel.
		for i := len(oldestPage) - 1; i >= 0; i-- {
			if len(messages) >= maxMessages {
				break
			}
			raw := oldestPage[i]
			msg, ok := raw.normalize(channel.ID, subject)
			if cursorState.Channels[channel.ID] == "" || slackTSLess(cursorState.Channels[channel.ID], raw.TS) {
				cursorState.Channels[channel.ID] = raw.TS
			}
			if !ok {
				continue
			}
			msg.From.Name = f.resolveUserName(ctx, accessToken, raw.User, names)
			messages = append(messages, msg)
		}
	}
	return FetchResult{Messages: messages, NextCursor: encodeSlackCursor(cursorState)}, nil
}

type slackConversation struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	IsIM   bool   `json:"is_im"`
	IsMPIM bool   `json:"is_mpim"`
}

type slackResponseMetadata struct {
	NextCursor string `json:"next_cursor"`
}

type slackConversationsPage struct {
	slackEnvelope
	Channels         []slackConversation   `json:"channels"`
	ResponseMetadata slackResponseMetadata `json:"response_metadata"`
}

type slackHistoryPage struct {
	slackEnvelope
	Messages         []slackMessage        `json:"messages"`
	HasMore          bool                  `json:"has_more"`
	ResponseMetadata slackResponseMetadata `json:"response_metadata"`
}

type slackCursorState struct {
	Version    int               `json:"version"`
	Default    string            `json:"default"`
	Channels   map[string]string `json:"channels"`
	ScanBefore map[string]string `json:"scanBefore,omitempty"`
}

// decodeSlackCursor accepts the legacy single Slack timestamp and migrates it
// into a per-channel cursor. The default watermark also covers channels first
// discovered after the migration.
func decodeSlackCursor(raw string, backfill time.Duration) (slackCursorState, error) {
	trimmed := strings.TrimSpace(raw)
	state := slackCursorState{Version: 1, Channels: map[string]string{}, ScanBefore: map[string]string{}}
	if trimmed == "" {
		state.Default = fmt.Sprintf("%d.000000", time.Now().UTC().Add(-backfill).Unix())
		return state, nil
	}
	if !strings.HasPrefix(trimmed, "{") {
		state.Default = trimmed
		return state, nil
	}
	if err := json.Unmarshal([]byte(trimmed), &state); err != nil {
		return slackCursorState{}, err
	}
	if state.Version != 1 {
		return slackCursorState{}, fmt.Errorf("unsupported version %d", state.Version)
	}
	if state.Channels == nil {
		state.Channels = map[string]string{}
	}
	if state.ScanBefore == nil {
		state.ScanBefore = map[string]string{}
	}
	if state.Default == "" {
		state.Default = fmt.Sprintf("%d.000000", time.Now().UTC().Add(-backfill).Unix())
	}
	return state, nil
}

func encodeSlackCursor(state slackCursorState) string {
	encoded, err := json.Marshal(state)
	if err != nil {
		// The state contains only strings and a string map, so serialization
		// cannot fail in practice. Preserve a safe watermark if it ever does.
		return state.Default
	}
	return string(encoded)
}

// resolveUserName resolves a Slack user id to a display name best-effort with
// an in-cycle cache; lookup failures fall back to the raw user id.
func (f *SlackFetcher) resolveUserName(ctx context.Context, accessToken, userID string, cache map[string]string) string {
	if userID == "" {
		return ""
	}
	if name, ok := cache[userID]; ok {
		return name
	}
	var info struct {
		slackEnvelope
		User struct {
			Name     string `json:"name"`
			RealName string `json:"real_name"`
			Profile  struct {
				DisplayName string `json:"display_name"`
			} `json:"profile"`
		} `json:"user"`
	}
	params := url.Values{}
	params.Set("user", userID)
	name := userID
	if err := f.callAPI(ctx, accessToken, "users.info", params, &info); err == nil {
		for _, candidate := range []string{info.User.Profile.DisplayName, info.User.RealName, info.User.Name} {
			if strings.TrimSpace(candidate) != "" {
				name = strings.TrimSpace(candidate)
				break
			}
		}
	}
	cache[userID] = name
	return name
}

// slackEnvelope is the {ok, error} wrapper every Web API reply carries.
type slackEnvelope struct {
	OK       bool   `json:"ok"`
	APIError string `json:"error"`
}

// slackResponse lets callAPI check the envelope on any embedded response.
type slackResponse interface {
	envelope() slackEnvelope
}

type slackAPIError struct {
	Method string
	Code   string
}

func (e *slackAPIError) Error() string {
	return fmt.Sprintf("slack %s returned error %q", e.Method, e.Code)
}

func isSlackUnavailableChannel(err error) bool {
	var apiErr *slackAPIError
	if !errors.As(err, &apiErr) {
		return false
	}
	switch apiErr.Code {
	case "channel_not_found", "not_in_channel", "is_archived":
		return true
	default:
		return false
	}
}

func (e slackEnvelope) envelope() slackEnvelope { return e }

func (f *SlackFetcher) callAPI(ctx context.Context, accessToken, method string, params url.Values, out slackResponse) error {
	base := f.BaseURL
	if base == "" {
		base = slackDefaultBaseURL
	}
	fullURL := strings.TrimRight(base, "/") + "/" + method + "?" + params.Encode()
	if err := providerGetJSON(ctx, f.HTTP, accessToken, fullURL, out); err != nil {
		var httpErr *providerHTTPError
		if asProviderHTTPError(err, &httpErr) && httpErr.Status == http.StatusTooManyRequests {
			return errSlackRateLimited
		}
		return err
	}
	env := out.envelope()
	if !env.OK {
		if env.APIError == "ratelimited" {
			return errSlackRateLimited
		}
		return &slackAPIError{Method: method, Code: env.APIError}
	}
	return nil
}

type slackMessage struct {
	Type        string `json:"type"`
	Subtype     string `json:"subtype"`
	User        string `json:"user"`
	BotID       string `json:"bot_id"`
	Text        string `json:"text"`
	TS          string `json:"ts"`
	ClientMsgID string `json:"client_msg_id"`
}

// normalize maps one Slack message to the bridge shape. ok=false means skip:
// bot messages, non-message subtypes (thread_broadcast excepted), empty text.
func (m slackMessage) normalize(channelID, subject string) (EmailMessage, bool) {
	if m.Type != "message" || m.TS == "" || m.BotID != "" {
		return EmailMessage{}, false
	}
	if m.Subtype != "" && m.Subtype != "thread_broadcast" {
		return EmailMessage{}, false
	}
	text := strings.TrimSpace(m.Text)
	if text == "" {
		return EmailMessage{}, false
	}
	messageID := m.ClientMsgID
	if messageID == "" {
		messageID = m.TS
	}
	return EmailMessage{
		ProviderEventID:   channelID + ":" + m.TS,
		ProviderMessageID: messageID,
		ProviderThreadID:  channelID,
		Subject:           subject,
		BodyText:          text,
		OccurredAt:        slackTSTime(m.TS),
	}, true
}

// slackTSLess compares two Slack ts strings as floats (seconds.microseconds).
func slackTSLess(a, b string) bool {
	af, aerr := strconv.ParseFloat(a, 64)
	bf, berr := strconv.ParseFloat(b, 64)
	if aerr != nil || berr != nil {
		return a < b
	}
	return af < bf
}

func slackTSTime(ts string) time.Time {
	secText, fracText, _ := strings.Cut(ts, ".")
	sec, err := strconv.ParseInt(secText, 10, 64)
	if err != nil || sec <= 0 {
		return time.Now().UTC()
	}
	// The ts fraction is microseconds; pad/truncate to nanoseconds so float
	// rounding never shifts the timestamp.
	nsec := int64(0)
	if fracText != "" {
		if parsed, err := strconv.ParseInt((fracText + "000000000")[:9], 10, 64); err == nil {
			nsec = parsed
		}
	}
	return time.Unix(sec, nsec).UTC()
}
