// Package healthattest reports cap.browser.open's runtime health to
// capability-core on browser-broker's own owner lane.
//
// browser-broker owns the browser capability's runtime: every browser run needs
// a grant it issued, and Quarry (quarry-edge) validates those grants against it
// before it executes browser_agent. So the broker is the one workload that can
// truthfully say whether cap.browser.open can run. capability-core refuses that
// id on the generic and tenant health lanes by design and accepts it only from
// the exact service:browser-broker identity, in org global, holding
// capability:browser:health:write, on POST /api/v1/capabilities/browser/health.
//
// The attestation is renewed on a ticker well inside capability-core's
// five-minute freshness window. When the broker cannot measure its health (a
// failed token mint or row read) it posts nothing: "could not measure" is never
// written as health, and the row lapses to health_attestation_stale by itself.
//
// Nothing here is fatal: the broker must keep serving grants even when it
// cannot report its health, so every configuration or runtime failure only
// disables or skips the attestation and logs why. The service credential, the
// minted token, URLs and response bodies are never logged.
package healthattest

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/model-plane/pkg/servicetoken"
)

// Environment variables read by ConfigFromEnv.
const (
	EnvCapabilityCoreURL = "CAPABILITY_CORE_HTTP_URL"
	EnvAuthCoreURL       = "AUTH_CORE_URL"
	EnvServiceAPIKey     = "BROWSER_BROKER_SERVICE_API_KEY"
	EnvQuarryReadyURL    = "BROWSER_BROKER_QUARRY_READY_URL"
	EnvInterval          = "BROWSER_BROKER_HEALTH_ATTEST_INTERVAL_SECONDS"
)

// The attestation contract with capability-core and Auth Core.
const (
	CapabilityID = "cap.browser.open"
	// ServiceID is the registry key in the Control Plane principal policy; Auth
	// Core signs the token for "service:browser-broker".
	ServiceID = "browser-broker"
	Audience  = "capability-core"
	// OrgID is the pseudo-organization the global capability row lives in.
	OrgID       = "global"
	ReadScope   = "capability:read"
	WriteScope  = "capability:browser:health:write"
	tokenReason = "browser-broker attests cap.browser.open runtime health"

	attestPath = "/api/v1/capabilities/browser/health"
	rowPath    = "/api/v1/capabilities/" + CapabilityID

	// The ticker cadence. capability-core treats an attestation older than five
	// minutes as stale, so the ceiling is half of that: one missed beat never
	// lapses the row.
	DefaultInterval = 60 * time.Second
	MinInterval     = 15 * time.Second
	MaxInterval     = 150 * time.Second

	requestTimeout     = 5 * time.Second
	grantStoreTimeout  = 2 * time.Second
	quarryReadyTimeout = 3 * time.Second
	maxRowBytes        = 64 << 10
)

// The states and reason codes this attester posts.
const (
	StateAvailable = "available"
	StateUnhealthy = "unhealthy"

	ReasonReady                 = "browser_broker_ready"
	ReasonGRPCNotServing        = "browser_broker_not_serving"
	ReasonGrantStoreUnavailable = "grant_store_unavailable"
	ReasonQuarryEdgeNotReady    = "quarry_edge_not_ready"
)

// Config is the attester's validated configuration.
type Config struct {
	CapabilityCoreURL string
	AuthCoreURL       string
	// ServiceAPIKey is the browser-broker service-principal credential. Never
	// logged and never part of an error.
	ServiceAPIKey string
	// QuarryReadyURL is optional; when empty the Quarry probe is skipped.
	QuarryReadyURL string
	Interval       time.Duration
}

// ConfigFromEnv reads the attester's configuration.
//
// With none of the three required variables set the attester is simply off
// (enabled=false, nil error). With only some of them set it is off too, and the
// error names the missing variables, never a value. An unusable URL disables it
// the same way. An interval that is not an integer falls back to the default;
// any integer is clamped to [MinInterval, MaxInterval].
func ConfigFromEnv(getenv func(string) string) (Config, bool, error) {
	cfg := Config{
		CapabilityCoreURL: strings.TrimRight(strings.TrimSpace(getenv(EnvCapabilityCoreURL)), "/"),
		AuthCoreURL:       strings.TrimRight(strings.TrimSpace(getenv(EnvAuthCoreURL)), "/"),
		ServiceAPIKey:     strings.TrimSpace(getenv(EnvServiceAPIKey)),
		QuarryReadyURL:    strings.TrimSpace(getenv(EnvQuarryReadyURL)),
		Interval:          IntervalFromEnv(getenv(EnvInterval)),
	}
	var missing []string
	if cfg.CapabilityCoreURL == "" {
		missing = append(missing, EnvCapabilityCoreURL)
	}
	if cfg.AuthCoreURL == "" {
		missing = append(missing, EnvAuthCoreURL)
	}
	if cfg.ServiceAPIKey == "" {
		missing = append(missing, EnvServiceAPIKey)
	}
	if len(missing) == 3 {
		return cfg, false, nil
	}
	if len(missing) > 0 {
		return cfg, false, errors.New("healthattest: missing " + strings.Join(missing, ", "))
	}
	for name, raw := range map[string]string{
		EnvCapabilityCoreURL: cfg.CapabilityCoreURL,
		EnvAuthCoreURL:       cfg.AuthCoreURL,
		EnvQuarryReadyURL:    cfg.QuarryReadyURL,
	} {
		if raw == "" {
			continue
		}
		if !validHTTPURL(raw) {
			return cfg, false, errors.New("healthattest: " + name + " is not an http(s) URL")
		}
	}
	return cfg, true, nil
}

