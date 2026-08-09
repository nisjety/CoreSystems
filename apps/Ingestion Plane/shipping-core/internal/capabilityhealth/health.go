// Package capabilityhealth attests shipping-core's own runtime health to
// Model Plane's capability-core, mirroring execution-core's
// health_attest.rs pattern for cap.command.shell/cap.command.sandbox — but
// for the two shipping capabilities capability-core has never had a
// runtime authority for.
//
// # Why this exists
//
// capability-core denies dispatch before it ever evaluates risk: its policy
// path derives availability from the registry row, and a row that has never
// been attested reports `health_not_attested` and blocks — see
// execution-core's health_attest.rs for the established rationale this
// mirrors. execution-core only attests the two capabilities IT is the
// runtime authority for (cap.command.shell, cap.command.sandbox); nothing
// anywhere attests cap.tool.shipping.read or cap.tool.shipping.book, so both
// sat permanently unavailable regardless of whether real carrier
// credentials were configured.
//
// # Measure, then attest — never attest blind
//
// An attestation is a claim about health, so it is only ever made from a
// real probe: Probe drives a synthetic-but-realistic quote request through
// the SAME quoteengine.Engine every live /api/quotes call uses, and only
// counts a carrier as proof of connectivity via the engine's own
// established "verified" bookkeeping (a non-mock carrier that has
// successfully returned at least one quote). A failed probe attests
// nothing — leaving the rows unavailable is the correct, fail-closed
// outcome, not a shipping-core failure.
package capabilityhealth

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	"shipping-core/internal/carrier"
	"shipping-core/internal/quoteengine"
)

const (
	// ReadCapabilityID is the low-risk read surface (get_shipping_quotes /
	// shipping_carriers).
	ReadCapabilityID = "cap.tool.shipping.read"
	// BookCapabilityID is the high-risk, approval-gated booking surface.
	// Attesting it available does not bypass approval: capability-core's
	// policy engine still returns "ask" for a high-risk capability. This
	// only proves the underlying runtime (a working carrier connection)
	// exists at all, exactly as execution-core attests cap.command.shell
	// from the same sandbox probe it uses for the low-risk sandbox
	// capability.
	BookCapabilityID = "cap.tool.shipping.book"

	tokenReason = "shipping-core runtime capability health attestation"

	// defaultIntervalSeconds sits well inside capability-core's 5-minute
	// attestation TTL, but deliberately slower than execution-core's local
	// sandbox-probe heartbeat: each tick drives a REAL request against a
	// live third-party carrier API, not a local process probe, so this
	// heartbeat should not hammer that API any faster than it has to.
	defaultIntervalSeconds = 120
	minIntervalSeconds     = 30
	// attestationTTLSeconds mirrors capability-core's
	// AvailabilityAttestationTTL. Never sent — only the ceiling the
	// interval is clamped against.
	attestationTTLSeconds = 300
	maxIntervalSeconds     = attestationTTLSeconds / 2

	requestTimeout = 10 * time.Second
	probeTimeout   = 15 * time.Second
)

// Config is read from the environment. Shipping-core already mints
// service-to-service credentials with INGESTION_SERVICE_ID/
// SHIPPING_SERVICE_API_KEY for its Data Plane client (internal/dataplane);
// reused here for the capability-core audience instead.
type Config struct {
	AuthCoreURL       string
	CapabilityCoreURL string
	ServiceID         string
	ServiceCredential string
	IntervalSeconds   int
}

// ConfigFromEnv reads AUTH_CORE_URL, CAPABILITY_CORE_HTTP_URL, and the
// shared service credential. A missing piece means Configured() reports
// false and the heartbeat logs once and stays off, rather than retrying a
// call it knows cannot succeed.
func ConfigFromEnv() Config {
	return Config{
		AuthCoreURL:       strings.TrimRight(os.Getenv("AUTH_CORE_URL"), "/"),
		CapabilityCoreURL: strings.TrimRight(os.Getenv("CAPABILITY_CORE_HTTP_URL"), "/"),
		ServiceID:         envOr("INGESTION_SERVICE_ID", "shipping-core"),
		ServiceCredential: firstEnv("SHIPPING_SERVICE_API_KEY", "INGESTION_SERVICE_API_KEY"),
		IntervalSeconds:   intervalFromEnv(os.Getenv("SHIPPING_CAPABILITY_HEALTH_INTERVAL_SECONDS")),
	}
}

// Configured reports whether enough configuration exists to attest health.
func (c Config) Configured() bool {
	return c.AuthCoreURL != "" && c.CapabilityCoreURL != "" && c.ServiceID != "" && c.ServiceCredential != ""
}

