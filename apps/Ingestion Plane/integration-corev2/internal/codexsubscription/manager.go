// Package codexsubscription brokers a user's own ChatGPT subscription for
// text inference. By default it runs each turn in the pinned, hardened Codex
// app-server; the opt-in direct transport sends Responses API requests to the
// ChatGPT Codex backend with the connection's ChatGPT OAuth token. Either way
// the broker reads and refreshes that token in the connection's Codex
// auth.json, and the app-server remains the device-code sign-in flow. It
// never copies a ChatGPT access or refresh token into the database, a log, or
// another CoreSystem plane.
package codexsubscription

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
)

const (
	ProviderKey   = "openai-codex-subscription"
	ConnectorType = "openai-codex-subscription"
	Capability    = "model.inference"
)

var (
	ErrDisabled                 = errors.New("codex subscription connections are disabled")
	ErrLoginNotFound            = errors.New("subscription login was not found")
	ErrLoginExpired             = errors.New("subscription login expired")
	ErrInvalidRequest           = errors.New("invalid subscription invocation request")
	ErrReauthenticationRequired = errors.New("ChatGPT subscription reauthentication is required")
)

// Config controls the subscription broker. Home must be a persistent
// service-owned volume: the Codex sign-in writes each connection's auth.json
// there and the broker refreshes it in place, while the integration database
// stores only the opaque connection metadata.
type Config struct {
	Enabled           bool
	Home              string
	LoginTTL          time.Duration
	InvocationTimeout time.Duration
	// MaxConcurrency caps concurrent calls across all connections (default
	// 16); MaxConcurrencyPerConnection caps them per connection (default 4).
	MaxConcurrency              int
	MaxConcurrencyPerConnection int
	// QueueTimeout bounds the wait for a concurrency slot (default 30s);
	// past it the call is rate limited with RateLimitQueue.
	QueueTimeout time.Duration
}

// DeviceLogin is safe to return to the browser. The user code is short lived
// and is not a bearer credential.
type DeviceLogin struct {
	LoginID         string    `json:"loginId"`
	ConnectionID    string    `json:"connectionId"`
	VerificationURL string    `json:"verificationUrl"`
	UserCode        string    `json:"userCode"`
	ExpiresAt       time.Time `json:"expiresAt"`
}

// LoginStatus reports a pending device-code exchange without exposing an
// account access token or identity profile.
type LoginStatus struct {
	LoginID      string `json:"loginId"`
	ConnectionID string `json:"connectionId"`
	Status       string `json:"status"`
	ErrorCode    string `json:"errorCode,omitempty"`
	Message      string `json:"message,omitempty"`
}

// ChatMessage is deliberately a small text-only contract. Subscription
// inference exposes no tools, files, or execution to Model Plane: tool
// proposals travel as JSON text that Model Gateway validates.
type ChatMessage struct {
	Role    string `json:"role"`
	Content string `json:"content"`
	Name    string `json:"name,omitempty"`
}

type InvokeRequest struct {
	ConnectionID    string          `json:"connectionId"`
	RequestID       string          `json:"requestId"`
	Model           string          `json:"model"`
	Messages        []ChatMessage   `json:"messages"`
	MaxTokens       int             `json:"maxTokens"`
	ReasoningEffort string          `json:"reasoningEffort"`
	ServiceTier     string          `json:"serviceTier,omitempty"`
	OutputSchema    json.RawMessage `json:"outputSchema,omitempty"`
}

type InvokeResponse struct {
	RequestID string `json:"requestId"`
	Content   string `json:"content"`
	ModelUsed string `json:"modelUsed"`
	// Usage is the token accounting the upstream reported for the turn. It
	// is nil when none arrived before the turn completed, which callers must
	// read as unknown usage, not as a free turn.
	Usage *TokenUsage `json:"usage,omitempty"`
	// PhasesMs holds the broker's own phase durations in whole milliseconds,
	// keyed by the Phase* names below. A phase that did not run is absent.
	// Durations only: nothing here is derived from prompt or answer text.
	PhasesMs map[string]int64 `json:"phasesMs,omitempty"`
}

// TokenUsage is the token accounting of one turn, as response.completed's
// usage (direct) or the app-server's thread/tokenUsage/updated notification
// reports it. Counts only, never content. As in the OpenAI accounting,
// InputTokens already includes CachedInputTokens and CacheWriteInputTokens,
// and OutputTokens already includes ReasoningOutputTokens.
type TokenUsage struct {
	InputTokens           int64 `json:"inputTokens"`
	CachedInputTokens     int64 `json:"cachedInputTokens"`
	CacheWriteInputTokens int64 `json:"cacheWriteInputTokens"`
	OutputTokens          int64 `json:"outputTokens"`
	ReasoningOutputTokens int64 `json:"reasoningOutputTokens"`
}