// IntervalFromEnv parses the attestation interval in seconds.
func IntervalFromEnv(raw string) time.Duration {
	seconds, err := strconv.Atoi(strings.TrimSpace(raw))
	if err != nil {
		return DefaultInterval
	}
	return min(max(time.Duration(seconds)*time.Second, MinInterval), MaxInterval)
}

func validHTTPURL(raw string) bool {
	parsed, err := url.Parse(raw)
	return err == nil && (parsed.Scheme == "http" || parsed.Scheme == "https") && parsed.Host != ""
}

// Attester posts cap.browser.open's health to capability-core.
type Attester struct {
	cfg    Config
	tokens *servicetoken.Provider
	client *http.Client
	ready  func(context.Context) error
	// serving reports whether the broker's gRPC surface (ValidateGrant and
	// the rest) is up; nil means it is not checked.
	serving func() bool
	logger  *slog.Logger
}

// New builds an Attester. ready is the grant-store ping /readyz uses; a nil
// ready (ephemeral development) counts as ready.
func New(cfg Config, ready func(context.Context) error, logger *slog.Logger) (*Attester, error) {
	if logger == nil {
		logger = slog.Default()
	}
	client := noRedirectClient()
	tokens, err := servicetoken.New(servicetoken.Config{
		AuthCoreURL: cfg.AuthCoreURL,
		ServiceID:   ServiceID,
		Credential:  cfg.ServiceAPIKey,
		Audience:    Audience,
		Scopes:      []string{ReadScope, WriteScope},
		Reason:      tokenReason,
		HTTPClient:  client,
	})
	if err != nil {
		return nil, err
	}
	if cfg.Interval <= 0 {
		cfg.Interval = DefaultInterval
	}
	return &Attester{cfg: cfg, tokens: tokens, client: client, ready: ready, logger: logger}, nil
}

// noRedirectClient never follows a redirect: a privileged credential must not
// be replayed to wherever a 3xx points, and a redirected readiness probe is not
// a ready answer. The 3xx response itself is returned and treated as a failure.
func noRedirectClient() *http.Client {
	return &http.Client{
		Timeout: requestTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
}

// Start reads the configuration and, when it is complete, runs the attester in
// the background until ctx ends. It reports whether the attester started. It
// never fails the caller: an incomplete configuration is a WARN, an absent one
// an INFO. serving reports whether the broker's gRPC server is serving; the
// caller starts the attester only once that server has been launched, so the
// capability is never attested available before grants can be validated.
func Start(ctx context.Context, getenv func(string) string, ready func(context.Context) error, serving func() bool, logger *slog.Logger) bool {
	if logger == nil {
		logger = slog.Default()
	}
	cfg, enabled, err := ConfigFromEnv(getenv)
	if err != nil {
		logger.Warn("cap.browser.open health attestation off: configuration is incomplete", "error", err.Error())
		return false
	}
	if !enabled {
		logger.Info("cap.browser.open health attestation off",
			"required", strings.Join([]string{EnvCapabilityCoreURL, EnvAuthCoreURL, EnvServiceAPIKey}, ","))
		return false
	}
	attester, err := New(cfg, ready, logger)
	if err != nil {
		logger.Warn("cap.browser.open health attestation off: configuration is invalid", "error", err.Error())
		return false
	}
	attester.serving = serving
	logger.Info("cap.browser.open health attestation on",
		"interval_seconds", int(cfg.Interval.Seconds()),
		"quarry_probe", cfg.QuarryReadyURL != "")
	go attester.Run(ctx)
	return true
}

// Run attests once immediately, then on every tick until ctx ends.
func (a *Attester) Run(ctx context.Context) {
	_ = a.Attest(ctx)
	ticker := time.NewTicker(a.cfg.Interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			_ = a.Attest(ctx)
		}
	}
}

