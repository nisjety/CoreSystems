package oauth

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"

	"github.com/triodelab/integration-corev2/internal/config"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/providers"
	"github.com/triodelab/integration-corev2/internal/store"
)

type Service struct {
	cfg       config.Config
	repo      store.Repository
	vault     *secretcrypto.Vault
	microsoft *MicrosoftClient
	clients   map[string]ProviderOAuthClient
	publisher events.Publisher
	now       func() time.Time
	refreshMu sync.Mutex
	refreshes map[string]*refreshCall
	// controlPlane re-mints Control Plane-owned sign-in tokens; nil disables
	// the fallback (see delegated.go).
	controlPlane ControlPlaneTokenSource
}

type refreshCall struct {
	done        chan struct{}
	connection  store.Connection
	accessToken string
	err         error
}

type CreateSessionInput struct {
	ProviderKey     string
	OrganizationID  string
	WorkspaceID     string
	UserID          string
	UserEmail       string
	SelectedSources []string
	Capabilities    []string
	Bundles         []string
	ReturnURL       string
	ProviderContext map[string]string
}

type CreateSessionResult struct {
	SessionToken      string    `json:"sessionToken"`
	ConnectURL        string    `json:"connectUrl"`
	AuthorizationURL  string    `json:"authorizationUrl"`
	AuthMode          string    `json:"authMode"`
	ExpiresAt         time.Time `json:"expiresAt"`
	ProviderConfigKey string    `json:"providerConfigKey"`
	Provider          struct {
		Key       string `json:"key"`
		Label     string `json:"label"`
		ConfigKey string `json:"configKey"`
	} `json:"provider"`
	Capabilities []string `json:"capabilities"`
	Scopes       []string `json:"scopes"`
}

type CallbackResult struct {
	SessionID    string
	ConnectionID string
	ProviderKey  string
	Success      bool
	ErrorCode    string
	Message      string
	ReturnURL    string
}

type AccessTokenResult struct {
	ConnectionID string    `json:"connectionId"`
	ProviderKey  string    `json:"providerKey"`
	AccessToken  string    `json:"accessToken"`
	ExpiresAt    time.Time `json:"expiresAt"`
	Scopes       []string  `json:"scopes"`
	Capabilities []string  `json:"capabilities"`
}

func NewService(cfg config.Config, repo store.Repository, vault *secretcrypto.Vault, microsoft *MicrosoftClient) *Service {
	clients := NewProviderClients(cfg, microsoft, nil)
	return &Service{
		cfg:       cfg,
		repo:      repo,
		vault:     vault,
		microsoft: microsoft,
		clients:   clients,
		publisher: events.NoopPublisher{},
		now:       func() time.Time { return time.Now().UTC() },
		refreshes: map[string]*refreshCall{},
	}
}

func (s *Service) SetEventPublisher(publisher events.Publisher) {
	if publisher == nil {
		s.publisher = events.NoopPublisher{}
		return
	}
	s.publisher = publisher
}

