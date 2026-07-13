// Package emailsync pulls inbound email from connected Gmail (Google) and
// Outlook (Microsoft Graph) mailboxes into the Velion Inbox.
//
// It is the missing "caller" side of the long-standing email pathway: the
// conversation-ingest-rs bridge (/internal/ingest/email) and the outbound
// reply ops (gmail.send / mail.send via buildSendOperation) already exist —
// this package fetches new inbox messages per connection and posts them to
// the bridge, which normalizes and forwards to conversation-core.
//
// Sync strategy follows both vendors' recommended client-sync model:
//   - Gmail: users.getProfile bootstrap → users.history.list incremental
//     (startHistoryId cursor); HTTP 404 = expired history → full re-bootstrap.
//   - Graph: /me/mailFolders/inbox/messages/delta initial full sync →
//     @odata.deltaLink cursor; 410 Gone / SyncStateNotFound → re-bootstrap.
//
// Polling is the correctness baseline (push notifications from both vendors
// are documented as hints only — history/delta remains the source of truth),
// so this worker is complete without webhook push; push endpoints can later
// trigger an immediate RunOnce without changing any of this logic.
package emailsync

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"slices"
	"strings"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// ErrCursorExpired signals that the provider no longer accepts the stored
// cursor (Gmail 404 on startHistoryId, Graph 410 Gone / SyncStateNotFound).
// The worker resets the cursor and re-bootstraps.
var ErrCursorExpired = errors.New("email sync cursor expired")

// Participant mirrors the bridge's participant shape.
type Participant struct {
	Name  string `json:"name,omitempty"`
	Email string `json:"email,omitempty"`
}

// EmailMessage is one normalized inbound message ready for the ingest bridge.
type EmailMessage struct {
	ProviderEventID   string
	ProviderMessageID string
	ProviderThreadID  string
	MessageIDHeader   string
	ReferencesHeader  string
	InReplyToHeader   string
	Subject           string
	From              Participant
	To                []Participant
	BodyText          string
	BodyHTML          string
	OccurredAt        time.Time
}

// FetchResult is one provider fetch: the new messages plus the cursor to
// store for the next cycle. NextCursor is always safe to persist — for Graph
// it may be an @odata.nextLink when the per-cycle cap interrupted a page walk
// (a valid resumption point per the delta contract).
type FetchResult struct {
	Messages   []EmailMessage
	NextCursor string
}

// Fetcher is one provider's mailbox reader (Gmail or Graph).
type Fetcher interface {
	// Fetch returns inbox messages newer than cursor. An empty cursor means
	// bootstrap: establish a fresh cursor and backfill at most maxMessages
	// from the backfill window. Implementations return ErrCursorExpired
	// (possibly wrapped) when the provider rejects the stored cursor.
	Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error)
}

// ConnectionSource is the store subset the worker needs.
type ConnectionSource interface {
	ListConnections(ctx context.Context, filter store.ConnectionFilter) ([]store.Connection, error)
	GetEmailSyncState(ctx context.Context, connectionID string) (store.EmailSyncState, error)
	UpsertEmailSyncState(ctx context.Context, state store.EmailSyncState) error
}

// TokenSource resolves a fresh (refresh-aware) access token for a connection.
type TokenSource interface {
	AccessTokenForConnection(ctx context.Context, connectionID string) (oauth.AccessTokenResult, error)
}

// Ingestor delivers one normalized message to the conversation-ingest bridge.
type Ingestor interface {
	Ingest(ctx context.Context, conn store.Connection, msg EmailMessage) error
}

// rawEmailEvent is the bridge's RawEmailEvent wire shape (snake_case).
type rawEmailEvent struct {
	OrgID             string        `json:"org_id"`
	ConnectionID      string        `json:"connection_id"`
	Provider          string        `json:"provider"`
	ProviderEventID   string        `json:"provider_event_id"`
	ProviderMessageID string        `json:"provider_message_id"`
	ProviderThreadID  string        `json:"provider_thread_id"`
	MessageIDHeader   string        `json:"message_id_header,omitempty"`
	ReferencesHeader  string        `json:"references_header,omitempty"`
	InReplyToHeader   string        `json:"in_reply_to_header,omitempty"`
	Direction         string        `json:"direction"`
	Subject           string        `json:"subject"`
	From              Participant   `json:"from"`
	To                []Participant `json:"to,omitempty"`
	BodyText          string        `json:"body_text"`
	BodyHTML          string        `json:"body_html,omitempty"`
	OccurredAt        time.Time     `json:"occurred_at"`
}

