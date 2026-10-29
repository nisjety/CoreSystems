// Package orgcore looks up Norwegian organisations through the Control Plane's org-core, which
// owns organisations and holds the platform's one Brønnøysund (Enhetsregisteret) client. anbud-core
// does not call Brønnøysund itself: one owner, one place to cache and rate-limit, and no second
// copy of the register's terms to track.
//
// The call is GET /api/v1/brreg/{orgnr} on org-core, authenticated as a service principal with the
// scope org:brreg:read. That scope is not ":self", so org-core asks for no signed user delegation,
// and this client sends no user identity. The principal and its token must be registered in
// org-core's ORG_CORE_SERVICE_CREDENTIALS; see docs/REQUIREMENTS.md.
//
// Only what a tender profile needs is kept. The register's e-mail address, telephone number and
// street addresses are never decoded: for a sole trader they are a person's.
package orgcore

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/secret"
)

const (
	// DefaultPrincipal is the service principal anbud-core presents to org-core.
	DefaultPrincipal = "anbud-core"

	lookupPath     = "/api/v1/brreg/"
	defaultTimeout = 10 * time.Second
	// maxBody bounds a response. One Enhetsregisteret record is a few kilobytes.
	maxBody = 1 << 20
	// Token bounds, as org-core checks them for a registered credential.
	minTokenLength = 32
	maxTokenLength = 512
)

var (
	// ErrNotConfigured means the org-core URL or the service token is not set. The service runs
	// without it and reports the lookup as not configured.
	ErrNotConfigured = errors.New("orgcore: org-core URL or service token is not configured")
	// ErrInvalidConfig means the configuration is set but unusable.
	ErrInvalidConfig = errors.New("orgcore: invalid configuration")
	// ErrInvalidOrgNumber means the number is not a valid Norwegian organisation number. No call
	// is made for it.
	ErrInvalidOrgNumber = errors.New("orgcore: not a valid organisation number")
	// ErrNotFound means Enhetsregisteret has no such organisation, or it was deleted (org-core 404).
	ErrNotFound = errors.New("orgcore: organisation not found in Enhetsregisteret")
	// ErrUnauthorized means org-core does not accept the principal or token (401).
	ErrUnauthorized = errors.New("orgcore: service credential rejected")
	// ErrScopeDenied means the credential lacks org:brreg:read (403).
	ErrScopeDenied = errors.New("orgcore: service scope org:brreg:read not granted")
	// ErrUpstream means org-core could not reach Enhetsregisteret (502).
	ErrUpstream = errors.New("orgcore: Enhetsregisteret unavailable behind org-core")
	// ErrUnavailable means org-core's service authentication is not set up (503).
	ErrUnavailable = errors.New("orgcore: org-core service authentication unavailable")
	// ErrUnexpectedStatus is any other status, a redirect included.
	ErrUnexpectedStatus = errors.New("orgcore: unexpected response status")
	// ErrBadResponse means the body is too large or is not the record shape.
	ErrBadResponse = errors.New("orgcore: unreadable response")
	// ErrMismatch means the record is for another organisation number than the one asked for.
	ErrMismatch = errors.New("orgcore: response is for another organisation number")
)

// Config configures a Client.
type Config struct {
	// BaseURL is org-core's base URL, for example http://org-core:8080. No path, query or
	// credentials.
	BaseURL string
	// Token is the service token registered for Principal in org-core.
	Token secret.Value
	// Principal defaults to DefaultPrincipal.
	Principal string
	// HTTPClient is optional; redirects are always refused on a copy of it.
	HTTPClient *http.Client
}

// Organisation is what a tender profile takes from Enhetsregisteret.
type Organisation struct {
	OrgNumber          string
	Name               string
	OrgForm            string // organisasjonsform code, such as AS, ENK, ORGL
	NACE               string // the first næringskode, such as 84.110
	NACEText           string
	Municipality       string // of the business address
	MunicipalityNumber string
	Employees          *int // nil when the register states none
	Bankrupt           bool
	UnderLiquidation   bool
	ForcedLiquidation  bool
}

// CanBid reports whether the register shows nothing that stops the organisation tendering: it is
// not bankrupt and not being wound up. It is a gate, not a qualification check.
func (o Organisation) CanBid() bool {
	return !o.Bankrupt && !o.UnderLiquidation && !o.ForcedLiquidation
}

// Client calls org-core. It is safe for concurrent use.
type Client struct {
	base      *url.URL
	token     secret.Value
	principal string
	http      *http.Client
}

// New validates cfg and builds a Client.
func New(cfg Config) (*Client, error) {
	token := strings.TrimSpace(cfg.Token.Reveal())
	rawURL := strings.TrimSpace(cfg.BaseURL)
	if token == "" || rawURL == "" {
		return nil, ErrNotConfigured
	}
	if !acceptableToken(token) {
		return nil, fmt.Errorf("%w: the service token must be %d-%d characters and not a placeholder", ErrInvalidConfig, minTokenLength, maxTokenLength)
	}
	base, err := url.Parse(rawURL)
	if err != nil || (base.Scheme != "http" && base.Scheme != "https") || base.Host == "" ||
		base.User != nil || base.RawQuery != "" || base.Fragment != "" || strings.Trim(base.Path, "/") != "" {
		// The URL is not echoed: it could carry credentials.
		return nil, fmt.Errorf("%w: ORG_CORE_URL must be an http(s) origin with no path, query or credentials", ErrInvalidConfig)
	}
	base.Path = ""
	principal := strings.TrimSpace(cfg.Principal)
	if principal == "" {
		principal = DefaultPrincipal
	}
	if !validPrincipal(principal) {
		return nil, fmt.Errorf("%w: the principal must be 1-128 of letters, digits and ._:-", ErrInvalidConfig)
	}

	hc := &http.Client{Timeout: defaultTimeout}
	if cfg.HTTPClient != nil {
		copied := *cfg.HTTPClient
		hc = &copied
	}
	// The token is a custom header, which net/http forwards to a redirect target. org-core
	// never redirects this route, so every redirect is refused.
	hc.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }

	return &Client{base: base, token: secret.Value(token), principal: principal, http: hc}, nil
}