func (s *Service) CreateSession(ctx context.Context, input CreateSessionInput) (CreateSessionResult, error) {
	provider, ok := providers.FindOAuth(input.ProviderKey)
	if !ok {
		return CreateSessionResult{}, fmt.Errorf("unsupported provider: %s", input.ProviderKey)
	}
	if err := s.cfg.ValidateProvider(provider.Key); err != nil {
		return CreateSessionResult{}, err
	}
	client, ok := s.clients[provider.Key]
	if !provider.DirectOAuthReady || !ok {
		return CreateSessionResult{}, fmt.Errorf("provider %s is registered in the catalog but direct OAuth is not implemented yet", provider.Key)
	}
	if strings.TrimSpace(input.OrganizationID) == "" {
		return CreateSessionResult{}, fmt.Errorf("organizationId is required")
	}
	if strings.TrimSpace(input.UserID) == "" {
		return CreateSessionResult{}, fmt.Errorf("userId is required")
	}
	workspaceID := strings.TrimSpace(input.WorkspaceID)
	if workspaceID == "" {
		workspaceID = strings.TrimSpace(input.OrganizationID)
	}
	providerContext, err := NormalizeProviderContext(provider.Key, input.ProviderContext)
	if err != nil {
		return CreateSessionResult{}, err
	}
	// Meta bakes permissions into a Facebook Login for Business configuration
	// rather than a per-request scope param (see MetaOAuthClient), so a
	// bundle needing a different permission set than the default connection
	// flow needs its own named configuration selected up front — callers
	// selecting the "conversions" bundle shouldn't need to know that OAuth
	// plumbing detail themselves.
	if provider.Key == "meta" && slices.Contains(input.Bundles, "conversions") {
		if _, ok := providerContext["business_login_config"]; !ok {
			providerContext["business_login_config"] = "conversions"
		}
	}
	if provider.Key == "meta" && slices.Contains(input.Bundles, "messenger") {
		if _, ok := providerContext["business_login_config"]; !ok {
			providerContext["business_login_config"] = "support_messaging"
		}
	}
	if provider.Key == "meta" && slices.Contains(input.Bundles, "ads") {
		if _, ok := providerContext["business_login_config"]; !ok {
			providerContext["business_login_config"] = "studio_ads"
		}
	}

	sessionID := "cs_" + uuid.NewString()
	state, err := RandomURLToken(32)
	if err != nil {
		return CreateSessionResult{}, err
	}
	verifier, err := RandomURLToken(64)
	if err != nil {
		return CreateSessionResult{}, err
	}
	redirectURI := s.callbackURL(provider.Key)
	capabilities := providers.ResolveCapabilities(provider, input.Capabilities, input.Bundles)
	scopes := providers.ResolveScopes(provider, capabilities)
	verifierCiphertext, err := s.vault.Encrypt(verifier, []byte(sessionID))
	if err != nil {
		return CreateSessionResult{}, err
	}
	session := store.ConnectSession{
		ID:                     sessionID,
		ProviderKey:            provider.Key,
		ConnectorType:          provider.ConnectorType,
		OrganizationID:         strings.TrimSpace(input.OrganizationID),
		WorkspaceID:            workspaceID,
		UserID:                 strings.TrimSpace(input.UserID),
		UserEmail:              strings.TrimSpace(input.UserEmail),
		StateHash:              HashState(state),
		CodeVerifierCiphertext: verifierCiphertext,
		RedirectURI:            redirectURI,
		ReturnURL:              strings.TrimSpace(input.ReturnURL),
		ProviderContext:        providerContext,
		Capabilities:           capabilities,
		Scopes:                 scopes,
		ExpiresAt:              s.now().Add(s.cfg.SessionTTL),
		CreatedAt:              s.now(),
	}
	authURL, err := client.AuthorizationURL(state, redirectURI, verifier, scopes, providerContext)
	if err != nil {
		return CreateSessionResult{}, err
	}
	if err := s.repo.CreateConnectSession(ctx, session); err != nil {
		return CreateSessionResult{}, err
	}
	result := CreateSessionResult{
		SessionToken:      sessionID,
		ConnectURL:        authURL,
		AuthorizationURL:  authURL,
		AuthMode:          "direct-oauth",
		ExpiresAt:         session.ExpiresAt,
		ProviderConfigKey: provider.ConnectorType,
		Capabilities:      capabilities,
		Scopes:            scopes,
	}
	result.Provider.Key = provider.Key
	result.Provider.Label = provider.Label
	result.Provider.ConfigKey = provider.ConnectorType
	return result, nil
}

