// Package emailsync pulls inbound email from connected Gmail (Google) and
// Outlook (Microsoft Graph) mailboxes into the Verevon Inbox.
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
//
// The same loop also polls chat sources into the unified inbox: Microsoft
// Teams chats/channels (teams.go), Slack conversations (slack.go), X direct
// messages (xdm.go), and Discord guild channels (discord.go). Each chat plan
// keeps its own cursor row (conn.ID + ":" + suffix) and stamps its channel
// provider on delivered events so conversation-core routes them into the
// matching inbox channel.
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

	"github.com/triodelab/integration-corev2/internal/handoff"
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
	ProviderEventID       string
	ProviderMessageID     string
	ProviderThreadID      string
	MessageIDHeader       string
	ReferencesHeader      string
	InReplyToHeader       string
	AutoSubmitted         string
	ContentType           string
	OutboundCorrelationID string
	Direction             string
	Subject               string
	From                  Participant
	To                    []Participant
	BodyText              string
	BodyHTML              string
	OccurredAt            time.Time
}

// FetchResult is one provider fetch: the new messages plus the cursor to
// store for the next cycle. NextCursor is always safe to persist — for Graph
// it may be an @odata.nextLink when the per-cycle cap interrupted a page walk
// (a valid resumption point per the delta contract).
type FetchResult struct {
	Messages             []EmailMessage
	NextCursor           string
	ProviderContextPatch map[string]string
}

// Fetcher is one provider's inbound message reader (mailbox or chat source).
type Fetcher interface {
	// Fetch returns inbox messages newer than cursor. An empty cursor means
	// bootstrap: establish a fresh cursor and backfill at most maxMessages
	// from the backfill window. Implementations return ErrCursorExpired
	// (possibly wrapped) when the provider rejects the stored cursor.
	Fetch(ctx context.Context, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error)
}

// CursorVersioner lets a fetcher invalidate cursor formats or bootstrap
// policies without a schema migration. A non-empty version is persisted as a
// prefix; legacy/unversioned cursors are reset once and then upgraded.
type CursorVersioner interface {
	CursorVersion() string
}

const cursorVersionSeparator = "|"

func decodeFetcherCursor(fetcher Fetcher, stored string) string {
	versioned, ok := fetcher.(CursorVersioner)
	if !ok {
		return stored
	}
	if strings.TrimSpace(versioned.CursorVersion()) == "" {
		// A stored version prefix belongs to a different bootstrap policy and is
		// never a provider URL. Reset it when the versioned mode is disabled.
		if strings.Contains(stored, cursorVersionSeparator) {
			return ""
		}
		return stored
	}
	prefix := strings.TrimSpace(versioned.CursorVersion()) + cursorVersionSeparator
	if !strings.HasPrefix(stored, prefix) {
		return ""
	}
	return strings.TrimPrefix(stored, prefix)
}

func encodeFetcherCursor(fetcher Fetcher, cursor string) string {
	versioned, ok := fetcher.(CursorVersioner)
	if !ok || strings.TrimSpace(versioned.CursorVersion()) == "" {
		return cursor
	}
	return strings.TrimSpace(versioned.CursorVersion()) + cursorVersionSeparator + cursor
}

// ConnectionFetcher is an optional Fetcher upgrade for sources that need
// connection identity beyond the access token (Discord resolves the guild to
// poll from the connection's provider context). Fetchers that do not need it
// keep the plain Fetch signature.
type ConnectionFetcher interface {
	FetchConnection(ctx context.Context, conn store.Connection, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error)
}

// fetchWith dispatches to FetchConnection when the fetcher wants the
// connection, and to plain Fetch otherwise.
func fetchWith(ctx context.Context, fetcher Fetcher, conn store.Connection, accessToken, cursor string, backfill time.Duration, maxMessages int) (FetchResult, error) {
	if cf, ok := fetcher.(ConnectionFetcher); ok {
		return cf.FetchConnection(ctx, conn, accessToken, cursor, backfill, maxMessages)
	}
	return fetcher.Fetch(ctx, accessToken, cursor, backfill, maxMessages)
}

// ConnectionSource is the store subset the worker needs.
type ConnectionSource interface {
	ListConnections(ctx context.Context, filter store.ConnectionFilter) ([]store.Connection, error)
	GetEmailSyncState(ctx context.Context, connectionID string) (store.EmailSyncState, error)
	UpsertEmailSyncState(ctx context.Context, state store.EmailSyncState) error
}