func intervalFromEnv(raw string) int {
	raw = strings.TrimSpace(raw)
	if raw != "" {
		if value, err := strconv.Atoi(raw); err == nil && value > 0 {
			return clampInterval(value)
		}
	}
	return defaultIntervalSeconds
}

func clampInterval(value int) int {
	if value < minIntervalSeconds {
		return minIntervalSeconds
	}
	if value > maxIntervalSeconds {
		return maxIntervalSeconds
	}
	return value
}

// Attestor mints capability-core credentials and posts availability.
type Attestor struct {
	cfg    Config
	http   *http.Client
	logger *slog.Logger

	mu    sync.Mutex
	token string
	exp   time.Time
}

// NewAttestor builds an Attestor from deployment configuration.
func NewAttestor(cfg Config, logger *slog.Logger) *Attestor {
	return &Attestor{
		cfg:    cfg,
		http:   &http.Client{Timeout: requestTimeout},
		logger: logger,
	}
}

// bearer mints (or reuses) a capability-core-audience token scoped to
// exactly the two scopes this attestor needs: reading a row's current
// version, and writing global runtime health.
func (a *Attestor) bearer(ctx context.Context) (string, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.token != "" && time.Now().Before(a.exp) {
		return a.token, nil
	}

	body, err := json.Marshal(map[string]any{
		"orgId":  "global",
		"scopes": []string{"capability:read", "capability:health:global:write"},
		"reason": tokenReason,
	})
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: encode token request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, a.cfg.AuthCoreURL+"/api/capability-core/internal-token", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: build token request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-ID", a.cfg.ServiceID)
	req.Header.Set("X-Service-API-Key", a.cfg.ServiceCredential)

	resp, err := a.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: token request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<16))
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: read token response: %w", err)
	}
	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		return "", fmt.Errorf("capabilityhealth: auth core returned %d: %s", resp.StatusCode, raw)
	}
	var parsed struct {
		Token            string `json:"token"`
		ExpiresInSeconds int    `json:"expiresInSeconds"`
		Audience         string `json:"audience"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil || strings.TrimSpace(parsed.Token) == "" {
		return "", fmt.Errorf("capabilityhealth: invalid token response")
	}
	if parsed.Audience != "capability-core" {
		return "", fmt.Errorf("capabilityhealth: token minted for unexpected audience %q", parsed.Audience)
	}
	ttl := time.Duration(parsed.ExpiresInSeconds) * time.Second
	if ttl <= 30*time.Second {
		ttl = 4 * time.Minute
	}
	a.token = parsed.Token
	a.exp = time.Now().Add(ttl - 30*time.Second)
	return a.token, nil
}

// rowVersion reads a capability row's current version immediately before
// attesting it: capability-core guards the availability write with an
// optimistic-concurrency check against this exact value.
func (a *Attestor) rowVersion(ctx context.Context, bearer, capabilityID string) (string, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, a.cfg.CapabilityCoreURL+"/api/v1/capabilities/"+capabilityID, nil)
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: build row request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+bearer)

	resp, err := a.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: row request failed: %w", err)
	}
	defer resp.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<16))
	if err != nil {
		return "", fmt.Errorf("capabilityhealth: read row response: %w", err)
	}
	if resp.StatusCode != http.StatusOK {
		return "", fmt.Errorf("capabilityhealth: capability-core did not return the %s row (status %d)", capabilityID, resp.StatusCode)
	}
	var parsed struct {
		Version string `json:"version"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil || strings.TrimSpace(parsed.Version) == "" {
		return "", fmt.Errorf("capabilityhealth: %s row has no version to attest against", capabilityID)
	}
	return parsed.Version, nil
}

// attestationBody mirrors capability-core's exact accepted field set
// (the server decodes with DisallowUnknownFields) — see execution-core's
// Attestation struct in health_attest.rs for the same contract in Rust.
type attestationBody struct {
	ID            string `json:"id"`
	Version       string `json:"version"`
	State         string `json:"state"`
	ReasonCode    string `json:"reason_code"`
	Reason        string `json:"reason"`
	ExecutionMode string `json:"execution_mode"`
	CostClass     string `json:"cost_class"`
}