// Broker phase names used as InvokeResponse.PhasesMs keys. Model Plane
// forwards them as upstream phase timings and drops any other key
// (inference-core codex_subscription.rs BROKER_PHASES), so they are part of
// this route's contract. The direct transport reports only turn_start,
// first_delta and turn_total: its turn_start runs from the first POST to the
// final attempt's response headers.
const (
	// PhaseSpawn is starting the per-invocation Codex app-server process.
	PhaseSpawn = "spawn"
	// PhaseInitialize is the initialize handshake with that process.
	PhaseInitialize = "initialize"
	// PhaseThreadStart is the thread/start request.
	PhaseThreadStart = "thread_start"
	// PhaseTurnStart is the turn/start request.
	PhaseTurnStart = "turn_start"
	// PhaseFirstDelta runs from turn/start's acknowledgement to the turn's
	// first answer text. Absent when no text arrived before completion.
	PhaseFirstDelta = "first_delta"
	// PhaseTurnTotal runs from turn/start's acknowledgement to the turn's
	// completion, so it contains PhaseFirstDelta.
	PhaseTurnTotal = "turn_total"
)

// Runner is the narrow seam between the Manager and the ChatGPT backend:
// inference through the selected Transport, sign-in and sign-out through the
// Codex CLI. Keeping credential custody and process ownership behind it lets
// HTTP handlers remain responsible for org/user authorization and audit
// decisions.
type Runner interface {
	BeginDeviceLogin(ctx context.Context, codeHome string) (LoginProcess, DeviceCode, error)
	Invoke(ctx context.Context, codeHome string, request InvokeRequest) (InvokeResponse, error)
	InvokeStream(ctx context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error)
	Logout(ctx context.Context, codeHome string) error
}

type DeviceCode struct {
	VerificationURL string
	UserCode        string
}

type LoginProcess interface {
	Poll(ctx context.Context) (ProcessLoginStatus, error)
	Close() error
}

type ProcessLoginStatus struct {
	Status    string
	ErrorCode string
	Message   string
}

type pendingLogin struct {
	connectionID string
	expiresAt    time.Time
	process      LoginProcess
}

// Manager owns in-memory device-code sessions and the invocation limits.
// Completed credentials remain in the per-connection Codex home; the broker
// reads and refreshes them there but never copies an OAuth token into the
// database, a log, or another plane.
type Manager struct {
	enabled           bool
	home              string
	loginTTL          time.Duration
	invocationTimeout time.Duration
	runner            Runner
	limiter           *invocationLimiter
	now               func() time.Time

	mu      sync.Mutex
	pending map[string]pendingLogin
}

func NewManager(cfg Config, runner Runner) (*Manager, error) {
	if !cfg.Enabled {
		return nil, ErrDisabled
	}
	if runner == nil {
		return nil, errors.New("codex subscription runner is required")
	}
	if strings.TrimSpace(cfg.Home) == "" {
		return nil, errors.New("codex subscription home is required")
	}
	home, err := filepath.Abs(strings.TrimSpace(cfg.Home))
	if err != nil {
		return nil, fmt.Errorf("resolve codex subscription home: %w", err)
	}
	if err := os.MkdirAll(home, 0o700); err != nil {
		return nil, fmt.Errorf("create codex subscription home: %w", err)
	}
	// No Codex process of an earlier broker instance survives it, so every
	// throwaway state directory left in the home is a leftover prompt log.
	sweepEphemeralHomes(home)
	if cfg.LoginTTL <= 0 {
		cfg.LoginTTL = 10 * time.Minute
	}
	if cfg.InvocationTimeout <= 0 {
		cfg.InvocationTimeout = 90 * time.Second
	}
	return &Manager{
		enabled:           true,
		home:              home,
		loginTTL:          cfg.LoginTTL,
		invocationTimeout: cfg.InvocationTimeout,
		runner:            runner,
		limiter:           newInvocationLimiter(cfg.MaxConcurrency, cfg.MaxConcurrencyPerConnection, cfg.QueueTimeout),
		now:               func() time.Time { return time.Now().UTC() },
		pending:           make(map[string]pendingLogin),
	}, nil
}

