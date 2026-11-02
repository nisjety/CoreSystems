package userauthz

// WS14-S4: the owner-asserted grant writes. documents-api is the registered
// owner service for resource type "document" (user-core authztaxonomy
// OwnerService), so it is the one caller Control admits on POST/DELETE
// /authz/grant and GET /authz/grants when CONTROL_OWNER_ASSERTED_GRANTS is on.
// Control cannot see documents, so it trusts this service to have proved the
// ACTOR owns the document before calling (the share handlers do that). Every
// call carries the actor's verified bearer plus a v2 delegation that names this
// exact operation, document and tenant; Control takes granted_by from the
// delegated user, never from the body.

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
)

const maxGrantResponseBytes = 1 << 20

// ErrNotConfigured means the client has no URL or service credential.
var ErrNotConfigured = errors.New("userauthz: client not configured")

// StatusError is a non-200 answer from Control. Code is Control's stable code
// when it sent one (role_not_supported, subject_type_not_supported,
// member_not_in_organization, ...).
type StatusError struct {
	Status  int
	Code    string
	Message string
}

func (e *StatusError) Error() string {
	if e.Code != "" {
		return fmt.Sprintf("userauthz: user-core answered %d (%s)", e.Status, e.Code)
	}
	return fmt.Sprintf("userauthz: user-core answered %d", e.Status)
}

// Grant is one explicit grant on a document as Control lists it.
type Grant struct {
	GrantID     string `json:"grant_id"`
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	Role        string `json:"role"`
	GrantedBy   string `json:"granted_by"`
	GrantedAt   string `json:"granted_at"`
}

// SpaceVisible is Control's read-time join for one Space: the documents
// attached to the Space that the delegated user may see through it now.
type SpaceVisible struct {
	IDs              []string        `json:"ids"`
	SpaceRef         string          `json:"space_ref"`
	AudienceRevision json.RawMessage `json:"audience_revision"`
}

func verifiedBearer(authorization string) bool {
	return strings.HasPrefix(authorization, "Bearer ") &&
		strings.TrimSpace(strings.TrimPrefix(authorization, "Bearer ")) != "" &&
		!strings.ContainsAny(authorization, "\r\n")
}

