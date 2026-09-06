package oauth

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Delegated sign-in tokens.
//
// Control Plane (auth-core / Better Auth) owns the Microsoft sign-in identity
// and its refresh token, which is bound to auth-core's own Azure app
// registration. integration-corev2 owns the org's Microsoft *connection*. The
// two used to drift: a user signed in with Microsoft every day while the
// integration connection's separate refresh token expired after two idle days
// and every lane failed until a manual reconnect.
//
// AdoptDelegatedToken lets auth-core hand the login's access token to this
// service after each sign-in. The connection is created if missing, revived if
// it was `needs_refresh`, and its capabilities are the UNION of what it already
// had and what the login token's scopes support — a sign-in can only ever
// widen a connection. The refresh token is deliberately NOT adopted: it cannot
// be redeemed with this service's client credentials. Instead the Better Auth
// account row id is recorded as `control_plane_token_ref`, and when this
// service's own refresh fails (or it never had one) it asks auth-core to
// re-mint via ControlPlaneTokenSource.

const (
	providerContextTokenSource      = "token_source"
	providerContextTokenRef         = "control_plane_token_ref"
	providerContextSignInAccountID  = "control_plane_account_id"
	tokenSourceControlPlaneSignIn   = "control-plane-signin"
	delegatedTokenDefaultLifetime   = 55 * time.Minute
	eventConnectionAdopted          = "verevon.ingestion.integration.connection_adopted"
	auditEventConnectionAdopted     = "connection.signin_adopted"
	auditEventConnectionCPRefreshed = "connection.control_plane_refreshed"
)

// ErrControlPlaneTokenSourceUnavailable means no scoped Control Plane
// credential is configured (or auth-core rejected the principal). Callers keep
// their original error instead of treating it as a provider verdict.
var ErrControlPlaneTokenSourceUnavailable = errors.New("control plane token source unavailable")

// ControlPlaneRefreshError is auth-core's structured refusal to re-mint.
type ControlPlaneRefreshError struct {
	Code   string
	Detail string
}

func (e *ControlPlaneRefreshError) Error() string {
	if e.Detail != "" {
		return fmt.Sprintf("auth-core oauth refresh %s: %s", e.Code, e.Detail)
	}
	return "auth-core oauth refresh " + e.Code
}

// DelegatedToken is a Graph access token minted by Control Plane's Azure app.
type DelegatedToken struct {
	AccessToken string
	ExpiresAt   time.Time
	Scopes      []string
}

// ControlPlaneTokenSource re-mints a delegated token for a Better Auth account
// row (`tokenRef`). Implemented by controlplane.AuthOAuthClient.
type ControlPlaneTokenSource interface {
	RefreshDelegatedToken(ctx context.Context, tokenRef string) (DelegatedToken, error)
}

// SetControlPlaneTokenSource enables the Control Plane refresh fallback.
func (s *Service) SetControlPlaneTokenSource(source ControlPlaneTokenSource) {
	s.controlPlane = source
}

// DelegatedTokenInput is what auth-core hands over after a sign-in.
type DelegatedTokenInput struct {
	ProviderKey       string
	OrganizationID    string
	WorkspaceID       string
	UserID            string
	UserEmail         string
	ProviderAccountID string
	AccessToken       string
	ExpiresAt         time.Time
	Scopes            []string
	TokenRef          string
}