// ConnectionSyncStatusUpdater is optional so lightweight test stores and
// alternate runtimes can keep using the worker without owning the connection
// table. The Postgres repository implements it to project provider delivery
// health into the connection status API.
type ConnectionSyncStatusUpdater interface {
	UpdateConnectionSyncStatus(ctx context.Context, connectionID, status string) error
}

// ConnectionContextUpdater is the optional store capability used when a
// provider safely discovers a durable tenant binding during fetch. The
// binding must be committed before any fetched message is delivered.
type ConnectionContextUpdater interface {
	BindConnectionProviderContext(ctx context.Context, id, key, value string) (store.Connection, error)
}

// InboxSyncJobClient is the narrow internal handoff the worker uses for an
// explicit Support refresh. The API owns queue transitions and audit events;
// the worker only claims a bounded job and reports its content-free outcome.
type InboxSyncJobClient interface {
	ClaimSyncJob(context.Context, handoff.SyncClaimRequest) (store.SyncJob, error)
	UpdateSyncProgress(context.Context, string, handoff.SyncProgressRequest) (store.SyncJob, error)
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
	OrgID                 string        `json:"org_id"`
	ConnectionID          string        `json:"connection_id"`
	Provider              string        `json:"provider"`
	ProviderEventID       string        `json:"provider_event_id"`
	ProviderMessageID     string        `json:"provider_message_id"`
	ProviderThreadID      string        `json:"provider_thread_id"`
	MessageIDHeader       string        `json:"message_id_header,omitempty"`
	ReferencesHeader      string        `json:"references_header,omitempty"`
	InReplyToHeader       string        `json:"in_reply_to_header,omitempty"`
	AutoSubmitted         string        `json:"auto_submitted,omitempty"`
	ContentType           string        `json:"content_type,omitempty"`
	OutboundCorrelationID string        `json:"outbound_correlation_id,omitempty"`
	Direction             string        `json:"direction"`
	Subject               string        `json:"subject"`
	From                  Participant   `json:"from"`
	To                    []Participant `json:"to,omitempty"`
	BodyText              string        `json:"body_text"`
	BodyHTML              string        `json:"body_html,omitempty"`
	OccurredAt            time.Time     `json:"occurred_at"`
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
	direction := strings.TrimSpace(msg.Direction)
	if direction == "" {
		direction = "inbound"
	}
	event := rawEmailEvent{
		OrgID:        conn.OrganizationID,
		ConnectionID: conn.ID,
		// Provider is the connection's provider key ("google"/"microsoft") so
		// the stored ChannelThreadRef routes replies through the matching
		// buildSendOperation case (gmail.send / mail.send).
		Provider:              conn.ProviderKey,
		ProviderEventID:       msg.ProviderEventID,
		ProviderMessageID:     msg.ProviderMessageID,
		ProviderThreadID:      msg.ProviderThreadID,
		MessageIDHeader:       msg.MessageIDHeader,
		ReferencesHeader:      msg.ReferencesHeader,
		InReplyToHeader:       msg.InReplyToHeader,
		AutoSubmitted:         msg.AutoSubmitted,
		ContentType:           msg.ContentType,
		OutboundCorrelationID: msg.OutboundCorrelationID,
		Direction:             direction,
		Subject:               msg.Subject,
		From:                  msg.From,
		To:                    msg.To,
		BodyText:              msg.BodyText,
		BodyHTML:              msg.BodyHTML,
		OccurredAt:            msg.OccurredAt,
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

// Worker drives one poll loop over all inbox-capable connections (email
// mailboxes plus the chat sources: Teams, Slack, X DMs, Discord).
type Worker struct {
	Store       ConnectionSource
	Tokens      TokenSource
	Ingest      Ingestor
	Integration InboxSyncJobClient
	Gmail       Fetcher
	Graph       Fetcher
	Teams       Fetcher
	Slack       Fetcher
	XDM         Fetcher
	Discord     Fetcher
	Logger      *zerolog.Logger

	PollInterval       time.Duration
	ManualPollInterval time.Duration
	BackfillWindow     time.Duration
	MaxPerCycle        int
}

// providerPlan is one poll source: a provider key plus the capability that
// authorizes reading it and the raw OAuth scope fallback (older connections
// may predate the capability catalog rows).
type providerPlan struct {
	providerKey string
	capability  string
	scope       string
	// stateKeySuffix isolates this plan's cursor row: empty keeps the bare
	// connection id (backward compatible with pre-existing gmail/outlook
	// rows); non-empty stores under conn.ID + ":" + suffix so several plans
	// can share one connection (email_sync_state has no FK, so synthetic
	// keys are safe).
	stateKeySuffix string
	// fetcherKey selects a dedicated fetcher; empty keeps the historical
	// per-provider dispatch (google→Gmail, microsoft→Graph).
	fetcherKey string
	// channelProvider overrides the outbound `provider` field stamped on
	// delivered events so conversation-core routes the thread into the
	// matching inbox channel; empty keeps the connection's own provider key
	// ("google"/"microsoft") so replies route through gmail.send/mail.send.
	channelProvider string
}

// source names the plan for fetcher dispatch and error messages: the
// dedicated fetcher key when set, otherwise the provider key.
func (p providerPlan) source() string {
	if p.fetcherKey != "" {
		return p.fetcherKey
	}
	return p.providerKey
}

func (p providerPlan) inboxChannel() string {
	if p.channelProvider != "" {
		return p.channelProvider
	}
	return "email"
}

// stateKey returns the email_sync_state primary key for this plan.
func (p providerPlan) stateKey(connectionID string) string {
	if p.stateKeySuffix == "" {
		return connectionID
	}
	return connectionID + ":" + p.stateKeySuffix
}

var providerPlans = []providerPlan{
	{providerKey: "google", capability: "gmail.read", scope: "https://www.googleapis.com/auth/gmail.readonly"},
	{providerKey: "microsoft", capability: "mail.read", scope: "Mail.Read"},
	{providerKey: "microsoft", capability: "teams.messages.read", scope: "ChannelMessage.Read.All", stateKeySuffix: "teams", fetcherKey: "teams", channelProvider: "teams"},
	{providerKey: "slack", capability: "channels.history", scope: "channels:history", stateKeySuffix: "slack", fetcherKey: "slack", channelProvider: "slack"},
	{providerKey: "x", capability: "social.inbox.read", scope: "dm.read", stateKeySuffix: "xdm", fetcherKey: "xdm", channelProvider: "x"},
	{providerKey: "discord", capability: "messages.read", scope: "bot", stateKeySuffix: "discord", fetcherKey: "discord", channelProvider: "discord"},
}

const emailWorkerConsumer = "email-worker"

func (w Worker) Run(ctx context.Context) error {
	interval := w.PollInterval
	if interval <= 0 {
		interval = 60 * time.Second
	}
	manualInterval := w.ManualPollInterval
	if manualInterval <= 0 {
		manualInterval = 2 * time.Second
	}
	scheduledTicker := time.NewTicker(interval)
	manualTicker := time.NewTicker(manualInterval)
	defer scheduledTicker.Stop()
	defer manualTicker.Stop()

	if _, err := w.RunManualInboxJobs(ctx); err != nil {
		w.logWarn(err, "manual inbox sync iteration failed")
	}
	if _, err := w.RunOnce(ctx); err != nil {
		w.logWarn(err, "email sync iteration failed")
	}
	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-manualTicker.C:
			if _, err := w.RunManualInboxJobs(ctx); err != nil {
				w.logWarn(err, "manual inbox sync iteration failed")
			}
		case <-scheduledTicker.C:
			if _, err := w.RunOnce(ctx); err != nil {
				w.logWarn(err, "email sync iteration failed")
			}
		}
	}
}