// call sends one delegated request to user-core and returns the 200 payload.
// A non-200 answer becomes a *StatusError; a transport failure is wrapped.
func (c *Client) call(ctx context.Context, method, pathAndQuery string, body []byte, orgID, userID, authorization, operation, resourceID, reason string) ([]byte, error) {
	if c == nil || c.baseURL == "" || c.serviceToken == "" {
		return nil, ErrNotConfigured
	}
	if !verifiedBearer(authorization) {
		return nil, fmt.Errorf("userauthz: verified user bearer required")
	}
	if strings.TrimSpace(orgID) == "" || strings.TrimSpace(userID) == "" {
		return nil, fmt.Errorf("userauthz: org and user are required")
	}
	var reader io.Reader
	if body != nil {
		reader = bytes.NewReader(body)
	}
	req, err := http.NewRequestWithContext(ctx, method, c.baseURL+pathAndQuery, reader)
	if err != nil {
		return nil, fmt.Errorf("userauthz: build request: %w", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	req.Header.Set("Authorization", authorization)
	req.Header.Set("X-Service-Token", c.serviceToken)
	req.Header.Set("X-Service-Id", "documents-api")
	if err := c.signOperation(req, orgID, userID, operation, "document", resourceID, reason, body); err != nil {
		return nil, fmt.Errorf("userauthz: sign request: %w", err)
	}
	resp, err := c.http.Do(req)
	if err != nil {
		return nil, fmt.Errorf("userauthz: request failed: %w", err)
	}
	defer resp.Body.Close()
	payload, _ := io.ReadAll(io.LimitReader(resp.Body, maxGrantResponseBytes))
	if resp.StatusCode != http.StatusOK {
		var refusal struct {
			Code  string `json:"code"`
			Error string `json:"error"`
		}
		_ = json.Unmarshal(payload, &refusal)
		return nil, &StatusError{Status: resp.StatusCode, Code: strings.TrimSpace(refusal.Code), Message: strings.TrimSpace(refusal.Error)}
	}
	return payload, nil
}

// Grant writes (idempotently) a user view grant on documentID. The caller must
// already have proved userID owns the document. The role is passed through so
// Control can refuse one it does not support for documents.
func (c *Client) Grant(ctx context.Context, orgID, userID, authorization, documentID, subjectID, role string) (*Grant, error) {
	body, err := json.Marshal(map[string]string{
		"org_id":        orgID,
		"resource_type": "document",
		"resource_id":   documentID,
		"subject_id":    subjectID,
		"subject_type":  "user",
		"role":          role,
	})
	if err != nil {
		return nil, fmt.Errorf("userauthz: encode request: %w", err)
	}
	payload, err := c.call(ctx, http.MethodPost, "/api/v1/internal/authz/grant", body,
		orgID, userID, authorization, "authz:grant", documentID, "share document with a user")
	if err != nil {
		return nil, err
	}
	var out Grant
	if err := json.Unmarshal(payload, &out); err != nil {
		return nil, fmt.Errorf("userauthz: decode response: %w", err)
	}
	return &out, nil
}

// Revoke removes the user grant for subjectID on documentID. Control answers
// 200 for an absent grant, so it is idempotent. Only user subjects: a Space
// subject is removed by the room-file delete path, never here.
func (c *Client) Revoke(ctx context.Context, orgID, userID, authorization, documentID, subjectID string) error {
	q := url.Values{}
	q.Set("org_id", orgID)
	q.Set("resource_type", "document")
	q.Set("resource_id", documentID)
	q.Set("subject_id", subjectID)
	q.Set("subject_type", "user")
	_, err := c.call(ctx, http.MethodDelete, "/api/v1/internal/authz/grant?"+q.Encode(), nil,
		orgID, userID, authorization, "authz:revoke", documentID, "remove a document share")
	return err
}

// ListGrants lists every explicit grant on documentID (user and Space rows).
func (c *Client) ListGrants(ctx context.Context, orgID, userID, authorization, documentID string) ([]Grant, error) {
	q := url.Values{}
	q.Set("org_id", orgID)
	q.Set("resource_type", "document")
	q.Set("resource_id", documentID)
	payload, err := c.call(ctx, http.MethodGet, "/api/v1/internal/authz/grants?"+q.Encode(), nil,
		orgID, userID, authorization, "authz:grants", documentID, "list document shares")
	if err != nil {
		return nil, err
	}
	var out struct {
		Grants []Grant `json:"grants"`
	}
	if err := json.Unmarshal(payload, &out); err != nil {
		return nil, fmt.Errorf("userauthz: decode response: %w", err)
	}
	return out.Grants, nil
}

// ListVisibleSpace returns the documents attached to spaceRef that userID may
// see through it now (current member AND current audience). Control fails
// closed to an empty set for a user who does not resolve. The delegation's
// resource id is the Space reference, which Control requires to match.
func (c *Client) ListVisibleSpace(ctx context.Context, orgID, userID, authorization, spaceRef string) (*SpaceVisible, error) {
	q := url.Values{}
	q.Set("org_id", orgID)
	q.Set("subject_id", userID)
	q.Set("resource_type", "document")
	q.Set("space_ref", spaceRef)
	payload, err := c.call(ctx, http.MethodGet, "/api/v1/internal/authz/space-visible?"+q.Encode(), nil,
		orgID, userID, authorization, "authz:space-visible", spaceRef, "resolve room files visible through a Space")
	if err != nil {
		return nil, err
	}
	var out SpaceVisible
	if err := json.Unmarshal(payload, &out); err != nil {
		return nil, fmt.Errorf("userauthz: decode response: %w", err)
	}
	return &out, nil
}