func (m *Manager) StartLogin(ctx context.Context, connectionID string) (DeviceLogin, error) {
	if m == nil || !m.enabled {
		return DeviceLogin{}, ErrDisabled
	}
	connectionID = strings.TrimSpace(connectionID)
	if connectionID == "" {
		return DeviceLogin{}, fmt.Errorf("%w: connection id is required", ErrInvalidRequest)
	}
	home, err := m.connectionHome(connectionID)
	if err != nil {
		return DeviceLogin{}, err
	}
	if err := os.MkdirAll(home, 0o700); err != nil {
		return DeviceLogin{}, fmt.Errorf("create connection codex home: %w", err)
	}
	process, code, err := m.runner.BeginDeviceLogin(ctx, home)
	if err != nil {
		return DeviceLogin{}, err
	}
	if strings.TrimSpace(code.VerificationURL) == "" || strings.TrimSpace(code.UserCode) == "" {
		_ = process.Close()
		return DeviceLogin{}, errors.New("codex app-server did not return a device code")
	}
	loginID := "codex_login_" + uuid.NewString()
	expiresAt := m.now().Add(m.loginTTL)
	m.mu.Lock()
	m.pending[loginID] = pendingLogin{connectionID: connectionID, expiresAt: expiresAt, process: process}
	m.mu.Unlock()
	return DeviceLogin{
		LoginID:         loginID,
		ConnectionID:    connectionID,
		VerificationURL: code.VerificationURL,
		UserCode:        code.UserCode,
		ExpiresAt:       expiresAt,
	}, nil
}

func (m *Manager) PollLogin(ctx context.Context, loginID string) (LoginStatus, error) {
	if m == nil || !m.enabled {
		return LoginStatus{}, ErrDisabled
	}
	loginID = strings.TrimSpace(loginID)
	m.mu.Lock()
	pending, found := m.pending[loginID]
	if found && !m.now().Before(pending.expiresAt) {
		delete(m.pending, loginID)
	}
	m.mu.Unlock()
	if !found {
		return LoginStatus{}, ErrLoginNotFound
	}
	if !m.now().Before(pending.expiresAt) {
		m.expireLogin(pending)
		return LoginStatus{LoginID: loginID, ConnectionID: pending.connectionID, Status: "expired", ErrorCode: "login_expired", Message: "The device-code sign-in window expired."}, ErrLoginExpired
	}
	state, err := pending.process.Poll(ctx)
	if err != nil {
		return LoginStatus{}, err
	}
	status := LoginStatus{LoginID: loginID, ConnectionID: pending.connectionID, Status: state.Status, ErrorCode: state.ErrorCode, Message: state.Message}
	if status.Status == "" {
		status.Status = "pending"
	}
	if status.Status == "connected" || status.Status == "failed" {
		m.mu.Lock()
		delete(m.pending, loginID)
		m.mu.Unlock()
		_ = pending.process.Close()
	}
	return status, nil
}

func (m *Manager) Invoke(ctx context.Context, request InvokeRequest) (InvokeResponse, error) {
	home, request, err := m.prepareInvocation(request)
	if err != nil {
		return InvokeResponse{}, err
	}
	invokeCtx, cancel := context.WithTimeout(ctx, m.invocationTimeout)
	defer cancel()
	release, waited, err := m.limiter.acquire(invokeCtx, request.ConnectionID)
	if err != nil {
		return InvokeResponse{}, err
	}
	defer release()
	return m.runner.Invoke(withQueueWait(invokeCtx, waited), home, request)
}

func (m *Manager) InvokeStream(ctx context.Context, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error) {
	run, release, err := m.ReserveStream(ctx, request)
	if err != nil {
		return InvokeResponse{}, err
	}
	defer release()
	return run(onDelta)
}

// ReserveStream validates a streamed invocation and takes its concurrency
// slots, so a caller can report a full queue as a rate limit before it
// commits to a streamed response. The invocation timeout starts here. run
// executes the turn; release frees the slots and ends the invocation context.
// The caller must call release exactly once, whether or not it calls run;
// release is idempotent.
func (m *Manager) ReserveStream(ctx context.Context, request InvokeRequest) (run func(onDelta func(string) error) (InvokeResponse, error), release func(), err error) {
	home, request, err := m.prepareInvocation(request)
	if err != nil {
		return nil, nil, err
	}
	invokeCtx, cancel := context.WithTimeout(ctx, m.invocationTimeout)
	releaseSlots, waited, err := m.limiter.acquire(invokeCtx, request.ConnectionID)
	if err != nil {
		cancel()
		return nil, nil, err
	}
	release = func() {
		releaseSlots()
		cancel()
	}
	run = func(onDelta func(string) error) (InvokeResponse, error) {
		if onDelta == nil {
			return InvokeResponse{}, fmt.Errorf("%w: stream delta callback is required", ErrInvalidRequest)
		}
		return m.runner.InvokeStream(withQueueWait(invokeCtx, waited), home, request, onDelta)
	}
	return run, release, nil
}