// RunManualInboxJobs consumes explicit refreshes requested from Support. It
// does not use the generic integration-sync queue: those jobs can represent
// document ingestion, while these jobs must be fetched by the inbox worker.
func (w Worker) RunManualInboxJobs(ctx context.Context) (int, error) {
	if w.Integration == nil {
		return 0, nil
	}
	total := 0
	for {
		job, err := w.Integration.ClaimSyncJob(ctx, handoff.SyncClaimRequest{
			Consumer: emailWorkerConsumer,
			Target:   emailWorkerConsumer,
		})
		if err != nil {
			var serviceErr *handoff.ServiceHTTPError
			if errors.As(err, &serviceErr) && serviceErr.StatusCode == http.StatusNotFound {
				return total, nil
			}
			return total, err
		}
		ingested, processErr := w.processManualInboxJob(ctx, job)
		total += ingested
		if processErr != nil {
			return total, processErr
		}
	}
}

func (w Worker) processManualInboxJob(ctx context.Context, job store.SyncJob) (int, error) {
	plan, ok := inboxPlanForManualJob(job)
	if !ok {
		return 0, w.failManualInboxJob(ctx, job, "inbox_sync_plan_unavailable")
	}
	connection, err := w.connectionForManualJob(ctx, job)
	if err != nil {
		return 0, w.failManualInboxJob(ctx, job, "inbox_sync_connection_unavailable")
	}
	if !connectionEligible(connection, plan.capability, plan.scope) {
		return 0, w.failManualInboxJob(ctx, job, "inbox_sync_permission_unavailable")
	}
	ingested, err := w.syncConnection(ctx, connection, plan)
	if err != nil {
		if progressErr := w.failManualInboxJob(ctx, job, "inbox_sync_failed"); progressErr != nil {
			return ingested, fmt.Errorf("%w; record manual inbox failure: %v", err, progressErr)
		}
		// The terminal failed receipt is the operator-facing outcome for a
		// provider failure. Keep claiming the remaining independent jobs so a
		// bad mailbox cannot delay a healthy one in the same Support refresh.
		return ingested, nil
	}
	if _, err := w.Integration.UpdateSyncProgress(ctx, job.ID, handoff.SyncProgressRequest{
		Consumer: emailWorkerConsumer,
		Status:   "completed",
		Message:  "Inbox refresh completed.",
		Checkpoint: map[string]any{
			"inboxChannel": plan.inboxChannel(),
		},
		Metadata: map[string]any{
			"messagesIngested": ingested,
		},
	}); err != nil {
		return ingested, fmt.Errorf("complete manual inbox sync job: %w", err)
	}
	return ingested, nil
}