// IngestClient posts normalized messages to conversation-ingest-rs.
type IngestClient struct {
	BaseURL      string
	ServiceToken string
	HTTP         *http.Client
	Now          func() time.Time
	Nonce        func() string
}

func (c *IngestClient) Ingest(ctx context.Context, conn store.Connection, msg EmailMessage) error {
	event := rawEmailEvent{
		OrgID:        conn.OrganizationID,
		ConnectionID: conn.ID,
		// Provider is the connection's provider key ("google"/"microsoft") so
		// the stored ChannelThreadRef routes replies through the matching
		// buildSendOperation case (gmail.send / mail.send).
		Provider:          conn.ProviderKey,
		ProviderEventID:   msg.ProviderEventID,
		ProviderMessageID: msg.ProviderMessageID,
		ProviderThreadID:  msg.ProviderThreadID,
		MessageIDHeader:   msg.MessageIDHeader,
		ReferencesHeader:  msg.ReferencesHeader,
		InReplyToHeader:   msg.InReplyToHeader,
		Direction:         "inbound",
		Subject:           msg.Subject,
		From:              msg.From,
		To:                msg.To,
		BodyText:          msg.BodyText,
		BodyHTML:          msg.BodyHTML,
		OccurredAt:        msg.OccurredAt,
	}
	payload, err := json.Marshal(event)
	if err != nil {
		return fmt.Errorf("marshal email event: %w", err)
	}
	url := strings.TrimRight(c.BaseURL, "/") + "/internal/ingest/email"
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(payload))
	if err != nil {
		return fmt.Errorf("build ingest request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	if !validServiceToken(c.ServiceToken) {
		return fmt.Errorf("conversation ingest service token must be a non-placeholder secret of at least 32 bytes")
	}
	now := time.Now
	if c.Now != nil {
		now = c.Now
	}
	nonce := newDelegationNonce
	if c.Nonce != nil {
		nonce = c.Nonce
	}
	delegationNonce := strings.TrimSpace(nonce())
	if delegationNonce == "" {
		return fmt.Errorf("create conversation ingest delegation nonce")
	}
	for name, value := range conversationIngestDelegationHeaders(
		c.ServiceToken,
		req.Method,
		req.URL.RequestURI(),
		payload,
		conn.OrganizationID,
		now().UTC(),
		delegationNonce,
	) {
		req.Header.Set(name, value)
	}
	client := c.HTTP
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	redirectSafeClient := *client
	redirectSafeClient.CheckRedirect = func(_ *http.Request, _ []*http.Request) error {
		return http.ErrUseLastResponse
	}
	resp, err := redirectSafeClient.Do(req)
	if err != nil {
		return fmt.Errorf("post ingest bridge: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusAccepted {
		_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 2048))
		return fmt.Errorf("ingest bridge returned unexpected status %d", resp.StatusCode)
	}
	var ack struct {
		Data struct {
			Detail struct {
				ID string `json:"id"`
			} `json:"detail"`
			Message struct {
				ID string `json:"id"`
			} `json:"message"`
			Created *bool `json:"created"`
		} `json:"data"`
	}
	decoder := json.NewDecoder(io.LimitReader(resp.Body, 1<<20))
	if err := decoder.Decode(&ack); err != nil || strings.TrimSpace(ack.Data.Detail.ID) == "" || strings.TrimSpace(ack.Data.Message.ID) == "" || ack.Data.Created == nil {
		return fmt.Errorf("ingest bridge returned an invalid persistence acknowledgement")
	}
	return nil
}

func bodyDigest(body []byte) string {
	digest := sha256.Sum256(body)
	return base64.RawURLEncoding.EncodeToString(digest[:])
}

