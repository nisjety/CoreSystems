// Package bearerrefresh renews Studio's bounded Auth Core service bearers.
// It never handles Google OAuth credentials or provider access tokens.
package bearerrefresh

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

const defaultServiceID = "marketing-core"

var serviceIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$`)

type Config struct {
	AuthCoreURL    string
	OrganizationID string
	// ServiceID is deployment-owned, never derived from a browser request.
	// Empty preserves the existing marketing-core principal.
	ServiceID      string
	ServiceKeyFile string
	TokenFile      string
	// Capability selects a fixed least-privilege issuance policy. Empty
	// preserves the existing Integration read-only bearer contract.
	Capability string
}

// State contains only expiry metadata. Readiness must be checked against the
// current time; a previously successful renewal does not imply a valid token.
type State struct {
	RefreshedAt time.Time `json:"refreshedAt"`
	ExpiresAt   time.Time `json:"expiresAt"`
}

type Refresher struct {
	cfg     Config
	profile capabilityProfile
	client  *http.Client
	now     func() time.Time
	wait    func(context.Context, time.Duration) error
}

func New(cfg Config, client *http.Client, now func() time.Time) (*Refresher, error) {
	if cfg.ServiceID == "" {
		cfg.ServiceID = defaultServiceID
	}
	if cfg.Capability == "" {
		cfg.Capability = defaultCapability
	}
	if err := validateConfig(cfg); err != nil {
		return nil, err
	}
	if now == nil {
		now = time.Now
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	localClient := http.Client{Transport: transport}
	if client != nil {
		localClient = *client
	}
	localClient.Timeout = 10 * time.Second
	localClient.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	profile, _ := profileFor(cfg.Capability) // validated before constructing the client
	return &Refresher{cfg: cfg, profile: profile, client: &localClient, now: now, wait: waitContext}, nil
}

func validateConfig(cfg Config) error {
	if _, err := profileFor(cfg.Capability); err != nil {
		return err
	}
	if !serviceIDPattern.MatchString(cfg.ServiceID) {
		return errors.New("MARKETING_INTEGRATION_SERVICE_ID must be a 1–128 character ASCII principal identifier")
	}
	endpoint, err := url.Parse(cfg.AuthCoreURL)
	if err != nil || endpoint.Host == "" || endpoint.User != nil || endpoint.RawQuery != "" || endpoint.Fragment != "" || (endpoint.Path != "" && endpoint.Path != "/") {
		return errors.New("AUTH_CORE_URL must be an origin without credentials, path, query, or fragment")
	}
	local := endpoint.Host == "auth-core:3011" || endpoint.Host == "localhost:3011" || endpoint.Host == "127.0.0.1:3011"
	if endpoint.Scheme != "https" && !(endpoint.Scheme == "http" && local) {
		return errors.New("AUTH_CORE_URL requires HTTPS or the approved local Auth Core origin on port 3011")
	}
	if cfg.OrganizationID == "" || len(cfg.OrganizationID) > 200 || strings.ContainsAny(cfg.OrganizationID, " \r\n\t") {
		return errors.New("MARKETING_INTEGRATION_ORGANIZATION_ID is required and must be a valid organization ID")
	}
	if !filepath.IsAbs(cfg.TokenFile) || filepath.Clean(cfg.TokenFile) != cfg.TokenFile || cfg.TokenFile == cfg.ServiceKeyFile || cfg.TokenFile+".status.json" == cfg.ServiceKeyFile {
		return errors.New("INTEGRATION_BEARER_TOKEN_FILE must be a distinct normalized absolute path")
	}
	if err := privateDirectory(filepath.Dir(cfg.TokenFile)); err != nil {
		return err
	}
	_, err = readPrivate(cfg.ServiceKeyFile, 8192)
	return err
}

// Refresh obtains a new bearer through Auth's audited issuance endpoint. Error
// text is intentionally fixed so responses and credentials never enter logs.
func (r *Refresher) Refresh(ctx context.Context) (State, error) {
	key, err := readPrivate(r.cfg.ServiceKeyFile, 8192)
	if err != nil {
		return State{}, err
	}
	key = bytes.TrimSpace(key)
	if len(key) < 16 || bytes.ContainsAny(key, " \r\n\t") {
		return State{}, errors.New("service key file content is invalid")
	}
	body, _ := json.Marshal(struct {
		OrgID  string   `json:"orgId"`
		Scopes []string `json:"scopes"`
		Reason string   `json:"reason"`
	}{r.cfg.OrganizationID, r.profile.scopes, r.profile.reason})
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(r.cfg.AuthCoreURL, "/")+r.profile.path, bytes.NewReader(body))
	if err != nil {
		return State{}, errors.New("could not construct Auth Core token request")
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("x-service-id", r.cfg.ServiceID)
	request.Header.Set("x-service-api-key", string(key))
	response, err := r.client.Do(request)
	if err != nil {
		return State{}, errors.New("Auth Core token request failed")
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK && response.StatusCode != http.StatusCreated {
		return State{}, errors.New("Auth Core refused service token issuance")
	}
	payload, err := io.ReadAll(io.LimitReader(response.Body, 16385))
	if err != nil || len(payload) > 16384 {
		return State{}, errors.New("Auth Core token response is unreadable or too large")
	}
	var bundle struct {
		Token     string    `json:"token"`
		ExpiresAt time.Time `json:"expiresAt"`
		Audience  string    `json:"audience"`
		Issuer    string    `json:"issuer"`
	}
	if err := json.Unmarshal(payload, &bundle); err != nil || bundle.Audience != r.profile.audience {
		return State{}, errors.New("Auth Core token response is invalid")
	}
	now := r.now().UTC()
	expiry, issuer, err := tokenExpiryForProfile(bundle.Token, r.cfg.OrganizationID, r.cfg.ServiceID, now, r.profile)
	if err != nil || expiry.Sub(now) < 45*time.Second || !expiry.Equal(bundle.ExpiresAt) || issuer != bundle.Issuer {
		return State{}, errors.New("Auth Core returned an invalid or insufficiently fresh service token")
	}
	state := State{RefreshedAt: now, ExpiresAt: expiry}
	if err := atomicPrivateWrite(r.cfg.TokenFile, []byte(bundle.Token)); err != nil {
		return State{}, err
	}
	metadata, _ := json.Marshal(state)
	if err := atomicPrivateWrite(r.cfg.TokenFile+".status.json", metadata); err != nil {
		return State{}, err
	}
	return state, nil
}

// Check checks expiry and binding of the file issued through the trusted Auth
// transport. The receiving plane performs cryptographic JWT verification.
func (r *Refresher) Check() (State, error) {
	token, err := readPrivate(r.cfg.TokenFile, 8192)
	if err != nil {
		return State{}, err
	}
	expiry, _, err := tokenExpiryForProfile(string(token), r.cfg.OrganizationID, r.cfg.ServiceID, r.now().UTC(), r.profile)
	if err == nil && expiry.Sub(r.now()) < 15*time.Second {
		err = errors.New("service bearer is too close to expiry")
	}
	return State{ExpiresAt: expiry}, err
}

// Run renews before expiry and fails after five consecutive unsuccessful
// attempts. Deployments can restart it; expired files never pass Check.
func (r *Refresher) Run(ctx context.Context, report func(State, error)) error {
	failures := 0
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		state, err := r.Refresh(ctx)
		if report != nil {
			report(state, err)
		}
		delay := time.Second
		if err != nil {
			failures++
			if failures >= 5 {
				return errors.New("Auth Core bearer renewal failed after bounded retries")
			}
			delay *= time.Duration(1 << (failures - 1))
		} else {
			failures = 0
			delay = state.ExpiresAt.Sub(r.now()) * 4 / 5
			// JWT expiry is wall time. A long monotonic timer can remain asleep
			// past expiry after the local VM resumes or its clock is corrected.
			if err := r.waitUntil(ctx, r.now().Add(delay).Round(0)); err != nil {
				return err
			}
			continue
		}
		if err := r.wait(ctx, delay); err != nil {
			return err
		}
	}
}

func (r *Refresher) waitUntil(ctx context.Context, deadline time.Time) error {
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		remaining := deadline.Sub(r.now())
		if remaining <= 0 {
			return nil
		}
		if remaining > 30*time.Second {
			remaining = 30 * time.Second
		}
		if err := r.wait(ctx, remaining); err != nil {
			return err
		}
	}
}

func waitContext(ctx context.Context, delay time.Duration) error {
	timer := time.NewTimer(delay)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}