// AdoptDelegatedToken creates or widens the org's provider connection from a
// Control Plane sign-in token. The bool reports whether a connection was
// created (as opposed to updated).
func (s *Service) AdoptDelegatedToken(ctx context.Context, input DelegatedTokenInput) (store.Connection, bool, error) {
	provider, ok := providers.FindOAuth(input.ProviderKey)
	if !ok {
		return store.Connection{}, false, fmt.Errorf("provider %q is not an OAuth provider", input.ProviderKey)
	}
	organizationID := strings.TrimSpace(input.OrganizationID)
	accessToken := strings.TrimSpace(input.AccessToken)
	if organizationID == "" || accessToken == "" {
		return store.Connection{}, false, errors.New("organizationId and accessToken are required")
	}
	expiresAt := input.ExpiresAt
	if expiresAt.IsZero() {
		expiresAt = s.now().Add(delegatedTokenDefaultLifetime)
	}

	// Identify the provider account with the token itself: Better Auth's
	// `account.account_id` is the OIDC subject, while connections created by
	// this service's own OAuth flow store the Graph object id. Matching on the
	// Graph profile keeps one connection per account instead of minting a
	// duplicate on the first hand-off. Best-effort: an unreachable Graph
	// falls back to the org's single connection for this connector.
	profile := ProviderProfile{}
	if client, ok := s.clients[provider.Key]; ok {
		profile, _ = client.Profile(ctx, accessToken, nil)
	}
	profileAccountID := strings.TrimSpace(profile.ID)

	existing, found, err := s.findAdoptableConnection(ctx, organizationID, provider.ConnectorType, profileAccountID)
	if err != nil {
		return store.Connection{}, false, err
	}

	granted := grantedScopesForSignIn(input.Scopes)
	grantedCapabilities := capabilitiesAllowedByScopes(provider, allCapabilityKeys(provider), granted)

	connectionID := "conn_" + uuid.NewString()
	createdAt := s.now()
	capabilities := grantedCapabilities
	scopes := granted
	encryptedRefreshToken := ""
	providerContext := map[string]string{}
	displayName := firstNonEmptyString(profile.DisplayName, strings.TrimSpace(input.UserEmail), provider.Key+" connection")
	lastSyncStatus := "pending"
	if found {
		connectionID = existing.ID
		createdAt = existing.CreatedAt
		capabilities = unionSorted(existing.Capabilities, grantedCapabilities)
		scopes = unionSorted(existing.Scopes, granted)
		// Never adopt the sign-in refresh token: it belongs to Control Plane's
		// Azure app and this service's client cannot redeem it. Keep our own.
		encryptedRefreshToken = existing.EncryptedRefreshToken
		for key, value := range existing.ProviderContext {
			providerContext[key] = value
		}
		displayName = firstNonEmptyString(existing.DisplayName, displayName)
		lastSyncStatus = existing.LastSyncStatus
	}
	providerContext[providerContextTokenSource] = tokenSourceControlPlaneSignIn
	if ref := strings.TrimSpace(input.TokenRef); ref != "" {
		providerContext[providerContextTokenRef] = ref
	}
	if accountID := strings.TrimSpace(input.ProviderAccountID); accountID != "" {
		providerContext[providerContextSignInAccountID] = accountID
	}
	if email := strings.TrimSpace(profile.Email); email != "" && providerContext["mailbox_address"] == "" {
		providerContext["mailbox_address"] = email
	}

	encryptedAccessToken, err := s.vault.Encrypt(accessToken, []byte(connectionID))
	if err != nil {
		return store.Connection{}, false, err
	}

	connection := store.Connection{
		ID:                    connectionID,
		ProviderKey:           provider.Key,
		ConnectorType:         provider.ConnectorType,
		OrganizationID:        organizationID,
		WorkspaceID:           firstNonEmptyString(strings.TrimSpace(input.WorkspaceID), existing.WorkspaceID, organizationID),
		UserID:                firstNonEmptyString(strings.TrimSpace(input.UserID), existing.UserID),
		UserEmail:             firstNonEmptyString(strings.TrimSpace(input.UserEmail), existing.UserEmail),
		Status:                "active",
		DisplayName:           displayName,
		ProviderAccountID:     firstNonEmptyString(existing.ProviderAccountID, profileAccountID),
		TenantID:              firstNonEmptyString(existing.TenantID, profile.TenantID),
		ProviderContext:       providerContext,
		Capabilities:          capabilities,
		Scopes:                scopes,
		EncryptedAccessToken:  encryptedAccessToken,
		EncryptedRefreshToken: encryptedRefreshToken,
		AccessTokenExpiresAt:  expiresAt,
		LastRefreshedAt:       s.now(),
		LastSyncStatus:        lastSyncStatus,
		CreatedAt:             createdAt,
		UpdatedAt:             s.now(),
	}

	var saved store.Connection
	err = s.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		var saveErr error
		saved, saveErr = tx.UpsertConnection(ctx, connection)
		if saveErr != nil {
			return saveErr
		}
		return tx.InsertAuditEvent(ctx, store.AuditEvent{
			ID:             "audit:integration:connection-signin-adopted:" + saved.ID + ":" + uuid.NewString(),
			OrganizationID: organizationID,
			UserID:         saved.UserID,
			ConnectionID:   saved.ID,
			EventType:      auditEventConnectionAdopted,
			ProviderKey:    provider.Key,
			Metadata: map[string]any{
				"created":             !found,
				"grantedCapabilities": append([]string(nil), grantedCapabilities...),
				"capabilities":        append([]string(nil), saved.Capabilities...),
				"scopes":              redactScopes(saved.Scopes),
				"tokenSource":         tokenSourceControlPlaneSignIn,
			},
			CreatedAt: s.now(),
		})
	})
	if err != nil {
		return store.Connection{}, false, err
	}
	_ = s.publisher.Publish(ctx, events.Event{
		ID:             uuid.NewString(),
		Type:           eventConnectionAdopted,
		Source:         "integration-corev2",
		Version:        "1",
		OrganizationID: organizationID,
		WorkspaceID:    saved.WorkspaceID,
		UserID:         saved.UserID,
		ConnectionID:   saved.ID,
		ProviderKey:    saved.ProviderKey,
		Data: map[string]any{
			"created":      !found,
			"capabilities": append([]string(nil), saved.Capabilities...),
			"tokenSource":  tokenSourceControlPlaneSignIn,
		},
	})
	return saved, !found, nil
}