func (s *Service) CompleteCallback(ctx context.Context, providerKey, state, code, errorCode, errorDescription string) (CallbackResult, error) {
	stateHash := HashState(state)
	session, err := s.repo.GetConnectSessionByStateHash(ctx, stateHash)
	if err != nil {
		return CallbackResult{Success: false, ErrorCode: "invalid_state", Message: "OAuth state was not recognized."}, err
	}
	result := CallbackResult{
		SessionID:   session.ID,
		ProviderKey: session.ProviderKey,
		ReturnURL:   session.ReturnURL,
	}
	if session.ConsumedAt != nil {
		result.ErrorCode = "session_consumed"
		result.Message = "Connect session was already used."
		return result, fmt.Errorf("connect session already consumed")
	}
	if s.now().After(session.ExpiresAt) {
		_ = s.repo.MarkConnectSessionConsumed(ctx, session.ID, "session_expired", "Connect session expired.")
		result.ErrorCode = "session_expired"
		result.Message = "Connect session expired."
		return result, fmt.Errorf("connect session expired")
	}
	if providers.NormalizeKey(providerKey) != session.ProviderKey {
		result.ErrorCode = "provider_mismatch"
		result.Message = "OAuth callback provider did not match the session."
		return result, fmt.Errorf("provider mismatch")
	}
	if errorCode != "" {
		_ = s.repo.MarkConnectSessionConsumed(ctx, session.ID, errorCode, errorDescription)
		result.ErrorCode = errorCode
		result.Message = errorDescription
		return result, fmt.Errorf("provider returned OAuth error: %s", errorCode)
	}
	if strings.TrimSpace(code) == "" {
		result.ErrorCode = "missing_code"
		result.Message = "OAuth provider did not return a code."
		return result, fmt.Errorf("missing oauth code")
	}

	verifier, err := s.vault.Decrypt(session.CodeVerifierCiphertext, []byte(session.ID))
	if err != nil {
		result.ErrorCode = "invalid_verifier"
		result.Message = "OAuth session verifier could not be read."
		return result, err
	}
	token, err := s.exchangeCode(ctx, session, code, verifier)
	if err != nil {
		_ = s.repo.MarkConnectSessionConsumed(ctx, session.ID, "token_exchange_failed", err.Error())
		result.ErrorCode = "token_exchange_failed"
		result.Message = err.Error()
		return result, err
	}
	connection, err := s.persistConnection(ctx, session, token)
	if err != nil {
		_ = s.repo.MarkConnectSessionConsumed(ctx, session.ID, "connection_store_failed", err.Error())
		result.ErrorCode = "connection_store_failed"
		result.Message = err.Error()
		return result, err
	}
	result.Success = true
	result.ConnectionID = connection.ID
	result.Message = "Connection completed."
	// Meta inbox completion is finalized by the API only after provider asset
	// subscriptions and their durable sync state succeed. This keeps the
	// connect-session status endpoint and integration.connected event truthful.
	if isMetaFamilyProvider(connection.ProviderKey) {
		return result, nil
	}
	if err := s.FinalizeConnectedCallback(ctx, result, connection); err != nil {
		return result, err
	}
	return result, nil
}

func (s *Service) FinalizeConnectedCallback(ctx context.Context, result CallbackResult, connection store.Connection) error {
	if err := s.repo.MarkConnectSessionConsumed(ctx, result.SessionID, "", ""); err != nil {
		return err
	}
	_ = s.publisher.Publish(ctx, events.Event{
		Type:           "integration.connected",
		OrganizationID: connection.OrganizationID,
		WorkspaceID:    connection.WorkspaceID,
		UserID:         connection.UserID,
		ConnectionID:   connection.ID,
		ProviderKey:    connection.ProviderKey,
		Data: map[string]any{
			"connectorType": connection.ConnectorType,
			"displayName":   connection.DisplayName,
			"capabilities":  connection.Capabilities,
			"scopes":        redactScopes(connection.Scopes),
			"status":        connection.Status,
		},
		CreatedAt: s.now(),
	})
	return nil
}

func (s *Service) FailCallback(ctx context.Context, sessionID, errorCode, description string) error {
	return s.repo.MarkConnectSessionConsumed(ctx, sessionID, errorCode, description)
}

func (s *Service) AccessToken(ctx context.Context, organizationID, connectorType string) (AccessTokenResult, error) {
	connection, err := s.repo.FindActiveConnection(ctx, organizationID, connectorType)
	if err != nil {
		return AccessTokenResult{}, err
	}
	return s.accessTokenForConnection(ctx, connection)
}

func (s *Service) AccessTokenForConnection(ctx context.Context, connectionID string) (AccessTokenResult, error) {
	connection, err := s.repo.GetConnection(ctx, connectionID)
	if err != nil {
		return AccessTokenResult{}, err
	}
	if connection.DeletedAt != nil || connection.Status != "active" {
		return AccessTokenResult{}, store.ErrNotFound
	}
	return s.accessTokenForConnection(ctx, connection)
}

