// Package siteintegrations verifies website bindings through Integration Core.
// Provider credentials remain in Integration Core; connection IDs stay server-side.
package siteintegrations

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type Config struct {
	IntegrationCoreURL, BearerTokenFile, PreferredGoogleConnectionID string
	GoogleConsentReturnURL, GoogleOAuthCallbackURL                   string
}
type Mapping struct {
	Status        string `json:"status"`
	ConnectionID  string `json:"connectionId,omitempty"`
	Property      string `json:"property,omitempty"`
	AccessLevel   string `json:"accessLevel,omitempty"`
	PropertyID    string `json:"propertyId,omitempty"`
	StreamID      string `json:"streamId,omitempty"`
	MeasurementID string `json:"measurementId,omitempty"`
	ReportingSite string `json:"reportingSite,omitempty"`
	CheckedAt     string `json:"checkedAt"`
}
type Evidence struct {
	SearchConsole          Mapping
	GA4                    Mapping
	ConnectionsFingerprint string
}
type Broker struct {
	cfg    Config
	client *http.Client
}
type connection struct {
	ID           string   `json:"id"`
	ProviderKey  string   `json:"providerKey"`
	Status       string   `json:"status"`
	Capabilities []string `json:"capabilities"`
	Scopes       []string `json:"scopes"`
	AccountID    string   `json:"providerAccountId"`
}

const analyticsScope = "https://www.googleapis.com/auth/analytics.readonly"

var errUnavailable = errors.New("integration discovery unavailable")

func New(cfg Config) *Broker {
	return &Broker{cfg: cfg, client: &http.Client{Timeout: 8 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

func (b *Broker) Discover(ctx context.Context, site string) Evidence {
	checked := time.Now().UTC().Format(time.RFC3339)
	state := func(gsc, ga4 string) Evidence {
		return Evidence{SearchConsole: Mapping{Status: gsc, CheckedAt: checked}, GA4: Mapping{Status: ga4, CheckedAt: checked}}
	}
	canonical, err := pulse.CanonicalSite(site)
	if err != nil || canonical != site {
		return state("source_unavailable", "source_unavailable")
	}
	if b.cfg.IntegrationCoreURL == "" || b.cfg.BearerTokenFile == "" {
		return state("needs_connection", "requires_consent")
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			Connections []connection `json:"connections"`
		} `json:"data"`
	}
	if b.request(ctx, "GET", "/api/v1/connections?providerKey=google", nil, &envelope) != nil || !envelope.Success || len(envelope.Data.Connections) > 20 {
		return state("source_unavailable", "source_unavailable")
	}
	connections := []connection{}
	for _, item := range envelope.Data.Connections {
		if item.ProviderKey == "google" && item.Status == "active" && validID(item.ID) {
			connections = append(connections, item)
		}
	}
	sort.Slice(connections, func(i, j int) bool {
		if (connections[i].ID == b.cfg.PreferredGoogleConnectionID) != (connections[j].ID == b.cfg.PreferredGoogleConnectionID) {
			return connections[i].ID == b.cfg.PreferredGoogleConnectionID
		}
		return connections[i].ID < connections[j].ID
	})
	evidence := state("needs_connection", "requires_consent")
	evidence.ConnectionsFingerprint = connectionFingerprint(envelope.Data.Connections)
	// Each independent provider fails honestly without suppressing the other's result.
	gsc := make(chan Mapping, 1)
	ga4 := make(chan Mapping, 1)
	go func() { gsc <- b.discoverSearchConsole(ctx, connections, site, checked) }()
	go func() { ga4 <- b.discoverAnalytics(ctx, connections, site, checked) }()
	evidence.SearchConsole = <-gsc
	evidence.GA4 = <-ga4
	return evidence
}

func (b *Broker) request(ctx context.Context, method, path string, body any, result any) error {
	base, err := url.Parse(b.cfg.IntegrationCoreURL)
	if err != nil || base.User != nil || base.Host == "" || base.Path != "" && base.Path != "/" || base.RawQuery != "" || base.Fragment != "" || base.Scheme != "https" && base.Scheme != "http" {
		return errUnavailable
	}
	if base.Scheme == "http" && !has([]string{"localhost", "127.0.0.1", "integration-api", "integration-core"}, base.Hostname()) {
		return errUnavailable
	}
	info, err := os.Lstat(b.cfg.BearerTokenFile)
	if err != nil || !info.Mode().IsRegular() || info.Size() < 1 || info.Size() > 8192 {
		return errUnavailable
	}
	data, err := os.ReadFile(b.cfg.BearerTokenFile)
	token := strings.TrimSpace(string(data))
	if err != nil || len(data) > 8192 || token == "" || strings.ContainsAny(token, " \r\n\t") {
		return errUnavailable
	}
	var input io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return errUnavailable
		}
		input = bytes.NewReader(encoded)
	}
	req, err := http.NewRequestWithContext(ctx, method, strings.TrimRight(b.cfg.IntegrationCoreURL, "/")+path, input)
	if err != nil {
		return errUnavailable
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Accept", "application/json")
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	response, err := b.client.Do(req)
	if err != nil {
		return errUnavailable
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return errUnavailable
	}
	payload, err := io.ReadAll(io.LimitReader(response.Body, (2<<20)+1))
	if err != nil || len(payload) > 2<<20 || json.Unmarshal(payload, result) != nil {
		return errUnavailable
	}
	return nil
}

func (b *Broker) action(ctx context.Context, id, operation string, params map[string]any, result any) error {
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			Action struct {
				ProviderKey, Operation string
				Result                 json.RawMessage
			} `json:"action"`
		} `json:"data"`
	}
	if !validID(id) || b.request(ctx, "POST", "/api/v1/connections/"+id+"/actions", map[string]any{"operation": operation, "params": params}, &envelope) != nil || !envelope.Success || envelope.Data.Action.ProviderKey != "google" || envelope.Data.Action.Operation != operation || json.Unmarshal(envelope.Data.Action.Result, result) != nil {
		return errUnavailable
	}
	return nil
}

func validID(value string) bool {
	if len(value) < 1 || len(value) > 128 {
		return false
	}
	for _, char := range value {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '_' || char == '-') {
			return false
		}
	}
	return true
}
func has(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}
func authorized(item connection, capability string, scopes ...string) bool {
	if !has(item.Capabilities, capability) {
		return false
	}
	for _, scope := range scopes {
		if has(item.Scopes, scope) {
			return true
		}
	}
	return false
}
