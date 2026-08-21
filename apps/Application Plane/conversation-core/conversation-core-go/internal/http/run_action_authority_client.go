package http

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net"
	stdhttp "net/http"
	"net/url"
	"strings"
	"time"
)

const controlRunActionAuthorityPath = "/api/v1/internal/spaces/run-action-authority-check"

// ControlRunActionAuthorityValidator performs the final, non-secret Control
// authority read for a signed run decision. It sends only decision claims that
// Conversation Core has already signature-verified; the bearer itself is
// never forwarded, persisted, or exposed to a browser.
type ControlRunActionAuthorityValidator struct {
	endpoint     *url.URL
	serviceToken string
	client       *stdhttp.Client
}

// NewControlRunActionAuthorityValidator requires authenticated transport for
// the credentialed Control authority check. The sole exception is an explicit
// development opt-in for an IP loopback endpoint; Docker service names and all
// remote/private-network hosts still require HTTPS.
func NewControlRunActionAuthorityValidator(baseURL, serviceToken string, allowInsecureLoopback bool) (*ControlRunActionAuthorityValidator, error) {
	parsed, err := url.Parse(strings.TrimSpace(baseURL))
	if err != nil || parsed.Scheme == "" || parsed.Host == "" || (parsed.Scheme != "http" && parsed.Scheme != "https") ||
		parsed.User != nil || parsed.RawQuery != "" || parsed.Fragment != "" {
		return nil, fmt.Errorf("Control run action authority URL is invalid")
	}
	if parsed.Scheme == "http" && (!allowInsecureLoopback || !isIPLoopback(parsed.Hostname())) {
		return nil, fmt.Errorf("Control run action authority must use HTTPS; plaintext is permitted only for an explicitly enabled IP-loopback development endpoint")
	}
	serviceToken = strings.TrimSpace(serviceToken)
	if len(serviceToken) < 32 {
		return nil, fmt.Errorf("Control run action authority service token is invalid")
	}
	parsed.Path = strings.TrimRight(parsed.Path, "/") + controlRunActionAuthorityPath
	return &ControlRunActionAuthorityValidator{
		endpoint:     parsed,
		serviceToken: serviceToken,
		client: &stdhttp.Client{
			Timeout: 5 * time.Second,
			CheckRedirect: func(_ *stdhttp.Request, _ []*stdhttp.Request) error {
				return stdhttp.ErrUseLastResponse
			},
		},
	}, nil
}

func isIPLoopback(host string) bool {
	ip := net.ParseIP(strings.TrimSpace(host))
	return ip != nil && ip.IsLoopback()
}

func (v *ControlRunActionAuthorityValidator) ValidateRunActionAuthority(ctx context.Context, decision runActionDecision) error {
	if v == nil || v.endpoint == nil || v.client == nil || len(v.serviceToken) < 32 {
		return fmt.Errorf("Control run action authority validator is unavailable")
	}
	payload, err := json.Marshal(decision)
	if err != nil {
		return fmt.Errorf("encode Control run action authority check: %w", err)
	}
	request, err := stdhttp.NewRequestWithContext(ctx, stdhttp.MethodPost, v.endpoint.String(), bytes.NewReader(payload))
	if err != nil {
		return fmt.Errorf("create Control run action authority request: %w", err)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", "conversation-core")
	request.Header.Set("X-Service-Token", v.serviceToken)
	response, err := v.client.Do(request)
	if err != nil {
		return fmt.Errorf("Control run action authority request failed: %w", err)
	}
	defer response.Body.Close()
	if response.StatusCode == stdhttp.StatusForbidden {
		return ErrRunActionAuthorityDenied
	}
	if response.StatusCode != stdhttp.StatusOK {
		return fmt.Errorf("Control run action authority is unavailable")
	}
	var envelope struct {
		Data struct {
			Authorized bool `json:"authorized"`
		} `json:"data"`
	}
	decoder := json.NewDecoder(io.LimitReader(response.Body, 4*1024))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&envelope); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return fmt.Errorf("Control run action authority response is invalid")
	}
	if !envelope.Data.Authorized {
		return ErrRunActionAuthorityDenied
	}
	return nil
}
