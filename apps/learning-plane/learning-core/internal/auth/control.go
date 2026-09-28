package auth

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

var (
	ErrUnauthenticated = errors.New("authentication required")
	ErrForbidden       = errors.New("active school membership required")
	ErrAuthorityDown   = errors.New("Control Plane authority unavailable")
)

type Identity struct {
	UserID    string
	OrgID     string
	SessionID string
	OrgRole   string
	ExpiresAt time.Time
}

type Authority struct {
	baseURL string
	token   string
	client  *http.Client
}

func NewAuthority(baseURL, membershipToken string) (*Authority, error) {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" || membershipToken == "" {
		return nil, errors.New("Control Plane URL and Learning membership credential are required")
	}
	return &Authority{
		baseURL: baseURL,
		token:   membershipToken,
		client: &http.Client{
			Timeout: 3 * time.Second,
			CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
	}, nil
}

// Ping checks that Auth Core responds and the dedicated Learning membership
// credential is accepted. The deliberately invalid exact-membership body is
// rejected before any user or organization lookup and creates no state.
func (a *Authority) Ping(ctx context.Context) error {
	if a == nil {
		return ErrAuthorityDown
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, a.baseURL+"/api/v1/internal/membership/decision", strings.NewReader(`{"userId":"","orgId":""}`))
	if err != nil {
		return ErrAuthorityDown
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Learning-Core-Membership-Token", a.token)
	response, err := a.client.Do(request)
	if err != nil {
		return ErrAuthorityDown
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusBadRequest {
		return ErrAuthorityDown
	}
	return nil
}

// Resolve authenticates a live Control session, then checks the exact
// active-organization membership with Auth Core's canonical membership table.
// Caller-supplied user/org/role headers are never consulted.
func (a *Authority) Resolve(ctx context.Context, authorization, cookie string) (Identity, error) {
	var identity Identity
	if a == nil {
		return identity, ErrAuthorityDown
	}
	if authorization != "" && cookie != "" {
		return identity, ErrUnauthenticated
	}
	if authorization == "" && cookie == "" {
		return identity, ErrUnauthenticated
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, a.baseURL+"/api/auth/get-session", nil)
	if err != nil {
		return identity, fmt.Errorf("build Control session request: %w", ErrAuthorityDown)
	}
	if authorization != "" {
		parts := strings.SplitN(authorization, " ", 2)
		if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") || strings.TrimSpace(parts[1]) == "" {
			return identity, ErrUnauthenticated
		}
		req.Header.Set("Authorization", "Bearer "+strings.TrimSpace(parts[1]))
	} else {
		if strings.ContainsAny(cookie, "\r\n") {
			return identity, ErrUnauthenticated
		}
		req.Header.Set("Cookie", cookie)
	}
	resp, err := a.client.Do(req)
	if err != nil {
		return identity, ErrAuthorityDown
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusUnauthorized || resp.StatusCode == http.StatusForbidden {
		return identity, ErrUnauthenticated
	}
	if resp.StatusCode != http.StatusOK {
		return identity, ErrAuthorityDown
	}

	var sessionPayload struct {
		Session *struct {
			ID                   string    `json:"id"`
			ActiveOrganizationID string    `json:"activeOrganizationId"`
			ExpiresAt            time.Time `json:"expiresAt"`
		} `json:"session"`
		User *struct {
			ID string `json:"id"`
		} `json:"user"`
	}
	if err := decodeBounded(resp.Body, 64<<10, &sessionPayload); err != nil {
		return identity, ErrAuthorityDown
	}
	if sessionPayload.User == nil || sessionPayload.Session == nil ||
		strings.TrimSpace(sessionPayload.User.ID) == "" ||
		strings.TrimSpace(sessionPayload.Session.ID) == "" ||
		strings.TrimSpace(sessionPayload.Session.ActiveOrganizationID) == "" ||
		!sessionPayload.Session.ExpiresAt.After(time.Now().UTC()) {
		return identity, ErrUnauthenticated
	}

	identity = Identity{
		UserID:    strings.TrimSpace(sessionPayload.User.ID),
		OrgID:     strings.TrimSpace(sessionPayload.Session.ActiveOrganizationID),
		SessionID: strings.TrimSpace(sessionPayload.Session.ID),
		ExpiresAt: sessionPayload.Session.ExpiresAt.UTC(),
	}
	role, member, err := a.CheckMembership(ctx, identity.UserID, identity.OrgID)
	if err != nil {
		return Identity{}, err
	}
	if !member {
		return Identity{}, ErrForbidden
	}
	identity.OrgRole = role
	return identity, nil
}

// CheckMembership asks Auth Core's canonical membership authority about one
// exact subject/organization pair. It does not expose a roster-list operation.
func (a *Authority) CheckMembership(ctx context.Context, userID, orgID string) (string, bool, error) {
	if a == nil || strings.TrimSpace(userID) == "" || strings.TrimSpace(orgID) == "" {
		return "", false, ErrAuthorityDown
	}
	decisionBody, _ := json.Marshal(map[string]string{"userId": strings.TrimSpace(userID), "orgId": strings.TrimSpace(orgID)})
	decisionReq, err := http.NewRequestWithContext(ctx, http.MethodPost, a.baseURL+"/api/v1/internal/membership/decision", bytes.NewReader(decisionBody))
	if err != nil {
		return "", false, ErrAuthorityDown
	}
	decisionReq.Header.Set("Content-Type", "application/json")
	decisionReq.Header.Set("X-Learning-Core-Membership-Token", a.token)
	decisionResp, err := a.client.Do(decisionReq)
	if err != nil {
		return "", false, ErrAuthorityDown
	}
	defer decisionResp.Body.Close()
	if decisionResp.StatusCode == http.StatusUnauthorized || decisionResp.StatusCode == http.StatusForbidden {
		return "", false, ErrAuthorityDown
	}
	if decisionResp.StatusCode != http.StatusOK {
		return "", false, ErrAuthorityDown
	}
	var decision struct {
		Version string  `json:"version"`
		Member  bool    `json:"member"`
		Role    *string `json:"role"`
	}
	if err := decodeBounded(decisionResp.Body, 16<<10, &decision); err != nil || decision.Version != "v1" {
		return "", false, ErrAuthorityDown
	}
	if !decision.Member {
		if decision.Role != nil {
			return "", false, ErrAuthorityDown
		}
		return "", false, nil
	}
	if decision.Role == nil {
		return "", false, ErrAuthorityDown
	}
	role := strings.ToLower(strings.TrimSpace(*decision.Role))
	switch role {
	case "owner", "admin", "member", "viewer":
		return role, true, nil
	default:
		return "", false, ErrAuthorityDown
	}
}

func decodeBounded(reader io.Reader, maxBytes int64, target any) error {
	decoder := json.NewDecoder(io.LimitReader(reader, maxBytes+1))
	if err := decoder.Decode(target); err != nil {
		return err
	}
	if decoder.InputOffset() > maxBytes {
		return errors.New("Control response exceeded size limit")
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		return errors.New("Control response contains trailing data")
	}
	return nil
}
