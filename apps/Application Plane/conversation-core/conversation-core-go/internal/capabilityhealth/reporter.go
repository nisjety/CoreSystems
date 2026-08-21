// Package capabilityhealth reports Conversation Core's owner-action readiness
// to Capability Core. It is deliberately narrower than a process health check:
// only a fully configured, Control-bound ticket adapter may be attested.
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
	ticketCapabilityID = "cap.tool.ticket.create"
	ownerHealthScope   = "capability:owner-action:health:write"
	readScope          = "capability:read"
	defaultInterval    = 2 * time.Minute
	maxResponseBytes   = 64 << 10
)

// Config is the minimum service-to-service configuration needed to report
// owner-action readiness. The credential is exchanged for a short-lived
// capability-core JWT and is never sent to Capability Core.
type Config struct {
	CapabilityCoreURL string
	AuthCoreURL       string
	ServiceID         string
	Credential        string
	Interval          time.Duration
	HTTPClient        *http.Client
}

// Reporter is a detached, fail-closed readiness heartbeat. A failed mint,
// row read, or attestation leaves the durable capability unavailable; it never
// changes Conversation Core's serving status.
type Reporter struct {
	capabilityCoreURL string
	authCoreURL       string
	serviceID         string
	credential        string
	interval          time.Duration
	http              *http.Client
}

type tokenRequest struct {
	OrgID  string   `json:"orgId"`
	Scopes []string `json:"scopes"`
	Reason string   `json:"reason"`
}

type tokenResponse struct {
	Token            string `json:"token"`
	Audience         string `json:"audience"`
	ExpiresInSeconds int    `json:"expiresInSeconds"`
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

// New validates the complete optional block. Callers should treat an unset
// block as disabled; a partial block is an operator error and must not result
// in an anonymous or generic health write.
func New(cfg Config) (*Reporter, error) {
	values := []string{cfg.CapabilityCoreURL, cfg.AuthCoreURL, cfg.ServiceID, cfg.Credential}
	configured := 0
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			configured++
		}
	}
	if configured != len(values) {
		return nil, errors.New("capability health reporter requires Capability Core URL, Auth Core URL, service id, and service credential")
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
	}, nil
}

func validateURL(raw, name string) (string, error) {
	parsed, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || parsed.Host == "" || parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" || (parsed.Scheme != "http" && parsed.Scheme != "https") {
		return "", fmt.Errorf("%s must be an absolute http(s) URL without credentials, query, or fragment", name)
	}
	return strings.TrimRight(parsed.String(), "/"), nil
}

// Start performs one immediate report and then refreshes it. It never returns
// an attestation error: expiry in Capability Core is the fail-closed signal.
func (r *Reporter) Start(ctx context.Context, logf func(string, ...any)) {
	if logf == nil {
		logf = func(string, ...any) {}
	}
	for {
		if err := r.Attest(ctx); err != nil {
			logf("conversation capability health attestation failed; ticket capability remains unavailable: %v", err)
		} else {
			logf("conversation capability health attested: %s", ticketCapabilityID)
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

// Attest reads the current catalog version and posts only content-free
// readiness metadata. The route is owner-action-specific and the JWT carries
// both capability:read and capability:owner-action:health:write.
func (r *Reporter) Attest(ctx context.Context) error {
	token, err := r.mint(ctx)
	if err != nil {
		return err
	}
	rowRequest, err := http.NewRequestWithContext(ctx, http.MethodGet, r.capabilityCoreURL+"/api/v1/capabilities/"+ticketCapabilityID, nil)
	if err != nil {
		return fmt.Errorf("build capability row request: %w", err)
	}
	rowRequest.Header.Set("Authorization", "Bearer "+token)
	rowResponse, err := r.http.Do(rowRequest)
	if err != nil {
		return fmt.Errorf("read capability row: %w", err)
	}
	rowBody, readErr := readBounded(rowResponse.Body)
	_ = rowResponse.Body.Close()
	if readErr != nil {
		return fmt.Errorf("read capability row response: %w", readErr)
	}
	if rowResponse.StatusCode != http.StatusOK {
		return fmt.Errorf("capability row read returned status %d", rowResponse.StatusCode)
	}
	var row capabilityRow
	if err := json.Unmarshal(rowBody, &row); err != nil || strings.TrimSpace(row.ID) != ticketCapabilityID || strings.TrimSpace(row.Version) == "" {
		return errors.New("capability row response did not contain a usable ticket version")
	}
	body, err := json.Marshal(attestation{
		ID:            ticketCapabilityID,
		Version:       row.Version,
		State:         "available",
		ReasonCode:    "runtime_healthy",
		Reason:        "conversation-core owner-action adapter and Control-bound execution path ready",
		ExecutionMode: "agentic",
		CostClass:     "bounded",
	})
	if err != nil {
		return fmt.Errorf("encode capability attestation: %w", err)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, r.capabilityCoreURL+"/api/v1/capabilities/owner-actions/health", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("build capability attestation request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Authorization", "Bearer "+token)
	response, err := r.http.Do(request)
	if err != nil {
		return fmt.Errorf("post capability attestation: %w", err)
	}
	responseBody, readErr := readBounded(response.Body)
	_ = response.Body.Close()
	if readErr != nil {
		return fmt.Errorf("read capability attestation response: %w", readErr)
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		return fmt.Errorf("capability attestation returned status %d: %s", response.StatusCode, strings.TrimSpace(string(responseBody)))
	}
	return nil
}

func (r *Reporter) mint(ctx context.Context) (string, error) {
	body, err := json.Marshal(tokenRequest{
		OrgID:  "global",
		Scopes: []string{readScope, ownerHealthScope},
		Reason: "Conversation Core owner-action capability health attestation",
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
		return "", fmt.Errorf("auth-core refused capability health token with status %d", response.StatusCode)
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