// findAdoptableConnection prefers the connection for this exact provider
// account, then the org's single connection for the connector (active or
// needs_refresh — a fresh sign-in is exactly what revives the latter).
func (s *Service) findAdoptableConnection(ctx context.Context, organizationID, connectorType, providerAccountID string) (store.Connection, bool, error) {
	if providerAccountID != "" {
		connection, err := s.repo.FindActiveConnectionByProviderAccount(ctx, organizationID, connectorType, providerAccountID)
		if err == nil {
			return connection, true, nil
		}
		if !errors.Is(err, store.ErrNotFound) {
			return store.Connection{}, false, err
		}
	}
	connection, err := s.repo.FindActiveConnection(ctx, organizationID, connectorType)
	if err == nil {
		return connection, true, nil
	}
	if errors.Is(err, store.ErrNotFound) {
		return store.Connection{}, false, nil
	}
	return store.Connection{}, false, err
}

// refreshFromControlPlane re-mints the access token through auth-core when the
// connection carries a Control Plane token ref. The bool is false when the
// fallback does not apply (not configured, or no ref); the caller then keeps
// its own error.
func (s *Service) refreshFromControlPlane(ctx context.Context, connection store.Connection) (store.Connection, string, bool, error) {
	if s.controlPlane == nil {
		return store.Connection{}, "", false, nil
	}
	tokenRef := strings.TrimSpace(connection.ProviderContext[providerContextTokenRef])
	if tokenRef == "" {
		return store.Connection{}, "", false, nil
	}
	token, err := s.controlPlane.RefreshDelegatedToken(ctx, tokenRef)
	if err != nil {
		if errors.Is(err, ErrControlPlaneTokenSourceUnavailable) {
			return store.Connection{}, "", false, nil
		}
		return store.Connection{}, "", true, err
	}
	encryptedAccessToken, err := s.vault.Encrypt(token.AccessToken, []byte(connection.ID))
	if err != nil {
		return store.Connection{}, "", true, err
	}
	connection.EncryptedAccessToken = encryptedAccessToken
	connection.AccessTokenExpiresAt = token.ExpiresAt
	connection.LastRefreshedAt = s.now()
	connection.Status = "active"
	if provider, ok := providers.FindOAuth(connection.ProviderKey); ok && len(token.Scopes) > 0 {
		granted := grantedScopesForSignIn(token.Scopes)
		connection.Scopes = unionSorted(connection.Scopes, granted)
		connection.Capabilities = unionSorted(connection.Capabilities, capabilitiesAllowedByScopes(provider, allCapabilityKeys(provider), granted))
	}
	saved, err := s.repo.UpdateConnectionCredentials(ctx, connection)
	if err != nil {
		return store.Connection{}, "", true, err
	}
	_ = s.publisher.Publish(ctx, events.Event{
		ID:             uuid.NewString(),
		Type:           "verevon.ingestion.integration." + auditEventConnectionCPRefreshed,
		Source:         "integration-corev2",
		Version:        "1",
		OrganizationID: saved.OrganizationID,
		WorkspaceID:    saved.WorkspaceID,
		UserID:         saved.UserID,
		ConnectionID:   saved.ID,
		ProviderKey:    saved.ProviderKey,
		Data:           map[string]any{"tokenSource": tokenSourceControlPlaneSignIn},
	})
	return saved, token.AccessToken, true, nil
}

// SplitGrantedScopes accepts Better Auth's comma-separated form and the
// provider's space-separated form.
func SplitGrantedScopes(raw string) []string {
	fields := strings.FieldsFunc(raw, func(r rune) bool { return r == ',' || r == ' ' || r == '\t' || r == '\n' })
	return normalizedStrings(fields)
}

// grantedScopesForSignIn normalizes the scopes a sign-in token carries for
// capability derivation. Graph sometimes prefixes resource scopes with its
// URL, and providers rarely echo the OIDC scopes (`openid`, `profile`,
// `email`, `offline_access`) back in the token response even though the
// session itself proves identity — those are treated as granted so the
// identity capability is not withheld from a token that plainly has it.
func grantedScopesForSignIn(scopes []string) []string {
	out := []string{"openid", "profile", "email", "offline_access"}
	for _, scope := range scopes {
		for _, part := range SplitGrantedScopes(scope) {
			part = strings.TrimPrefix(part, "https://graph.microsoft.com/")
			if part != "" {
				out = append(out, part)
			}
		}
	}
	return normalizedStrings(out)
}

func allCapabilityKeys(provider providers.Provider) []string {
	keys := make([]string, 0, len(provider.Capabilities))
	for _, capability := range provider.Capabilities {
		keys = append(keys, capability.Key)
	}
	return keys
}

func unionSorted(left, right []string) []string {
	merged := make([]string, 0, len(left)+len(right))
	merged = append(merged, left...)
	merged = append(merged, right...)
	merged = normalizedStrings(merged)
	slices.Sort(merged)
	return merged
}

func firstNonEmptyString(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}