func (s *Service) DisconnectConnection(ctx context.Context, connectionID, reason string) (store.Connection, error) {
	connection, err := s.repo.GetConnection(ctx, connectionID)
	if err != nil {
		return store.Connection{}, err
	}
	if connection.DeletedAt != nil || connection.Status == "deleted" {
		return connection, nil
	}
	if strings.TrimSpace(reason) == "" {
		reason = "user_requested"
	}

	// Provider revocation is an external side effect and cannot participate in
	// the database transaction. Persist a resumable saga stage first so a crash
	// or database failure never produces an untracked revocation attempt.
	pending := connection
	pending.Status = "disconnecting"
	err = s.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		var stageErr error
		pending, stageErr = tx.UpsertConnection(ctx, pending)
		if stageErr != nil {
			return stageErr
		}
		return tx.InsertAuditEvent(ctx, store.AuditEvent{
			ID:             "audit:integration:connection-delete-requested:" + pending.ID,
			OrganizationID: pending.OrganizationID,
			UserID:         pending.UserID,
			ConnectionID:   pending.ID,
			EventType:      "connection.delete.requested",
			ProviderKey:    pending.ProviderKey,
			Metadata:       map[string]any{"reason": reason},
			CreatedAt:      s.now(),
		})
	})
	if err != nil {
		return store.Connection{}, err
	}
	connection = pending

	revokeStatus := "not_required"
	var revokeErr error
	if connection.DeletedAt == nil && connection.EncryptedAccessToken != "" {
		if client, ok := s.clients[connection.ProviderKey]; ok {
			accessToken, decryptErr := s.vault.Decrypt(connection.EncryptedAccessToken, []byte(connection.ID))
			if decryptErr != nil {
				revokeStatus = "token_decrypt_failed"
				revokeErr = fmt.Errorf("decrypt provider credential for revocation: %w", decryptErr)
			} else if err := client.Revoke(ctx, accessToken, connection.ProviderContext); errors.Is(err, ErrRevocationUnsupported) {
				revokeStatus = "unsupported_provider_policy"
			} else if err != nil {
				revokeStatus = "revocation_failed"
				revokeErr = fmt.Errorf("revoke provider credential: %w", err)
			} else {
				revokeStatus = "completed"
			}
		} else {
			revokeStatus = "provider_client_unavailable"
			revokeErr = fmt.Errorf("provider client %q is unavailable for revocation", connection.ProviderKey)
		}
	}
	if revokeErr != nil {
		failed := connection
		failed.Status = "revocation_failed"
		if err := s.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
			var persistErr error
			failed, persistErr = tx.UpsertConnection(ctx, failed)
			if persistErr != nil {
				return persistErr
			}
			return tx.InsertAuditEvent(ctx, store.AuditEvent{
				ID:             "audit:integration:connection-revocation-failed:" + failed.ID,
				OrganizationID: failed.OrganizationID,
				UserID:         failed.UserID,
				ConnectionID:   failed.ID,
				EventType:      "connection.revocation.failed",
				ProviderKey:    failed.ProviderKey,
				Metadata: map[string]any{
					"reason":        reason,
					"revoke_status": revokeStatus,
				},
				CreatedAt: s.now(),
			})
		}); err != nil {
			return store.Connection{}, errors.Join(revokeErr, err)
		}
		return failed, revokeErr
	}
	var deleted store.Connection
	err = s.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		var deleteErr error
		deleted, deleteErr = tx.MarkConnectionDeleted(ctx, connection.ID)
		if deleteErr != nil {
			return deleteErr
		}
		return tx.InsertAuditEvent(ctx, store.AuditEvent{
			ID:             "audit:integration:connection-deleted:" + deleted.ID,
			OrganizationID: deleted.OrganizationID,
			UserID:         deleted.UserID,
			ConnectionID:   deleted.ID,
			EventType:      "connection.deleted",
			ProviderKey:    deleted.ProviderKey,
			Metadata: map[string]any{
				"reason":        reason,
				"revoke_status": revokeStatus,
			},
			CreatedAt: s.now(),
		})
	})
	if err != nil {
		return store.Connection{}, err
	}
	_ = s.publisher.Publish(ctx, events.Event{
		Type:           "integration.disconnected",
		OrganizationID: deleted.OrganizationID,
		WorkspaceID:    deleted.WorkspaceID,
		UserID:         deleted.UserID,
		ConnectionID:   deleted.ID,
		ProviderKey:    deleted.ProviderKey,
		Data: map[string]any{
			"connectorType": deleted.ConnectorType,
			"reason":        reason,
			"revokeStatus":  revokeStatus,
			"status":        deleted.Status,
		},
		CreatedAt: s.now(),
	})
	return deleted, nil
}