// String never shows the token.
func (c *Client) String() string {
	return fmt.Sprintf("orgcore.Client{base: %s, principal: %s, token: %s}", c.base.Redacted(), c.principal, c.token)
}

// GoString never shows the token.
func (c *Client) GoString() string { return c.String() }

// Format renders the same text for every verb, so no verb can reach the token.
func (c *Client) Format(f fmt.State, _ rune) { _, _ = io.WriteString(f, c.String()) }

// Lookup returns the organisation with the given number. The number is normalised and its check
// digit verified first; an invalid one is refused without a call, so nothing but nine digits ever
// reaches org-core's path.
func (c *Client) Lookup(ctx context.Context, orgNumber string) (Organisation, error) {
	number, err := normalize.OrgNumber(orgNumber)
	if err != nil {
		return Organisation{}, ErrInvalidOrgNumber
	}
	endpoint := c.base.JoinPath(lookupPath, number)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return Organisation{}, fmt.Errorf("orgcore: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("X-Service-Id", c.principal)
	req.Header.Set("X-Service-Token", c.token.Reveal())

	resp, err := c.http.Do(req)
	if err != nil {
		// net/http quotes the whole URL in a transport error, and the URL holds the organisation
		// number and org-core's address. The cause alone says what went wrong.
		var urlErr *url.Error
		if errors.As(err, &urlErr) {
			err = urlErr.Err
		}
		return Organisation{}, fmt.Errorf("orgcore: lookup request failed: %w", err)
	}
	defer resp.Body.Close()

	// The body of an error is never read into an error: it is org-core's, and could echo input.
	if err := statusError(resp.StatusCode); err != nil {
		return Organisation{}, err
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBody+1))
	if err != nil {
		return Organisation{}, fmt.Errorf("%w: read: %v", ErrBadResponse, err)
	}
	if len(body) > maxBody {
		return Organisation{}, fmt.Errorf("%w: over %d bytes", ErrBadResponse, maxBody)
	}
	var record enhet
	if err := json.Unmarshal(body, &record); err != nil {
		return Organisation{}, fmt.Errorf("%w: not an Enhetsregisteret record", ErrBadResponse)
	}
	if record.Organisasjonsnummer != number {
		return Organisation{}, ErrMismatch
	}
	return record.organisation(), nil
}

func statusError(status int) error {
	switch {
	case status == http.StatusOK:
		return nil
	case status == http.StatusNotFound:
		return ErrNotFound
	case status == http.StatusUnauthorized:
		return ErrUnauthorized
	case status == http.StatusForbidden:
		return ErrScopeDenied
	case status == http.StatusBadGateway:
		return ErrUpstream
	case status == http.StatusServiceUnavailable:
		return ErrUnavailable
	}
	return fmt.Errorf("%w: %d", ErrUnexpectedStatus, status)
}

// enhet is the part of an Enhetsregisteret record that is decoded. Contact details and street
// addresses are deliberately absent, so they are never held.
type enhet struct {
	Organisasjonsnummer string `json:"organisasjonsnummer"`
	Navn                string `json:"navn"`
	Organisasjonsform   struct {
		Kode string `json:"kode"`
	} `json:"organisasjonsform"`
	Forretningsadresse *struct {
		Kommune       string `json:"kommune"`
		Kommunenummer string `json:"kommunenummer"`
	} `json:"forretningsadresse"`
	Naeringskode1 *struct {
		Kode        string `json:"kode"`
		Beskrivelse string `json:"beskrivelse"`
	} `json:"naeringskode1"`
	AntallAnsatte   *int `json:"antallAnsatte"`
	Konkurs         bool `json:"konkurs"`
	UnderAvvikling  bool `json:"underAvvikling"`
	Tvangsavvikling bool `json:"underTvangsavviklingEllerTvangsopplosning"`
}

func (e enhet) organisation() Organisation {
	o := Organisation{
		OrgNumber:         e.Organisasjonsnummer,
		Name:              e.Navn,
		OrgForm:           e.Organisasjonsform.Kode,
		Employees:         e.AntallAnsatte,
		Bankrupt:          e.Konkurs,
		UnderLiquidation:  e.UnderAvvikling,
		ForcedLiquidation: e.Tvangsavvikling,
	}
	if e.Forretningsadresse != nil {
		o.Municipality = e.Forretningsadresse.Kommune
		o.MunicipalityNumber = e.Forretningsadresse.Kommunenummer
	}
	if e.Naeringskode1 != nil {
		o.NACE = e.Naeringskode1.Kode
		o.NACEText = e.Naeringskode1.Beskrivelse
	}
	return o
}

// acceptableToken mirrors org-core's own check of a registered token, so a token org-core would
// refuse fails here, at start-up, with a clear message.
func acceptableToken(token string) bool {
	if len(token) < minTokenLength || len(token) > maxTokenLength {
		return false
	}
	lower := strings.ToLower(token)
	for _, prefix := range []string{"test", "placeholder", "change-me", "replace-with"} {
		if strings.HasPrefix(lower, prefix) {
			return false
		}
	}
	return true
}

// validPrincipal mirrors org-core's rule for a principal name.
func validPrincipal(value string) bool {
	if len(value) < 1 || len(value) > 128 {
		return false
	}
	for _, r := range value {
		if (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || strings.ContainsRune("._:-", r) {
			continue
		}
		return false
	}
	return true
}