func (w Worker) failManualInboxJob(ctx context.Context, job store.SyncJob, failureCode string) error {
	_, err := w.Integration.UpdateSyncProgress(ctx, job.ID, handoff.SyncProgressRequest{
		Consumer: emailWorkerConsumer,
		Status:   "failed",
		Message:  "Inbox refresh failed. Check the connection status and reconnect if needed.",
		Metadata: map[string]any{
			"failureCode": failureCode,
		},
	})
	return err
}

func inboxPlanForManualJob(job store.SyncJob) (providerPlan, bool) {
	channel, _ := job.Metadata["inboxChannel"].(string)
	channel = strings.ToLower(strings.TrimSpace(channel))
	providerKey := strings.ToLower(strings.TrimSpace(job.ProviderKey))
	for _, plan := range providerPlans {
		if plan.providerKey == providerKey && plan.inboxChannel() == channel {
			return plan, true
		}
	}
	return providerPlan{}, false
}

func (w Worker) connectionForManualJob(ctx context.Context, job store.SyncJob) (store.Connection, error) {
	connections, err := w.Store.ListConnections(ctx, store.ConnectionFilter{ProviderKey: job.ProviderKey})
	if err != nil {
		return store.Connection{}, err
	}
	for _, connection := range connections {
		if connection.ID == job.ConnectionID && connection.OrganizationID == job.OrganizationID {
			return connection, nil
		}
	}
	return store.Connection{}, store.ErrNotFound
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
			ingested, err := w.syncConnection(ctx, conn, plan)
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
	if conn.Status != "active" {
		return false
	}
	if slices.Contains(conn.Capabilities, capability) {
		return true
	}
	return slices.ContainsFunc(conn.Scopes, func(s string) bool {
		return strings.EqualFold(s, scope)
	})
}