func (s *Service) accessTokenForConnection(ctx context.Context, connection store.Connection) (AccessTokenResult, error) {
	if connection.DeletedAt != nil || connection.Status != "active" {
		return AccessTokenResult{}, store.ErrNotFound
	}
	accessToken, err := s.vault.Decrypt(connection.EncryptedAccessToken, []byte(connection.ID))
	if err != nil {
		return AccessTokenResult{}, err
	}
	if s.now().Add(s.cfg.TokenRefreshSkew).Before(connection.AccessTokenExpiresAt) {
		return AccessTokenResult{
			ConnectionID: connection.ID,
			ProviderKey:  connection.ProviderKey,
			AccessToken:  accessToken,
			ExpiresAt:    connection.AccessTokenExpiresAt,
			Scopes:       connection.Scopes,
			Capabilities: connection.Capabilities,
		}, nil
	}
	refreshed, accessToken, err := s.refreshCoordinated(ctx, connection)
	if err != nil {
		return AccessTokenResult{}, err
	}
	return AccessTokenResult{
		ConnectionID: refreshed.ID,
		ProviderKey:  refreshed.ProviderKey,
		AccessToken:  accessToken,
		ExpiresAt:    refreshed.AccessTokenExpiresAt,
		Scopes:       refreshed.Scopes,
		Capabilities: refreshed.Capabilities,
	}, nil
}

func (s *Service) callbackURL(providerKey string) string {
	normalizedProviderKey := providers.NormalizeKey(providerKey)
	if normalizedProviderKey == "snapchat" && strings.TrimSpace(s.cfg.SnapchatRedirectBaseURL) != "" {
		return strings.TrimRight(s.cfg.SnapchatRedirectBaseURL, "/") + "/oauth/callback/snapchat"
	}
	return strings.TrimRight(s.cfg.PublicBaseURL, "/") + "/oauth/callback/" + normalizedProviderKey
}

func (s *Service) exchangeCode(ctx context.Context, session store.ConnectSession, code, verifier string) (TokenResult, error) {
	client, ok := s.clients[session.ProviderKey]
	if !ok {
		return TokenResult{}, fmt.Errorf("provider %s does not support token exchange yet", session.ProviderKey)
	}
	return client.ExchangeCode(ctx, code, session.RedirectURI, verifier, session.Scopes, session.ProviderContext)
}

