// Package capabilityhealth reports wiki-store's room canvas readiness to Capability Core (P-CFG, WS19-S9).
//
// Capability Core names wiki-store (service:wiki-store) as the only attester of the two room canvas capabilities on its
// owner-action lane. This reporter is that attester. It is narrower than a process health check: it attests only after its probe
// says the canvas routes are actually able to serve (the WIKISTORE_SPACE_CANVAS switch is on, a Control decision key set is
// loaded and the canvas store answers). A failed probe, mint, row read or attestation attests nothing, and Capability Core's
// expiry of the last attestation is the fail-closed signal. The reporter never changes wiki-store's own serving status.
package capabilityhealth

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const (
	// CanvasReadCapabilityID and CanvasWriteCapabilityID are the room canvas rows wiki-store owns.
	CanvasReadCapabilityID  = "cap.tool.space.canvas.read"
	CanvasWriteCapabilityID = "cap.tool.space.canvas.write"

	ownerHealthScope = "capability:owner-action:health:write"
	readScope        = "capability:read"
	ownerHealthPath  = "/api/v1/capabilities/owner-actions/health"
	defaultInterval  = 2 * time.Minute
	maxResponseBytes = 64 << 10
)

// Capabilities is the fixed list this reporter attests, in order.
var Capabilities = []string{CanvasReadCapabilityID, CanvasWriteCapabilityID}

// Probe reports whether the canvas routes can serve. A nil error is ready.
type Probe func(ctx context.Context) error

// Config is the service-to-service configuration of the reporter. The credential is exchanged at Auth Core for a short-lived
// capability-core token and is never sent to Capability Core.
type Config struct {
	CapabilityCoreURL string
	AuthCoreURL       string
	ServiceID         string
	Credential        string
	Interval          time.Duration
	HTTPClient        *http.Client
	Probe             Probe
}

// Configured reports whether any value of the optional block is set. An unset block disables the reporter.
func (c Config) Configured() bool {
	for _, value := range []string{c.CapabilityCoreURL, c.AuthCoreURL, c.ServiceID, c.Credential} {
		if strings.TrimSpace(value) != "" {
			return true
		}
	}
	return false
}

// Reporter is a detached, fail-closed readiness heartbeat.
type Reporter struct {
	capabilityCoreURL string
	authCoreURL       string
	serviceID         string
	credential        string
	interval          time.Duration
	http              *http.Client
	probe             Probe
}

type tokenRequest struct {
	OrgID  string   `json:"orgId"`
	Scopes []string `json:"scopes"`
	Reason string   `json:"reason"`
}

type tokenResponse struct {
	Token    string `json:"token"`
	Audience string `json:"audience"`
}

type capabilityRow struct {
	ID      string `json:"id"`
	Version string `json:"version"`
}

type attestation struct {
	ID            string `json:"id"`
	Version       string `json:"version"`
	State         string `json:"state"`
	ReasonCode    string `json:"reason_code"`
	Reason        string `json:"reason"`
	ExecutionMode string `json:"execution_mode"`
	CostClass     string `json:"cost_class"`
}

// New validates the complete block. A partial block, or a block without a probe, is refused: it must never produce an anonymous
// or unprobed health write.
func New(cfg Config) (*Reporter, error) {
	values := []string{cfg.CapabilityCoreURL, cfg.AuthCoreURL, cfg.ServiceID, cfg.Credential}
	for _, value := range values {
		if strings.TrimSpace(value) == "" {
			return nil, errors.New("canvas capability health reporter requires the Capability Core URL, the Auth Core URL, the service id and the service credential together")
		}
	}
	if cfg.Probe == nil {
		return nil, errors.New("canvas capability health reporter requires a readiness probe")
	}
	capURL, err := validateURL(cfg.CapabilityCoreURL, "Capability Core URL")
	if err != nil {
		return nil, err
	}
	authURL, err := validateURL(cfg.AuthCoreURL, "Auth Core URL")
	if err != nil {
		return nil, err
	}
	interval := cfg.Interval
	if interval <= 0 {
		interval = defaultInterval
	}
	client := cfg.HTTPClient
	if client == nil {
		client = &http.Client{Timeout: 5 * time.Second, CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
			return http.ErrUseLastResponse
		}}
	}
	return &Reporter{
		capabilityCoreURL: capURL,
		authCoreURL:       authURL,
		serviceID:         strings.TrimSpace(cfg.ServiceID),
		credential:        strings.TrimSpace(cfg.Credential),
		interval:          interval,
		http:              client,
		probe:             cfg.Probe,
	}, nil
}

func validateURL(raw, name string) (string, error) {
	parsed, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || parsed.Host == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" ||
		(parsed.Scheme != "http" && parsed.Scheme != "https") {
		return "", fmt.Errorf("%s must be an absolute http(s) URL without credentials, query, or fragment", name)
	}
	return strings.TrimRight(parsed.String(), "/"), nil
}