func conversationIngestDelegationHeaders(
	serviceToken string,
	method string,
	requestURI string,
	body []byte,
	organizationID string,
	timestamp time.Time,
	nonce string,
) map[string]string {
	timestampText := timestamp.UTC().Format(time.RFC3339)
	digest := bodyDigest(body)
	canonical := strings.Join([]string{
		"v2",
		"integration-email-worker",
		"conversation-ingest",
		timestampText,
		nonce,
		method,
		requestURI,
		"",
		strings.TrimSpace(organizationID),
		"",
		digest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(strings.TrimSpace(serviceToken)))
	_, _ = mac.Write([]byte(canonical))
	return map[string]string{
		"x-service-id":             "integration-email-worker",
		"x-org-id":                 strings.TrimSpace(organizationID),
		"x-delegation-timestamp":   timestampText,
		"x-delegation-nonce":       nonce,
		"x-delegation-body-sha256": digest,
		"x-delegation-signature":   base64.RawURLEncoding.EncodeToString(mac.Sum(nil)),
	}
}

func newDelegationNonce() string {
	bytes := make([]byte, 18)
	if _, err := rand.Read(bytes); err != nil {
		return ""
	}
	return base64.RawURLEncoding.EncodeToString(bytes)
}

func validServiceToken(token string) bool {
	token = strings.TrimSpace(token)
	lower := strings.ToLower(token)
	return len(token) >= 32 &&
		!strings.HasPrefix(lower, "change-me") &&
		!strings.HasPrefix(lower, "replace-with")
}

// Worker drives one poll loop over all email-capable connections.
type Worker struct {
	Store  ConnectionSource
	Tokens TokenSource
	Ingest Ingestor
	Gmail  Fetcher
	Graph  Fetcher
	Logger *zerolog.Logger

	PollInterval   time.Duration
	BackfillWindow time.Duration
	MaxPerCycle    int
}

// providerPlans maps provider keys to the capability that authorizes reading
// mail plus the raw OAuth scope fallback (older connections may predate the
// capability catalog rows).
var providerPlans = []struct {
	providerKey string
	capability  string
	scope       string
}{
	{providerKey: "google", capability: "gmail.read", scope: "https://www.googleapis.com/auth/gmail.readonly"},
	{providerKey: "microsoft", capability: "mail.read", scope: "Mail.Read"},
}

func (w Worker) Run(ctx context.Context) error {
	interval := w.PollInterval
	if interval <= 0 {
		interval = 60 * time.Second
	}
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		if _, err := w.RunOnce(ctx); err != nil {
			w.logWarn(err, "email sync iteration failed")
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-ticker.C:
		}
	}
}

// RunOnce syncs every eligible connection once and returns the number of
// messages ingested. Per-connection failures are isolated: they are recorded
// on that connection's sync state and do not abort the cycle.
func (w Worker) RunOnce(ctx context.Context) (int, error) {
	total := 0
	var firstErr error
	for _, plan := range providerPlans {
		connections, err := w.Store.ListConnections(ctx, store.ConnectionFilter{ProviderKey: plan.providerKey})
		if err != nil {
			if firstErr == nil {
				firstErr = fmt.Errorf("list %s connections: %w", plan.providerKey, err)
			}
			continue
		}
		for _, conn := range connections {
			if !connectionEligible(conn, plan.capability, plan.scope) {
				continue
			}
			ingested, err := w.syncConnection(ctx, conn)
			total += ingested
			if err != nil {
				w.logWarn(err, "email sync failed for connection "+conn.ID)
				if firstErr == nil {
					firstErr = err
				}
			}
			if ctx.Err() != nil {
				return total, ctx.Err()
			}
		}
	}
	return total, firstErr
}

func connectionEligible(conn store.Connection, capability, scope string) bool {
	if conn.DeletedAt != nil {
		return false
	}
	switch conn.Status {
	case "active", "needs_refresh":
	default:
		return false
	}
	if slices.Contains(conn.Capabilities, capability) {
		return true
	}
	return slices.ContainsFunc(conn.Scopes, func(s string) bool {
		return strings.EqualFold(s, scope)
	})
}

