// Package orgcore reads an organisation's own facts from the Control Plane's org-core. visibility-core uses
// it to know which website is the organisation's primary site. The credential holds the org:website:read
// scope and nothing else: org-core answers with the website it holds for the organisation, and nothing more.
package orgcore

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// Config binds the client to one organisation and one service credential.
type Config struct {
	URL            string
	ServiceID      string
	Token          string
	OrganizationID string
}

const (
	defaultServiceID = "visibility-core"
	minTokenLength   = 32
	maxTokenLength   = 512
	maxBody          = 16 << 10
	requestTimeout   = 5 * time.Second
)

var (
	// ErrRefused means org-core does not accept the service identity or its token.
	ErrRefused = errors.New("orgcore: org-core does not accept this service credential")
	// ErrUnavailable means the organisation's record could not be read.
	ErrUnavailable = errors.New("orgcore: the organisation record could not be read")
	// ErrWrongOrganization means a read was asked for an organisation the client is not bound to.
	ErrWrongOrganization = errors.New("orgcore: the client is bound to another organisation")
)

// Client reads one organisation's website from org-core.
type Client struct {
	cfg  Config
	http *http.Client
}

// NewClient checks the configuration. Without a URL and a token there is no org-core to read, and it returns
// nil: the registry then runs without a primary site. Anything partly configured is an error.
func NewClient(cfg Config, client *http.Client) (*Client, error) {
	if cfg.URL == "" && cfg.Token == "" {
		return nil, nil
	}
	if cfg.ServiceID == "" {
		cfg.ServiceID = defaultServiceID
	}
	u, err := url.Parse(cfg.URL)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || (u.Path != "" && u.Path != "/") {
		return nil, errors.New("org-core requires a fixed origin")
	}
	if u.Scheme != "https" && !(u.Scheme == "http" && localOrigin(u.Host)) {
		return nil, errors.New("org-core requires HTTPS or a local origin")
	}
	if cfg.OrganizationID == "" {
		return nil, errors.New("org-core requires a fixed organization")
	}
	if len(cfg.Token) < minTokenLength || len(cfg.Token) > maxTokenLength || strings.ContainsAny(cfg.Token, " \t\r\n") {
		return nil, errors.New("org-core requires a service token of 32 to 512 characters without spaces")
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	safe := http.Client{Transport: transport}
	if client != nil {
		safe = *client
	}
	safe.Timeout = requestTimeout
	safe.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Client{cfg: cfg, http: &safe}, nil
}

// localOrigin is the org-core a Studio deployment reaches on the inter-plane bus, or a loopback address for
// local development.
func localOrigin(host string) bool {
	if host == "org-core-service:8080" {
		return true
	}
	h := host
	if i := strings.LastIndex(host, ":"); i >= 0 {
		h = host[:i]
	}
	return h == "127.0.0.1" || h == "localhost"
}

// PrimarySite returns the website org-core holds for the organisation, or "" when it holds none.
func (c *Client) PrimarySite(ctx context.Context, orgID string) (string, error) {
	if orgID != c.cfg.OrganizationID {
		return "", ErrWrongOrganization
	}
	endpoint := strings.TrimRight(c.cfg.URL, "/") + "/api/v1/organizations/" + url.PathEscape(orgID) + "/website"
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return "", ErrUnavailable
	}
	request.Header.Set("Accept", "application/json")
	request.Header.Set("X-Service-Id", c.cfg.ServiceID)
	request.Header.Set("X-Service-Token", c.cfg.Token)
	response, err := c.http.Do(request)
	if err != nil {
		return "", ErrUnavailable
	}
	defer response.Body.Close()
	switch response.StatusCode {
	case http.StatusOK:
	case http.StatusNotFound:
		return "", nil
	case http.StatusUnauthorized, http.StatusForbidden:
		return "", ErrRefused
	default:
		return "", ErrUnavailable
	}
	var body struct {
		WebsiteURL *string `json:"website_url"`
	}
	if err := json.NewDecoder(io.LimitReader(response.Body, maxBody)).Decode(&body); err != nil {
		return "", ErrUnavailable
	}
	if body.WebsiteURL == nil {
		return "", nil
	}
	return strings.TrimSpace(*body.WebsiteURL), nil
}
