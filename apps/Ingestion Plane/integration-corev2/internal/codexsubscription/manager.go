// Package codexsubscription brokers a user's ChatGPT subscription to the
// official Codex app-server. It intentionally never exposes a reusable
// ChatGPT access or refresh token to another CoreSystem plane.
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

// Config controls the local Codex app-server broker. Home must be a persistent
// service-owned volume: Codex stores and refreshes ChatGPT auth there, while the
// integration database stores only the opaque connection metadata.
type Config struct {
	Enabled           bool
	Home              string
	LoginTTL          time.Duration
	InvocationTimeout time.Duration
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

// ChatMessage is deliberately a small text-only contract. Codex subscription
// inference does not expose tools, files, or structured-output execution to
// Model Plane in this first integration.
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
}

// Runner is the narrow seam around the official Codex app-server protocol.
// Keeping persistence and process ownership here lets HTTP handlers remain
// responsible for org/user authorization and audit decisions.
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

// Manager owns in-memory device-code sessions. Completed credentials remain in
// the per-connection Codex home, managed by Codex itself; no OAuth token is
// copied into this process or database.
type Manager struct {
	enabled           bool
	home              string
	loginTTL          time.Duration
	invocationTimeout time.Duration
	runner            Runner
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
	return m.runner.Invoke(invokeCtx, home, request)
}

func (m *Manager) InvokeStream(ctx context.Context, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error) {
	home, request, err := m.prepareInvocation(request)
	if err != nil {
		return InvokeResponse{}, err
	}
	if onDelta == nil {
		return InvokeResponse{}, fmt.Errorf("%w: stream delta callback is required", ErrInvalidRequest)
	}
	invokeCtx, cancel := context.WithTimeout(ctx, m.invocationTimeout)
	defer cancel()
	return m.runner.InvokeStream(invokeCtx, home, request, onDelta)
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