func (w Worker) syncConnection(ctx context.Context, conn store.Connection, plan providerPlan) (int, error) {
	fetcher := w.fetcherFor(plan)
	if fetcher == nil {
		return 0, fmt.Errorf("no email fetcher for provider %s", plan.source())
	}

	stateKey := plan.stateKey(conn.ID)
	state, err := w.Store.GetEmailSyncState(ctx, stateKey)
	if err != nil && !errors.Is(err, store.ErrNotFound) {
		return 0, fmt.Errorf("load sync state for %s: %w", stateKey, err)
	}
	state.ConnectionID = stateKey
	state.ProviderKey = conn.ProviderKey
	if plan.channelProvider != "" {
		state.ProviderKey = plan.channelProvider
	}

	token, err := w.Tokens.AccessTokenForConnection(ctx, conn.ID)
	if err != nil {
		return 0, w.recordConnectionFailure(ctx, conn.ID, state, fmt.Errorf("resolve access token: %w", err))
	}

	maxMessages := w.MaxPerCycle
	if maxMessages <= 0 {
		maxMessages = 100
	}
	backfill := w.BackfillWindow
	if backfill <= 0 {
		backfill = 24 * time.Hour
	}
	if plan.fetcherKey == "teams" {
		// Teams starts with 30 days. Each explicit inbox request adds another
		// durable 30-day window without changing the live-sync watermark state
		// until the fetcher begins that bounded replay.
		backfill = teamsBootstrapBackfill + time.Duration(max(state.HistoryBackfillDays, 0))*24*time.Hour
	}

	fetchCursor := decodeFetcherCursor(fetcher, state.Cursor)
	result, err := fetchWith(ctx, fetcher, conn, token.AccessToken, fetchCursor, backfill, maxMessages)
	if errors.Is(err, ErrCursorExpired) {
		// The provider invalidated our cursor (Gmail >~1 week of history,
		// Graph sync-state reset). Re-bootstrap immediately, once.
		w.logInfo("cursor expired for connection " + conn.ID + ", re-bootstrapping")
		result, err = fetchWith(ctx, fetcher, conn, token.AccessToken, "", backfill, maxMessages)
	}
	if err != nil {
		return 0, w.recordConnectionFailure(ctx, conn.ID, state, fmt.Errorf("fetch %s mailbox: %w", plan.source(), err))
	}

	if len(result.ProviderContextPatch) > 0 {
		updater, ok := w.Store.(ConnectionContextUpdater)
		if !ok {
			return 0, w.recordConnectionFailure(ctx, conn.ID, state, errors.New("persist provider context: store does not support connection context updates"))
		}
		for key, value := range result.ProviderContextPatch {
			updated, updateErr := updater.BindConnectionProviderContext(ctx, conn.ID, key, value)
			if updateErr != nil {
				return 0, w.recordConnectionFailure(ctx, conn.ID, state, fmt.Errorf("persist provider context: %w", updateErr))
			}
			conn = updated
		}
	}

	// deliveryConn is what the ingestor sees: chat plans stamp their channel
	// provider onto the copy so the bridge routes the thread into the
	// matching inbox channel; email plans pass the connection unchanged.
	deliveryConn := conn
	if plan.channelProvider != "" {
		deliveryConn.ProviderKey = plan.channelProvider
	}

	ingested := 0
	for _, msg := range result.Messages {
		// Self-echo guard: replies our own send ops produce also land in the
		// mailbox (Gmail threads them into INBOX conversations); skip mail
		// authored by the connected account itself.
		if msg.Direction != "outbound" && msg.From.Email != "" && strings.EqualFold(msg.From.Email, conn.UserEmail) {
			continue
		}
		if err := w.Ingest.Ingest(ctx, deliveryConn, msg); err != nil {
			// Do not advance the cursor past a failed ingest: the whole batch
			// re-runs next cycle and conversation-core's idempotency key makes
			// the already-ingested prefix a no-op.
			return ingested, w.recordConnectionFailure(ctx, conn.ID, state, fmt.Errorf("ingest message %s: %w", msg.ProviderEventID, err))
		}
		ingested++
	}

	state.Cursor = encodeFetcherCursor(fetcher, result.NextCursor)
	state.LastSyncedAt = time.Now().UTC()
	state.LastError = ""
	state.FailureCount = 0
	if err := w.Store.UpsertEmailSyncState(ctx, state); err != nil {
		return ingested, w.recordConnectionFailure(ctx, conn.ID, state, fmt.Errorf("persist sync state for %s: %w", stateKey, err))
	}
	w.updateConnectionSyncStatus(ctx, conn.ID, "synced")
	if ingested > 0 {
		w.logInfo(fmt.Sprintf("ingested %d %s message(s) for connection %s", ingested, deliveryConn.ProviderKey, conn.ID))
	}
	return ingested, nil
}

func (w Worker) fetcherFor(plan providerPlan) Fetcher {
	switch plan.fetcherKey {
	case "teams":
		return w.Teams
	case "slack":
		return w.Slack
	case "xdm":
		return w.XDM
	case "discord":
		return w.Discord
	case "":
	default:
		return nil
	}
	switch plan.providerKey {
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

func (w Worker) recordConnectionFailure(ctx context.Context, connectionID string, state store.EmailSyncState, cause error) error {
	err := w.recordFailure(ctx, state, cause)
	w.updateConnectionSyncStatus(ctx, connectionID, "failed")
	return err
}

func (w Worker) updateConnectionSyncStatus(ctx context.Context, connectionID, status string) {
	updater, ok := w.Store.(ConnectionSyncStatusUpdater)
	if !ok {
		return
	}
	if err := updater.UpdateConnectionSyncStatus(ctx, connectionID, status); err != nil {
		w.logWarn(err, "persist connection sync status for "+connectionID)
	}
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