func (s *Service) persistConnection(ctx context.Context, session store.ConnectSession, token TokenResult) (store.Connection, error) {
	profile := ProviderProfile{}
	if client, ok := s.clients[session.ProviderKey]; ok {
		// Profile lookup is best-effort. Providers without a profile endpoint
		// retain the legacy connector-level reconnect behavior below.
		profile, _ = client.Profile(ctx, token.AccessToken, session.ProviderContext)
	}
	providerAccountID := providerAccountIdentity(session.ProviderKey, profile, token)
	connectionID := "conn_" + uuid.NewString()
	createdAt := s.now()
	reconnecting := false
	existing := store.Connection{}
	var err error
	if providerAccountID != "" {
		existing, err = s.repo.FindActiveConnectionByProviderAccount(ctx, session.OrganizationID, session.ConnectorType, providerAccountID)
	} else {
		existing, err = s.repo.FindActiveConnection(ctx, session.OrganizationID, session.ConnectorType)
	}
	if err == nil {
		connectionID = existing.ID
		createdAt = existing.CreatedAt
		reconnecting = true
	} else if !errors.Is(err, store.ErrNotFound) {
		return store.Connection{}, err
	}
	accessToken, err := s.vault.Encrypt(token.AccessToken, []byte(connectionID))
	if err != nil {
		return store.Connection{}, err
	}
	refreshToken := existing.EncryptedRefreshToken
	if token.RefreshToken != "" {
		refreshToken, err = s.vault.Encrypt(token.RefreshToken, []byte(connectionID))
		if err != nil {
			return store.Connection{}, err
		}
	}
	displayName := profile.DisplayName
	if displayName == "" {
		displayName = session.UserEmail
	}
	if displayName == "" {
		displayName = session.ProviderKey + " connection"
	}
	providerContext := reconnectProviderContext(existing.ProviderContext, session.ProviderContext)
	if providers.NormalizeKey(session.ProviderKey) == "notion" {
		if workspaceID := strings.TrimSpace(stringValue(token.Raw["workspace_id"])); workspaceID != "" {
			providerContext["notion_workspace_id"] = workspaceID
		}
		if workspaceName := strings.TrimSpace(stringValue(token.Raw["workspace_name"])); workspaceName != "" {
			providerContext["notion_workspace_name"] = workspaceName
		}
	}
	if providerEmail := strings.TrimSpace(profile.Email); providerEmail != "" {
		// This is the provider-confirmed identity of the connected mailbox, not
		// the Verevon user's login email. It is safe to expose as connection
		// metadata for operator navigation, while OAuth tokens remain private.
		providerContext["mailbox_address"] = providerEmail
	}
	connectionScopes := append([]string{}, session.Scopes...)
	connectionCapabilities := append([]string{}, session.Capabilities...)
	connectionStatus := "active"
	if isMetaFamilyProvider(session.ProviderKey) && token.ScopesVerified {
		connectionScopes = normalizedStrings(token.Scope)
		if provider, ok := providers.FindOAuth(session.ProviderKey); ok {
			connectionCapabilities = capabilitiesAllowedByScopes(provider, session.Capabilities, connectionScopes)
		}
		if len(connectionCapabilities) != len(session.Capabilities) {
			connectionStatus = "needs_refresh"
		}
	}
	connection := store.Connection{
		ID:                    connectionID,
		ProviderKey:           session.ProviderKey,
		ConnectorType:         session.ConnectorType,
		OrganizationID:        session.OrganizationID,
		WorkspaceID:           session.WorkspaceID,
		UserID:                session.UserID,
		UserEmail:             session.UserEmail,
		Status:                connectionStatus,
		DisplayName:           displayName,
		ProviderAccountID:     providerAccountID,
		TenantID:              profile.TenantID,
		ProviderContext:       providerContext,
		Capabilities:          connectionCapabilities,
		Scopes:                connectionScopes,
		EncryptedAccessToken:  accessToken,
		EncryptedRefreshToken: refreshToken,
		AccessTokenExpiresAt:  token.ExpiresAt,
		LastRefreshedAt:       s.now(),
		LastSyncStatus:        "pending",
		CreatedAt:             createdAt,
		UpdatedAt:             s.now(),
	}
	var saved store.Connection
	err = s.repo.WithAuditTransaction(ctx, func(tx store.AuditTransaction) error {
		var saveErr error
		if reconnecting {
			saved, saveErr = tx.ReconnectConnection(ctx, connection)
		} else {
			saved, saveErr = tx.UpsertConnection(ctx, connection)
		}
		if saveErr != nil {
			return saveErr
		}
		return tx.InsertAuditEvent(ctx, store.AuditEvent{
			ID:             "audit:integration:connection-created:" + saved.ID + ":" + session.ID,
			OrganizationID: session.OrganizationID,
			UserID:         session.UserID,
			ConnectionID:   saved.ID,
			EventType:      "connection.created",
			ProviderKey:    session.ProviderKey,
			Metadata: map[string]any{
				"requestedCapabilities": append([]string(nil), session.Capabilities...),
				"requestedScopes":       redactScopes(session.Scopes),
				"capabilities":          append([]string(nil), saved.Capabilities...),
				"scopes":                redactScopes(saved.Scopes),
			},
			CreatedAt: s.now(),
		})
	})
	if err != nil {
		return store.Connection{}, err
	}
	return saved, nil
}

func providerAccountIdentity(providerKey string, profile ProviderProfile, token TokenResult) string {
	if providers.NormalizeKey(providerKey) == "notion" {
		if workspaceID := strings.TrimSpace(stringValue(token.Raw["workspace_id"])); workspaceID != "" {
			return workspaceID
		}
	}
	return strings.TrimSpace(profile.ID)
}