// Probe measures the broker's runtime: its own gRPC server first, then the
// grant store, then (when configured) quarry-edge's readiness. quarry-edge's
// /ready is its durability flag, set once at boot: it says the Quarry process
// is up, not that a grant validation round trip works end to end.
func (a *Attester) Probe(ctx context.Context) (state, reasonCode string) {
	if a.serving != nil && !a.serving() {
		return StateUnhealthy, ReasonGRPCNotServing
	}
	if a.ready != nil {
		pingCtx, cancel := context.WithTimeout(ctx, grantStoreTimeout)
		err := a.ready(pingCtx)
		cancel()
		if err != nil {
			return StateUnhealthy, ReasonGrantStoreUnavailable
		}
	}
	if a.cfg.QuarryReadyURL != "" && !a.quarryReady(ctx) {
		return StateUnhealthy, ReasonQuarryEdgeNotReady
	}
	return StateAvailable, ReasonReady
}

func (a *Attester) quarryReady(ctx context.Context) bool {
	probeCtx, cancel := context.WithTimeout(ctx, quarryReadyTimeout)
	defer cancel()
	request, err := http.NewRequestWithContext(probeCtx, http.MethodGet, a.cfg.QuarryReadyURL, nil)
	if err != nil {
		return false
	}
	response, err := a.client.Do(request)
	if err != nil {
		return false
	}
	drainAndClose(response)
	return response.StatusCode == http.StatusOK
}

// Errors returned by Attest. They carry no URL, token or response body.
var (
	ErrTokenMint = errors.New("healthattest: service token mint failed")
	ErrRowRead   = errors.New("healthattest: capability row read failed")
	ErrPost      = errors.New("healthattest: health attestation refused")
)

type availabilityBody struct {
	ID            string `json:"id"`
	Version       string `json:"version"`
	State         string `json:"state"`
	ReasonCode    string `json:"reason_code"`
	Reason        string `json:"reason"`
	ExecutionMode string `json:"execution_mode"`
	CostClass     string `json:"cost_class"`
}

// Attest measures the runtime and posts one attestation. A 401 from
// capability-core drops the cached token and retries once with a fresh one.
func (a *Attester) Attest(ctx context.Context) error {
	for attempt := 0; ; attempt++ {
		status, err := a.attestOnce(ctx)
		if status == http.StatusUnauthorized && attempt == 0 {
			a.tokens.Invalidate(OrgID)
			continue
		}
		return err
	}
}

func (a *Attester) attestOnce(ctx context.Context) (int, error) {
	token, err := a.tokens.Token(ctx, OrgID)
	if err != nil {
		// The provider's error can quote Auth Core's refusal; keep it out of the log.
		a.logger.Warn("cap.browser.open health not attested: service token mint failed")
		return 0, ErrTokenMint
	}
	version, status, err := a.rowVersion(ctx, token)
	if err != nil {
		a.logger.Warn("cap.browser.open health not attested: capability row read failed", "status", status)
		return status, ErrRowRead
	}

	state, reasonCode := a.Probe(ctx)
	executionMode := "agentic"
	if state != StateAvailable {
		executionMode = "unavailable"
	}
	body, err := json.Marshal(availabilityBody{
		ID:            CapabilityID,
		Version:       version,
		State:         state,
		ReasonCode:    reasonCode,
		Reason:        "",
		ExecutionMode: executionMode,
		CostClass:     "variable",
	})
	if err != nil {
		return 0, err
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.cfg.CapabilityCoreURL+attestPath, bytes.NewReader(body))
	if err != nil {
		return 0, ErrPost
	}
	request.Header.Set("Authorization", "Bearer "+token)
	request.Header.Set("Content-Type", "application/json")
	response, err := a.client.Do(request)
	if err != nil {
		a.logger.Warn("cap.browser.open health attestation failed: capability-core did not answer")
		return 0, ErrPost
	}
	drainAndClose(response)
	if response.StatusCode != http.StatusOK {
		a.logger.Warn("cap.browser.open health attestation refused", "status", response.StatusCode)
		return response.StatusCode, ErrPost
	}
	a.logger.Info("cap.browser.open health attested", "state", state, "reason_code", reasonCode)
	return response.StatusCode, nil
}

// rowVersion reads the capability row's current version, which capability-core
// uses as the optimistic-concurrency guard on the attestation.
func (a *Attester) rowVersion(ctx context.Context, token string) (string, int, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, a.cfg.CapabilityCoreURL+rowPath, nil)
	if err != nil {
		return "", 0, err
	}
	request.Header.Set("Authorization", "Bearer "+token)
	response, err := a.client.Do(request)
	if err != nil {
		return "", 0, err
	}
	defer drainAndClose(response)
	if response.StatusCode != http.StatusOK {
		return "", response.StatusCode, ErrRowRead
	}
	var row struct {
		Version string `json:"version"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, maxRowBytes)).Decode(&row); err != nil {
		return "", response.StatusCode, ErrRowRead
	}
	version := strings.TrimSpace(row.Version)
	if version == "" {
		return "", response.StatusCode, ErrRowRead
	}
	return version, response.StatusCode, nil
}

func drainAndClose(response *http.Response) {
	_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxRowBytes))
	_ = response.Body.Close()
}
