// Package planeclient provides narrow Studio clients for owner-plane contracts.
// It does not verify JWT signatures: destination services must independently
// verify signatures, organization policy and route scopes.
package planeclient

import (
	"errors"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

var ErrUnavailable = errors.New("owner plane unavailable")
var ErrInvalidInput = errors.New("invalid plane request")

// Unavailable deliberately excludes upstream response bodies and credential paths.
type Unavailable struct {
	Plane  string
	Reason string
}

func (e *Unavailable) Error() string         { return e.Plane + " unavailable: " + e.Reason }
func (e *Unavailable) Unwrap() error         { return ErrUnavailable }
func unavailable(plane, reason string) error { return &Unavailable{Plane: plane, Reason: reason} }

type Config struct {
	Core               string
	OrganizationID     string
	QuarryURL          string
	QuarryTokenFile    string
	ModelURL           string
	ModelTokenFile     string
	InferenceTokenFile string
}

func ConfigFromEnvironment(core string) Config {
	return Config{Core: core, OrganizationID: os.Getenv("STUDIO_ORGANIZATION_ID"), QuarryURL: os.Getenv("QUARRY_URL"), QuarryTokenFile: os.Getenv("QUARRY_BEARER_TOKEN_FILE"), ModelURL: os.Getenv("MODEL_GATEWAY_URL"), ModelTokenFile: os.Getenv("MODEL_BEARER_TOKEN_FILE"), InferenceTokenFile: os.Getenv("INFERENCE_BEARER_TOKEN_FILE")}
}

type Client struct {
	cfg  Config
	http *http.Client
}
type Connection struct {
	Configured bool   `json:"configured"`
	Status     string `json:"status"`
}
type Connections struct {
	Quarry Connection `json:"quarry"`
	Model  Connection `json:"model"`
}

// New creates an optional client. Partial credentials remain unavailable, not a
// fallback to another plane's bearer. The injected transport is for explicit tests.
func New(cfg Config, supplied *http.Client) (*Client, error) {
	if !regexp.MustCompile(`^[a-z][a-z0-9-]{1,63}$`).MatchString(cfg.Core) {
		return nil, errors.New("a fixed Studio core identity is required")
	}
	if cfg.QuarryURL != "" || cfg.ModelURL != "" {
		if strings.TrimSpace(cfg.OrganizationID) == "" || strings.TrimSpace(cfg.OrganizationID) != cfg.OrganizationID || len(cfg.OrganizationID) > 128 {
			return nil, errors.New("a fixed organization is required")
		}
	}
	if err := validateOrigin(cfg.QuarryURL, "quarry-edge"); err != nil {
		return nil, err
	}
	if err := validateOrigin(cfg.ModelURL, "model-gateway"); err != nil {
		return nil, err
	}
	if cfg.QuarryURL != "" && cfg.ModelURL != "" && strings.EqualFold(strings.TrimRight(cfg.QuarryURL, "/"), strings.TrimRight(cfg.ModelURL, "/")) {
		return nil, errors.New("Quarry and Model require distinct origins")
	}
	if cfg.QuarryURL == "" && cfg.QuarryTokenFile != "" || cfg.ModelURL == "" && (cfg.ModelTokenFile != "" || cfg.InferenceTokenFile != "") {
		return nil, errors.New("credentials require a fixed plane origin")
	}
	for _, p := range []string{cfg.QuarryTokenFile, cfg.ModelTokenFile, cfg.InferenceTokenFile} {
		if p != "" && (!filepath.IsAbs(p) || filepath.Clean(p) != p) {
			return nil, errors.New("bearer files require absolute clean paths")
		}
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	client := http.Client{Transport: transport}
	if supplied != nil {
		client = *supplied
		if tr, ok := client.Transport.(*http.Transport); ok {
			clone := tr.Clone()
			clone.Proxy = nil
			client.Transport = clone
		} else if client.Transport == nil {
			client.Transport = transport
		}
	}
	client.Timeout = 30 * time.Second
	client.Jar = nil
	client.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Client{cfg: cfg, http: &client}, nil
}
func validateOrigin(raw, localService string) error {
	if raw == "" {
		return nil
	}
	u, err := url.Parse(raw)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || u.Opaque != "" || (u.Path != "" && u.Path != "/") {
		return errors.New("plane URL must be a fixed origin")
	}
	local := u.Hostname() == "127.0.0.1" || u.Hostname() == "localhost" || u.Hostname() == localService
	if u.Scheme != "https" && !(u.Scheme == "http" && local) {
		return errors.New("plane origin requires HTTPS or an approved local service")
	}
	return nil
}

// Readiness inspects local deployment files only. Configured is NOT a verified
// connection, provider grant, model readiness or network-health assertion.
func (c *Client) Readiness() Connections {
	q, m := Connection{Status: "disabled"}, Connection{Status: "disabled"}
	if c != nil && c.cfg.QuarryURL != "" {
		q = Connection{Status: "missing_credential"}
		if _, err := c.bearer(c.cfg.QuarryTokenFile, "quarry", "map:read"); err == nil {
			q = Connection{Configured: true, Status: "configured"}
		}
	}
	if c != nil && c.cfg.ModelURL != "" {
		m = Connection{Status: "missing_credential"}
		if _, _, err := c.modelBearers(); err == nil {
			m = Connection{Configured: true, Status: "configured"}
		}
	}
	return Connections{Quarry: q, Model: m}
}