// attest reads capabilityID's current version, then posts a single
// "available" attestation for it.
func (a *Attestor) attest(ctx context.Context, capabilityID, reasonCode, reason string) error {
	bearer, err := a.bearer(ctx)
	if err != nil {
		return err
	}
	version, err := a.rowVersion(ctx, bearer, capabilityID)
	if err != nil {
		return err
	}
	body, err := json.Marshal(attestationBody{
		ID:      capabilityID,
		Version: version,
		State:   "available",
		// capability-core requires "agentic" for any approval-gated
		// capability (cap.tool.shipping.book is high-risk); using it for
		// both keeps read/book on the same declared execution mode.
		ExecutionMode: "agentic",
		// Matches the cost_class already registered on both rows: a real
		// carrier call has variable latency/cost, unlike execution-core's
		// bounded local sandbox probe.
		CostClass:  "variable",
		ReasonCode: reasonCode,
		Reason:     reason,
	})
	if err != nil {
		return fmt.Errorf("capabilityhealth: encode attestation: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, a.cfg.CapabilityCoreURL+"/api/v1/capabilities/availability", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("capabilityhealth: build attestation request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+bearer)

	resp, err := a.http.Do(req)
	if err != nil {
		return fmt.Errorf("capabilityhealth: attestation request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		raw, _ := io.ReadAll(io.LimitReader(resp.Body, 1<<16))
		return fmt.Errorf("capability-core refused the %s attestation (status %d): %s", capabilityID, resp.StatusCode, raw)
	}
	return nil
}

// syntheticProbeRequest is a safe, side-effect-free domestic Norway lane
// used purely to verify a carrier connection is live — never a real
// customer shipment, and never booked.
func syntheticProbeRequest() carrier.QuoteRequest {
	return carrier.QuoteRequest{
		From: carrier.Address{
			Name: "shipping-core health probe", Street: "Fjordgata 1",
			PostalCode: "0150", City: "Oslo", Country: "NO", IsBusiness: true,
		},
		To: carrier.Address{
			Name: "shipping-core health probe", Street: "Torggata 5",
			PostalCode: "5014", City: "Bergen", Country: "NO", IsBusiness: false,
		},
		Package: carrier.Package{WeightKg: 1, LengthCm: 20, WidthCm: 15, HeightCm: 10},
		Segment: carrier.SegmentB2C,
	}
}

// Probe runs a real, synthetic quote fan-out and reports whether at least
// one genuinely-connected (non-mock) carrier answered it. This is the
// ONLY signal availability is ever derived from — never attested blind.
// It deliberately reuses the engine's own "verified" bookkeeping
// (Carriers()'s VerifiedAt, set by GetQuotes as a side effect) rather than
// re-deriving connectivity logic here, so this stays in lockstep with
// whatever /api/carriers already reports.
func Probe(ctx context.Context, engine *quoteengine.Engine) (ok bool, detail string) {
	engine.GetQuotes(ctx, syntheticProbeRequest())
	for _, info := range engine.Carriers() {
		if info.Mode == carrier.ModeMock {
			continue
		}
		if info.VerifiedAt != nil {
			return true, fmt.Sprintf(
				"carrier %s verified by a successful upstream quote at %s",
				info.Code, info.VerifiedAt.UTC().Format(time.RFC3339),
			)
		}
	}
	return false, "no non-mock carrier has been verified by a successful upstream quote"
}

// RunHeartbeat drives a periodic real probe and attests both shipping
// capabilities only when it succeeds. Deliberately infallible and
// detached: a probe or attestation failure is logged and simply leaves
// the capability unavailable — the correct fail-closed state, not an
// outage of shipping-core itself. Ticks immediately on start so a fresh
// deploy doesn't sit unavailable for a full interval.
func RunHeartbeat(ctx context.Context, engine *quoteengine.Engine, logger *slog.Logger) {
	cfg := ConfigFromEnv()
	if !cfg.Configured() {
		logger.Warn("capability health attestation is not configured (AUTH_CORE_URL/CAPABILITY_CORE_HTTP_URL/service credential missing); shipping capabilities stay unavailable in capability-core")
		return
	}
	attestor := NewAttestor(cfg, logger)
	interval := time.Duration(cfg.IntervalSeconds) * time.Second

	tick := func() {
		pctx, cancel := context.WithTimeout(ctx, probeTimeout)
		defer cancel()
		ok, detail := Probe(pctx, engine)
		if !ok {
			logger.Warn("shipping capability health probe found no verified carrier; capabilities stay unavailable", "detail", detail)
			return
		}
		for _, id := range []string{ReadCapabilityID, BookCapabilityID} {
			actx, cancel := context.WithTimeout(ctx, requestTimeout)
			err := attestor.attest(actx, id, "shipping_quote_probe_succeeded", detail)
			cancel()
			if err != nil {
				logger.Warn("shipping capability attestation failed; capability stays unavailable", "capability", id, "err", err.Error())
				continue
			}
			logger.Info("shipping capability health attested", "capability", id)
		}
	}

	tick()
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			tick()
		}
	}
}

func envOr(key, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallback
}

func firstEnv(keys ...string) string {
	for _, key := range keys {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			return value
		}
	}
	return ""
}