// Start reports once now and then on every interval until ctx ends. It never returns an attestation error.
func (r *Reporter) Start(ctx context.Context, logf func(string, ...any)) {
	if logf == nil {
		logf = func(string, ...any) {}
	}
	for {
		if err := r.Attest(ctx); err != nil {
			logf("room canvas capability health not attested; the canvas capabilities stay unavailable: %v", err)
		}
		timer := time.NewTimer(r.interval)
		select {
		case <-ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
	}
}

// Attest runs the probe and, when it passes, attests every canvas capability with content-free readiness metadata. A failing
// probe stops before any token is minted.
func (r *Reporter) Attest(ctx context.Context) error {
	if err := r.probe(ctx); err != nil {
		return fmt.Errorf("canvas readiness probe failed: %w", err)
	}
	token, err := r.mint(ctx)
	if err != nil {
		return err
	}
	for _, capabilityID := range Capabilities {
		if err := r.attestOne(ctx, token, capabilityID); err != nil {
			return err
		}
	}
	return nil
}

func (r *Reporter) attestOne(ctx context.Context, token, capabilityID string) error {
	rowRequest, err := http.NewRequestWithContext(ctx, http.MethodGet, r.capabilityCoreURL+"/api/v1/capabilities/"+capabilityID, nil)
	if err != nil {
		return fmt.Errorf("build capability row request: %w", err)
	}
	rowRequest.Header.Set("Authorization", "Bearer "+token)
	rowResponse, err := r.http.Do(rowRequest)
	if err != nil {
		return fmt.Errorf("read capability row %s: %w", capabilityID, err)
	}
	rowBody, readErr := readBounded(rowResponse.Body)
	_ = rowResponse.Body.Close()
	if readErr != nil {
		return fmt.Errorf("read capability row response: %w", readErr)
	}
	if rowResponse.StatusCode != http.StatusOK {
		return fmt.Errorf("capability row %s read returned status %d", capabilityID, rowResponse.StatusCode)
	}
	var row capabilityRow
	if err := json.Unmarshal(rowBody, &row); err != nil || strings.TrimSpace(row.ID) != capabilityID || strings.TrimSpace(row.Version) == "" {
		return fmt.Errorf("capability row response for %s did not carry a usable version", capabilityID)
	}
	body, err := json.Marshal(attestation{
		ID:            capabilityID,
		Version:       row.Version,
		State:         "available",
		ReasonCode:    "runtime_healthy",
		Reason:        "wiki-store room canvas routes, Control decision keys and canvas store ready",
		ExecutionMode: "agentic",
		CostClass:     "bounded",
	})
	if err != nil {
		return fmt.Errorf("encode capability attestation: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, r.capabilityCoreURL+ownerHealthPath, bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build capability attestation request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+token)
	response, err := r.http.Do(request)
	if err != nil {
		return fmt.Errorf("post capability attestation: %w", err)
	}
	_, readErr = readBounded(response.Body)
	_ = response.Body.Close()
	if readErr != nil {
		return fmt.Errorf("read capability attestation response: %w", readErr)
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		// The body is not logged: only the status says why.
		return fmt.Errorf("capability attestation for %s returned status %d", capabilityID, response.StatusCode)
	}
	return nil
}

func (r *Reporter) mint(ctx context.Context) (string, error) {
	body, err := json.Marshal(tokenRequest{
		OrgID:  "global",
		Scopes: []string{readScope, ownerHealthScope},
		Reason: "wiki-store room canvas capability health attestation",
	})
	if err != nil {
		return "", fmt.Errorf("encode capability health token request: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, r.authCoreURL+"/api/capability-core/internal-token", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("build capability health token request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-service-id", r.serviceID)
	request.Header.Set("x-service-api-key", r.credential)
	response, err := r.http.Do(request)
	if err != nil {
		return "", fmt.Errorf("mint capability health token: %w", err)
	}
	responseBody, readErr := readBounded(response.Body)
	_ = response.Body.Close()
	if readErr != nil {
		return "", fmt.Errorf("read capability health token response: %w", readErr)
	}
	if response.StatusCode != http.StatusOK && response.StatusCode != http.StatusCreated {
		return "", fmt.Errorf("auth-core refused the capability health token with status %d", response.StatusCode)
	}
	var parsed tokenResponse
	if err := json.Unmarshal(responseBody, &parsed); err != nil || strings.TrimSpace(parsed.Token) == "" {
		return "", errors.New("auth-core returned no usable capability health token")
	}
	if parsed.Audience != "" && parsed.Audience != "capability-core" {
		return "", fmt.Errorf("auth-core returned unexpected token audience %q", parsed.Audience)
	}
	return strings.TrimSpace(parsed.Token), nil
}

func readBounded(reader io.Reader) ([]byte, error) {
	body, err := io.ReadAll(io.LimitReader(reader, maxResponseBytes+1))
	if err != nil {
		return nil, err
	}
	if len(body) > maxResponseBytes {
		return nil, errors.New("response exceeded size limit")
	}
	return body, nil
}

// CanvasProbe is the readiness probe of the canvas routes: the switch is on, at least one Control decision key is loaded, and the
// store answers ping.
func CanvasProbe(enabled bool, keyCount int, ping func(ctx context.Context) error) Probe {
	return func(ctx context.Context) error {
		if !enabled {
			return errors.New("WIKISTORE_SPACE_CANVAS is off")
		}
		if keyCount == 0 {
			return errors.New("no Control Space decision key is loaded")
		}
		if ping == nil {
			return errors.New("no canvas store")
		}
		pingCtx, cancel := context.WithTimeout(ctx, 3*time.Second)
		defer cancel()
		if err := ping(pingCtx); err != nil {
			return errors.New("the canvas store did not answer")
		}
		return nil
	}
}
