// Package userauthz is documents-api's thin client for user-core's per-user
// authorization facade. It resolves the EXPLICIT resource grants a viewer holds
// (resource_grants), which documents-api unions with owner_id/visibility to
// enforce ownership at the source.
//
// Cross-plane rule: the grant store lives in user-core (Control Plane);
// documents-api (Data Plane) NEVER queries it directly — it asks over this HTTP
// facade, authenticated with an audience/scope-bound service credential.
package userauthz

import (
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// Client calls user-core's internal authz facade.
type Client struct {
	baseURL      string
	serviceToken string
	http         *http.Client
	now          func() time.Time
}

// New constructs a Client. baseURL is user-core's root (e.g. http://user-core:8080).
func New(baseURL, serviceToken string) *Client {
	return &Client{
		baseURL:      strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		serviceToken: strings.TrimSpace(serviceToken),
		http:         &http.Client{Timeout: 3 * time.Second},
		now:          time.Now,
	}
}

func (c *Client) signDelegation(req *http.Request, orgID, userID string) error {
	timestamp := c.now().UTC().Format(time.RFC3339)
	nonceBytes := make([]byte, 18)
	if _, err := rand.Read(nonceBytes); err != nil {
		return fmt.Errorf("generate delegation nonce: %w", err)
	}
	nonce := base64.RawURLEncoding.EncodeToString(nonceBytes)
	bodyDigest := sha256.Sum256(nil)
	digest := base64.RawURLEncoding.EncodeToString(bodyDigest[:])
	canonical := strings.Join([]string{
		"v2",
		"documents-api",
		"user-core",
		timestamp,
		req.Method,
		req.URL.RequestURI(),
		userID,
		orgID,
		"authz:visible",
		"document",
		"",
		"resolve explicit document grants",
		"true",
		nonce,
		digest,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(c.serviceToken))
	_, _ = mac.Write([]byte(canonical))
	req.Header.Set("X-User-Id", userID)
	req.Header.Set("X-Org-Id", orgID)
	req.Header.Set("X-Delegation-Version", "v2")
	req.Header.Set("X-Delegation-Nonce", nonce)
	req.Header.Set("X-Delegation-Timestamp", timestamp)
	req.Header.Set("X-Delegation-Operation", "authz:visible")
	req.Header.Set("X-Delegation-Resource-Type", "document")
	req.Header.Set("X-Delegation-Resource-Id", "")
	req.Header.Set("X-Delegation-Reason", "resolve explicit document grants")
	req.Header.Set("X-Delegation-ZDR", "true")
	req.Header.Set("X-Delegation-Body-SHA256", digest)
	req.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
	return nil
}

type visibleResponse struct {
	IDs    []string `json:"ids"`
	AllOrg bool     `json:"all_org"`
}

// ListVisibleDocuments returns the document ids explicitly granted to the viewer
// in the org. The caller fails closed for grant-only documents on an error;
// owner and org-visible content remain available.
func (c *Client) ListVisibleDocuments(ctx context.Context, orgID, userID, authorization string) ([]string, error) {
	if c == nil || c.baseURL == "" {
		return nil, fmt.Errorf("userauthz: client not configured")
	}
	if !strings.HasPrefix(authorization, "Bearer ") || strings.TrimSpace(strings.TrimPrefix(authorization, "Bearer ")) == "" || strings.ContainsAny(authorization, "\r\n") {
		return nil, fmt.Errorf("userauthz: verified user bearer required")
	}
	q := url.Values{}
	q.Set("org_id", orgID)
	q.Set("subject_id", userID)
	q.Set("resource_type", "document")
	reqURL := c.baseURL + "/api/v1/internal/authz/visible?" + q.Encode()

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, reqURL, nil)
	if err != nil {
		return nil, fmt.Errorf("userauthz: build request: %w", err)
	}
	if c.serviceToken != "" {
		req.Header.Set("Authorization", authorization)
		req.Header.Set("X-Service-Token", c.serviceToken)
		req.Header.Set("X-Service-Id", "documents-api")
		if err := c.signDelegation(req, orgID, userID); err != nil {
			return nil, fmt.Errorf("userauthz: sign request: %w", err)
		}
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("userauthz: request failed: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("userauthz: unexpected status %d", resp.StatusCode)
	}

	var out visibleResponse
	if err := json.NewDecoder(resp.Body).Decode(&out); err != nil {
		return nil, fmt.Errorf("userauthz: decode response: %w", err)
	}
	// NOTE(PR-5): out.AllOrg (admin super-visibility / org:data:read_all) is not
	// consumed yet — the MVP facade always returns false. When the admin bypass
	// lands, AllOrg=true must short-circuit the ownership predicate. It is
	// intentionally NOT part of default agent grounding.
	return out.IDs, nil
}