// TransportKind reports the inference transport in use, or "" when the
// runner does not say.
func (m *Manager) TransportKind() TransportKind {
	if m == nil {
		return ""
	}
	if reporter, ok := m.runner.(interface{ TransportKind() TransportKind }); ok {
		return reporter.TransportKind()
	}
	return ""
}

func (m *Manager) prepareInvocation(request InvokeRequest) (string, InvokeRequest, error) {
	if m == nil || !m.enabled {
		return "", InvokeRequest{}, ErrDisabled
	}
	if strings.TrimSpace(request.ConnectionID) == "" || strings.TrimSpace(request.Model) == "" || len(request.Messages) == 0 {
		return "", InvokeRequest{}, fmt.Errorf("%w: connectionId, model, and messages are required", ErrInvalidRequest)
	}
	if len(request.OutputSchema) > 0 {
		var schema map[string]any
		if len(request.OutputSchema) > 256*1024 || json.Unmarshal(request.OutputSchema, &schema) != nil || schema == nil || schema["type"] != "object" {
			return "", InvokeRequest{}, ErrInvalidRequest
		}
	}
	request.ReasoningEffort = strings.ToLower(strings.TrimSpace(request.ReasoningEffort))
	if request.ReasoningEffort == "" {
		request.ReasoningEffort = "low"
	}
	if request.ReasoningEffort != "low" && request.ReasoningEffort != "high" {
		return "", InvokeRequest{}, fmt.Errorf("%w: reasoningEffort must be low or high", ErrInvalidRequest)
	}
	request.ConnectionID = strings.TrimSpace(request.ConnectionID)
	request.ServiceTier = strings.ToLower(strings.TrimSpace(request.ServiceTier))
	if request.ServiceTier != "" && request.ServiceTier != "priority" {
		return "", InvokeRequest{}, fmt.Errorf("%w: serviceTier must be empty or priority", ErrInvalidRequest)
	}
	home, err := m.connectionHome(request.ConnectionID)
	if err != nil {
		return "", InvokeRequest{}, err
	}
	return home, request, nil
}

// Logout removes the managed Codex login from the isolated connection home.
// The directory itself is deliberately retained: deleting provider state on a
// path configured outside this process needs an explicit retention workflow.
func (m *Manager) Logout(ctx context.Context, connectionID string) error {
	if m == nil || !m.enabled {
		return ErrDisabled
	}
	home, err := m.connectionHome(connectionID)
	if err != nil {
		return err
	}
	return m.runner.Logout(ctx, home)
}

// Close ends every pending device-code login on shutdown, so their Codex
// processes and throwaway state directories do not outlive a graceful stop.
// A login in progress is abandoned: the user starts a new connection.
func (m *Manager) Close() error {
	if m == nil {
		return nil
	}
	m.mu.Lock()
	pending := m.pending
	m.pending = make(map[string]pendingLogin)
	m.mu.Unlock()
	for _, login := range pending {
		_ = login.process.Close()
	}
	return nil
}

func (m *Manager) expireLogin(pending pendingLogin) {
	home, err := m.connectionHome(pending.connectionID)
	if err == nil {
		logoutCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_ = m.runner.Logout(logoutCtx, home)
		cancel()
	}
	_ = pending.process.Close()
}

func (m *Manager) connectionHome(connectionID string) (string, error) {
	connectionID = strings.TrimSpace(connectionID)
	if connectionID == "" || strings.ContainsAny(connectionID, `\\/`) || connectionID == "." || connectionID == ".." {
		return "", fmt.Errorf("%w: invalid connection id", ErrInvalidRequest)
	}
	dir := filepath.Join(m.home, connectionID)
	rel, err := filepath.Rel(m.home, dir)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) || filepath.IsAbs(rel) {
		return "", fmt.Errorf("%w: connection home escapes configured root", ErrInvalidRequest)
	}
	return dir, nil
}