func (s *Service) refresh(ctx context.Context, connection store.Connection, refreshToken string) (store.Connection, string, error) {
	client, ok := s.clients[connection.ProviderKey]
	if !ok {
		return store.Connection{}, "", fmt.Errorf("provider %s does not support refresh yet", connection.ProviderKey)
	}
	token, err := client.Refresh(ctx, refreshToken, connection.Scopes, connection.ProviderContext)
	if err != nil {
		if IsAuthorizationRefreshRequired(err) {
			connection.Status = "needs_refresh"
			if _, persistErr := s.repo.UpdateConnectionCredentials(ctx, connection); persistErr != nil {
				return store.Connection{}, "", fmt.Errorf("mark connection needs refresh: %w", errors.Join(err, persistErr))
			}
		}
		return store.Connection{}, "", err
	}
	encryptedAccessToken, err := s.vault.Encrypt(token.AccessToken, []byte(connection.ID))
	if err != nil {
		return store.Connection{}, "", err
	}
	connection.EncryptedAccessToken = encryptedAccessToken
	if token.RefreshToken != "" {
		encryptedRefreshToken, err := s.vault.Encrypt(token.RefreshToken, []byte(connection.ID))
		if err != nil {
			return store.Connection{}, "", err
		}
		connection.EncryptedRefreshToken = encryptedRefreshToken
	}
	connection.AccessTokenExpiresAt = token.ExpiresAt
	connection.LastRefreshedAt = s.now()
	connection.Status = "active"
	if isMetaFamilyProvider(connection.ProviderKey) && token.ScopesVerified {
		connection.Scopes = normalizedStrings(token.Scope)
		if provider, ok := providers.FindOAuth(connection.ProviderKey); ok {
			grantedCapabilities := capabilitiesAllowedByScopes(provider, connection.Capabilities, connection.Scopes)
			if len(grantedCapabilities) != len(connection.Capabilities) {
				connection.Status = "needs_refresh"
			}
			connection.Capabilities = grantedCapabilities
		}
	}
	saved, err := s.repo.UpdateConnectionCredentials(ctx, connection)
	if err != nil {
		return store.Connection{}, "", err
	}
	return saved, token.AccessToken, nil
}

func (s *Service) refreshCoordinated(ctx context.Context, connection store.Connection) (store.Connection, string, error) {
	return s.refreshSingleflight(ctx, connection.ID, func(ctx context.Context) (store.Connection, string, error) {
		if locker, ok := s.repo.(store.ConnectionRefreshLocker); ok {
			var refreshed store.Connection
			var accessToken string
			err := locker.WithConnectionRefreshLock(ctx, connection.ID, func(lockCtx context.Context) error {
				var refreshErr error
				refreshed, accessToken, refreshErr = s.refreshIfStillExpired(lockCtx, connection.ID)
				return refreshErr
			})
			return refreshed, accessToken, err
		}
		return s.refreshIfStillExpired(ctx, connection.ID)
	})
}

func (s *Service) refreshIfStillExpired(ctx context.Context, connectionID string) (store.Connection, string, error) {
	connection, err := s.repo.GetConnection(ctx, connectionID)
	if err != nil {
		return store.Connection{}, "", err
	}
	if connection.DeletedAt != nil || connection.Status != "active" {
		return store.Connection{}, "", store.ErrNotFound
	}
	accessToken, err := s.vault.Decrypt(connection.EncryptedAccessToken, []byte(connection.ID))
	if err != nil {
		return store.Connection{}, "", err
	}
	if s.now().Add(s.cfg.TokenRefreshSkew).Before(connection.AccessTokenExpiresAt) {
		return connection, accessToken, nil
	}
	if connection.EncryptedRefreshToken == "" {
		// A connection adopted from a Control Plane sign-in has no refresh
		// token of its own (it belongs to auth-core's Azure app); ask
		// auth-core to re-mint instead.
		if refreshed, token, applies, cpErr := s.refreshFromControlPlane(ctx, connection); applies {
			return refreshed, token, cpErr
		}
		return store.Connection{}, "", fmt.Errorf("connection has no refresh token")
	}
	refreshToken, err := s.vault.Decrypt(connection.EncryptedRefreshToken, []byte(connection.ID))
	if err != nil {
		return store.Connection{}, "", err
	}
	refreshed, token, err := s.refresh(ctx, connection, refreshToken)
	if err != nil {
		// Our own refresh token can die while the user keeps signing in
		// (public-client refresh tokens expire after 24 idle hours —
		// AADSTS70008). If Control Plane holds a live credential for this
		// account, recover with it rather than parking the connection in
		// needs_refresh until someone clicks reconnect.
		if cpRefreshed, cpToken, applies, cpErr := s.refreshFromControlPlane(ctx, connection); applies && cpErr == nil {
			return cpRefreshed, cpToken, nil
		}
		return store.Connection{}, "", err
	}
	return refreshed, token, nil
}