func (w Worker) syncConnection(ctx context.Context, conn store.Connection) (int, error) {
	fetcher := w.fetcherFor(conn.ProviderKey)
	if fetcher == nil {
		return 0, fmt.Errorf("no email fetcher for provider %s", conn.ProviderKey)
	}

	state, err := w.Store.GetEmailSyncState(ctx, conn.ID)
	if err != nil && !errors.Is(err, store.ErrNotFound) {
		return 0, fmt.Errorf("load sync state for %s: %w", conn.ID, err)
	}
	state.ConnectionID = conn.ID
	state.ProviderKey = conn.ProviderKey

	token, err := w.Tokens.AccessTokenForConnection(ctx, conn.ID)
	if err != nil {
		return 0, w.recordFailure(ctx, state, fmt.Errorf("resolve access token: %w", err))
	}

	maxMessages := w.MaxPerCycle
	if maxMessages <= 0 {
		maxMessages = 25
	}
	backfill := w.BackfillWindow
	if backfill <= 0 {
		backfill = 24 * time.Hour
	}

	result, err := fetcher.Fetch(ctx, token.AccessToken, state.Cursor, backfill, maxMessages)
	if errors.Is(err, ErrCursorExpired) {
		// The provider invalidated our cursor (Gmail >~1 week of history,
		// Graph sync-state reset). Re-bootstrap immediately, once.
		w.logInfo("cursor expired for connection " + conn.ID + ", re-bootstrapping")
		result, err = fetcher.Fetch(ctx, token.AccessToken, "", backfill, maxMessages)
	}
	if err != nil {
		return 0, w.recordFailure(ctx, state, fmt.Errorf("fetch %s mailbox: %w", conn.ProviderKey, err))
	}

	ingested := 0
	for _, msg := range result.Messages {
		// Self-echo guard: replies our own send ops produce also land in the
		// mailbox (Gmail threads them into INBOX conversations); skip mail
		// authored by the connected account itself.
		if msg.From.Email != "" && strings.EqualFold(msg.From.Email, conn.UserEmail) {
			continue
		}
		if err := w.Ingest.Ingest(ctx, conn, msg); err != nil {
			// Do not advance the cursor past a failed ingest: the whole batch
			// re-runs next cycle and conversation-core's idempotency key makes
			// the already-ingested prefix a no-op.
			return ingested, w.recordFailure(ctx, state, fmt.Errorf("ingest message %s: %w", msg.ProviderEventID, err))
		}
		ingested++
	}

	state.Cursor = result.NextCursor
	state.LastSyncedAt = time.Now().UTC()
	state.LastError = ""
	state.FailureCount = 0
	if err := w.Store.UpsertEmailSyncState(ctx, state); err != nil {
		return ingested, fmt.Errorf("persist sync state for %s: %w", conn.ID, err)
	}
	if ingested > 0 {
		w.logInfo(fmt.Sprintf("ingested %d %s message(s) for connection %s", ingested, conn.ProviderKey, conn.ID))
	}
	return ingested, nil
}

func (w Worker) fetcherFor(providerKey string) Fetcher {
	switch providerKey {
	case "google":
		return w.Gmail
	case "microsoft":
		return w.Graph
	default:
		return nil
	}
}

// recordFailure persists the failure on the sync state (cursor untouched so
// the next cycle retries from the same point) and returns the original error.
func (w Worker) recordFailure(ctx context.Context, state store.EmailSyncState, cause error) error {
	state.LastError = cause.Error()
	state.FailureCount++
	if err := w.Store.UpsertEmailSyncState(ctx, state); err != nil {
		w.logWarn(err, "persist failure state for connection "+state.ConnectionID)
	}
	return cause
}

func (w Worker) logWarn(err error, msg string) {
	if w.Logger != nil {
		w.Logger.Warn().Err(err).Msg(msg)
	}
}

func (w Worker) logInfo(msg string) {
	if w.Logger != nil {
		w.Logger.Info().Msg(msg)
	}
}
