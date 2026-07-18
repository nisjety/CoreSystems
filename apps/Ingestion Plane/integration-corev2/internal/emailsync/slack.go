package emailsync

import (
	"context"
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
// connection's OAuth access token. The cursor is a single watermark — the max
// message ts emitted across all conversations (Slack ts strings compare as
// floats) — so it never expires; duplicates from watermark re-reads are
// absorbed by conversation-core's idempotency key. A `ratelimited` reply (or
// HTTP 429) ends the cycle gracefully with whatever was collected: the
// watermark only covers emitted messages, so nothing is lost.
type SlackFetcher struct {
	BaseURL string // default https://slack.com/api
	HTTP    *http.Client
}

const slackDefaultBaseURL = "https://slack.com/api"

// slackPageSize is the limit applied to conversation listing and history calls.
const slackPageSize = 100

// errSlackRateLimited stops the cycle gracefully (partial result, no failure).
var errSlackRateLimited = errors.New("slack api rate limited")

func (f *SlackFetcher) Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	oldest := cursor
	if oldest == "" {
		oldest = fmt.Sprintf("%d.000000", time.Now().UTC().Add(-backfill).Unix())
	}
	nextCursor := cursor

	var conversations struct {
		slackEnvelope
		Channels []struct {
			ID     string `json:"id"`
			Name   string `json:"name"`
			IsIM   bool   `json:"is_im"`
			IsMPIM bool   `json:"is_mpim"`
		} `json:"channels"`
	}
	listParams := url.Values{}
	listParams.Set("types", "public_channel,private_channel,im,mpim")
	listParams.Set("limit", strconv.Itoa(slackPageSize))
	if err := f.callAPI(ctx, accessToken, "users.conversations", listParams, &conversations); err != nil {
		if errors.Is(err, errSlackRateLimited) {
			return FetchResult{NextCursor: nextCursor}, nil
		}
		return FetchResult{}, fmt.Errorf("slack conversations list: %w", err)
	}

	names := map[string]string{} // in-cycle users.info cache
	var messages []EmailMessage
	for _, channel := range conversations.Channels {
		if len(messages) >= maxMessages {
			break
		}
		subject := "(dm)"
		if !channel.IsIM && strings.TrimSpace(channel.Name) != "" {
			subject = "#" + strings.TrimSpace(channel.Name)
		}

		var history struct {
			slackEnvelope
			Messages []slackMessage `json:"messages"`
		}
		historyParams := url.Values{}
		historyParams.Set("channel", channel.ID)
		historyParams.Set("oldest", oldest)
		historyParams.Set("limit", strconv.Itoa(slackPageSize))
		historyParams.Set("inclusive", "false")
		if err := f.callAPI(ctx, accessToken, "conversations.history", historyParams, &history); err != nil {
			if errors.Is(err, errSlackRateLimited) {
				return FetchResult{Messages: messages, NextCursor: nextCursor}, nil
			}
			return FetchResult{}, fmt.Errorf("slack history %s: %w", channel.ID, err)
		}

		// History is newest-first; walk in reverse for natural inbox order.
		for i := len(history.Messages) - 1; i >= 0; i-- {
			if len(messages) >= maxMessages {
				break
			}
			raw := history.Messages[i]
			msg, ok := raw.normalize(channel.ID, subject)
			if !ok {
				continue
			}
			msg.From.Name = f.resolveUserName(ctx, accessToken, raw.User, names)
			messages = append(messages, msg)
			if nextCursor == "" || slackTSLess(nextCursor, raw.TS) {
				nextCursor = raw.TS
			}
		}
	}
	return FetchResult{Messages: messages, NextCursor: nextCursor}, nil
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
		return fmt.Errorf("slack %s returned error %q", method, env.APIError)
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
