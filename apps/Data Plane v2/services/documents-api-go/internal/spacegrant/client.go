// Package spacegrant is documents-api's client for the two Control calls that
// attach a room file to a Space and detach it again (WS14-S3).
//
// Cross-plane rule: the grant store lives in user-core (Control Plane);
// documents-api never touches it directly.
//
//   - AttachFile calls POST /api/v1/internal/spaces/file-grant with the
//     Control-signed import decision the file was accepted under. The import
//     worker has no user bearer, so the decision is the proof; Control verifies
//     its own signature and re-resolves membership, audience and entitlement.
//   - RevokeFile calls the owner-asserted DELETE /api/v1/internal/authz/grant
//     for the uploader, with the uploader's bearer and a signed v2 delegation
//     that names this exact operation and document.
package spacegrant

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
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
	servicePrincipal = "documents-api"
	maxBodyBytes     = 64 * 1024
)

// ErrNotConfigured means the client has no URL or service credential. Callers
// fail closed.
var ErrNotConfigured = errors.New("spacegrant: client not configured")

// ErrUnavailable means Control could not be reached or answered with an
// unexpected status (including 404 when its Space-subject flag is off).
var ErrUnavailable = errors.New("spacegrant: Control unavailable")

// DeniedError is a 403 from Control. Code is Control's stable denial code when
// it sent one.
type DeniedError struct {
	Code string
}

func (e *DeniedError) Error() string {
	if e.Code == "" {
		return "spacegrant: Control denied the room file grant"
	}
	return "spacegrant: Control denied the room file grant: " + e.Code
}

// Client calls user-core.
type Client struct {
	baseURL      string
	serviceToken string
	http         *http.Client
	now          func() time.Time
}

// New constructs a Client. baseURL is user-core's root (e.g. http://user-core:3012).
func New(baseURL, serviceToken string) *Client {
	return &Client{
		baseURL:      strings.TrimRight(strings.TrimSpace(baseURL), "/"),
		serviceToken: strings.TrimSpace(serviceToken),
		http:         &http.Client{Timeout: 5 * time.Second},
		now:          time.Now,
	}
}

func (c *Client) configured() bool {
	return c != nil && c.baseURL != "" && c.serviceToken != ""
}

// AttachFile writes (idempotently) the Space view grant for documentID, proved
// by the Control-signed import decision token the document was accepted under.
func (c *Client) AttachFile(ctx context.Context, decisionToken, documentID string) error {
	if !c.configured() {
		return ErrNotConfigured
	}
	decisionToken = strings.TrimSpace(decisionToken)
	documentID = strings.TrimSpace(documentID)
	if decisionToken == "" || documentID == "" {
		return errors.New("spacegrant: decision token and document id are required")
	}
	body, err := json.Marshal(map[string]string{"decision_token": decisionToken, "document_id": documentID})
	if err != nil {
		return fmt.Errorf("spacegrant: encode request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+"/api/v1/internal/spaces/file-grant", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("spacegrant: build request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("X-Service-Token", c.serviceToken)
	req.Header.Set("X-Service-Id", servicePrincipal)

	resp, err := c.http.Do(req)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	defer resp.Body.Close()
	payload, _ := io.ReadAll(io.LimitReader(resp.Body, maxBodyBytes))
	switch resp.StatusCode {
	case http.StatusOK:
		return nil
	case http.StatusForbidden:
		var refusal struct {
			Code string `json:"code"`
		}
		_ = json.Unmarshal(payload, &refusal)
		return &DeniedError{Code: strings.TrimSpace(refusal.Code)}
	default:
		return fmt.Errorf("%w: status %d", ErrUnavailable, resp.StatusCode)
	}
}

// RevokeFile removes the Space view grant for documentID on the uploader's
// behalf. authorization is the uploader's verified bearer. Control answers 200
// for an absent grant, so it is idempotent.
func (c *Client) RevokeFile(ctx context.Context, orgID, userID, authorization, documentID, spaceRef string) error {
	if !c.configured() {
		return ErrNotConfigured
	}
	if !strings.HasPrefix(authorization, "Bearer ") || strings.TrimSpace(strings.TrimPrefix(authorization, "Bearer ")) == "" || strings.ContainsAny(authorization, "\r\n") {
		return errors.New("spacegrant: verified user bearer required")
	}
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(userID) == "" || strings.TrimSpace(documentID) == "" || strings.TrimSpace(spaceRef) == "" {
		return errors.New("spacegrant: org, user, document and Space are required")
	}
	q := url.Values{}
	q.Set("org_id", orgID)
	q.Set("resource_type", "document")
	q.Set("resource_id", documentID)
	q.Set("subject_id", spaceRef)
	q.Set("subject_type", "space")
	req, err := http.NewRequestWithContext(ctx, http.MethodDelete, c.baseURL+"/api/v1/internal/authz/grant?"+q.Encode(), nil)
	if err != nil {
		return fmt.Errorf("spacegrant: build request: %w", err)
	}
	req.Header.Set("Authorization", authorization)
	req.Header.Set("X-Service-Token", c.serviceToken)
	req.Header.Set("X-Service-Id", servicePrincipal)
	if err := c.signDelegation(req, orgID, userID, "authz:revoke", "document", documentID, "revoke room file Space grant"); err != nil {
		return err
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrUnavailable, err)
	}
	defer resp.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, maxBodyBytes))
	switch resp.StatusCode {
	case http.StatusOK:
		return nil
	case http.StatusForbidden:
		return &DeniedError{}
	default:
		return fmt.Errorf("%w: status %d", ErrUnavailable, resp.StatusCode)
	}
}

// signDelegation signs the v2 delegation Control's service_delegation.go
// verifies (same canonical form as userauthz, with the operation and resource
// as parameters). The body is empty.
func (c *Client) signDelegation(req *http.Request, orgID, userID, operation, resourceType, resourceID, reason string) error {
	timestamp := c.now().UTC().Format(time.RFC3339)
	nonceBytes := make([]byte, 18)
	if _, err := rand.Read(nonceBytes); err != nil {
		return fmt.Errorf("spacegrant: generate delegation nonce: %w", err)
	}
	nonce := base64.RawURLEncoding.EncodeToString(nonceBytes)
	bodyDigest := sha256.Sum256(nil)
	digest := base64.RawURLEncoding.EncodeToString(bodyDigest[:])
	canonical := strings.Join([]string{
		"v2",
		servicePrincipal,
		"user-core",
		timestamp,
		req.Method,
		req.URL.RequestURI(),
		userID,
		orgID,
		operation,
		resourceType,
		resourceID,
		reason,
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
	req.Header.Set("X-Delegation-Operation", operation)
	req.Header.Set("X-Delegation-Resource-Type", resourceType)
	req.Header.Set("X-Delegation-Resource-Id", resourceID)
	req.Header.Set("X-Delegation-Reason", reason)
	req.Header.Set("X-Delegation-ZDR", "true")
	req.Header.Set("X-Delegation-Body-SHA256", digest)
	req.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
	return nil
}