func reconnectProviderContext(existing, session map[string]string) map[string]string {
	merged := make(map[string]string, len(session)+1)
	for key, value := range session {
		merged[key] = value
	}
	// guild_id is a verified tenant binding. A generic reconnect may replace
	// ordinary provider-derived context (including webhook account IDs), but
	// changing the guild requires a dedicated authorized rebind flow.
	if guildID := strings.TrimSpace(existing["guild_id"]); guildID != "" {
		merged["guild_id"] = guildID
	}
	// Keep the last verified webhook binding through reconnect. Successful Meta
	// provisioning replaces it atomically; dropping it in the OAuth callback
	// creates a window where already-subscribed provider events cannot resolve.
	if accountIDs := strings.TrimSpace(existing["webhook_account_ids"]); accountIDs != "" && strings.TrimSpace(merged["webhook_account_ids"]) == "" {
		merged["webhook_account_ids"] = accountIDs
	}
	return merged
}

type refreshWork func(context.Context) (store.Connection, string, error)

func (s *Service) refreshSingleflight(ctx context.Context, connectionID string, work refreshWork) (store.Connection, string, error) {
	s.refreshMu.Lock()
	if call, ok := s.refreshes[connectionID]; ok {
		s.refreshMu.Unlock()
		select {
		case <-call.done:
			return call.connection, call.accessToken, call.err
		case <-ctx.Done():
			return store.Connection{}, "", ctx.Err()
		}
	}
	call := &refreshCall{done: make(chan struct{})}
	s.refreshes[connectionID] = call
	s.refreshMu.Unlock()

	call.connection, call.accessToken, call.err = work(ctx)
	close(call.done)

	s.refreshMu.Lock()
	delete(s.refreshes, connectionID)
	s.refreshMu.Unlock()

	return call.connection, call.accessToken, call.err
}

func isMetaFamilyProvider(providerKey string) bool {
	switch strings.TrimSpace(strings.ToLower(providerKey)) {
	case "meta", "facebook", "instagram", "whatsapp", "meta-ads":
		return true
	default:
		return false
	}
}

func normalizedStrings(values []string) []string {
	seen := map[string]struct{}{}
	for _, value := range values {
		if value = strings.TrimSpace(value); value != "" {
			seen[value] = struct{}{}
		}
	}
	out := make([]string, 0, len(seen))
	for value := range seen {
		out = append(out, value)
	}
	slices.Sort(out)
	return out
}

func capabilitiesAllowedByScopes(provider providers.Provider, requested, grantedScopes []string) []string {
	granted := map[string]struct{}{}
	for _, scope := range grantedScopes {
		granted[strings.TrimSpace(scope)] = struct{}{}
	}
	requestedSet := map[string]struct{}{}
	for _, capability := range requested {
		requestedSet[capability] = struct{}{}
	}
	allowed := make([]string, 0, len(requested))
	for _, capability := range provider.Capabilities {
		if _, wanted := requestedSet[capability.Key]; !wanted {
			continue
		}
		allGranted := true
		for _, required := range capability.Scopes {
			if _, ok := granted[required]; !ok {
				allGranted = false
				break
			}
		}
		if allGranted {
			allowed = append(allowed, capability.Key)
		}
	}
	slices.Sort(allowed)
	return allowed
}

func redactScopes(scopes []string) []string {
	out := make([]string, 0, len(scopes))
	for _, scope := range scopes {
		out = append(out, scope)
	}
	return out
}
