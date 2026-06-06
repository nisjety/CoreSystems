package store

import (
	"context"
	"errors"
	"time"
)

var (
	ErrNotFound = errors.New("not found")
	ErrConflict = errors.New("conflict")
)

type ConnectSession struct {
	ID                     string
	ProviderKey            string
	ConnectorType          string
	OrganizationID         string
	WorkspaceID            string
	UserID                 string
	UserEmail              string
	StateHash              string
	CodeVerifierCiphertext string
	RedirectURI            string
	ReturnURL              string
	ProviderContext        map[string]string
	Capabilities           []string
	Scopes                 []string
	ExpiresAt              time.Time
	CreatedAt              time.Time
	ConsumedAt             *time.Time
	ErrorCode              string
	ErrorDescription       string
}

type Connection struct {
	ID                    string            `json:"id"`
	ProviderKey           string            `json:"providerKey"`
	ConnectorType         string            `json:"connectorType"`
	OrganizationID        string            `json:"organizationId"`
	WorkspaceID           string            `json:"workspaceId"`
	UserID                string            `json:"userId"`
	UserEmail             string            `json:"userEmail"`
	Status                string            `json:"status"`
	DisplayName           string            `json:"displayName,omitempty"`
	ProviderAccountID     string            `json:"providerAccountId,omitempty"`
	TenantID              string            `json:"tenantId,omitempty"`
	ProviderContext       map[string]string `json:"providerContext,omitempty"`
	Capabilities          []string          `json:"capabilities"`
	Scopes                []string          `json:"scopes"`
	EncryptedAccessToken  string            `json:"-"`
	EncryptedRefreshToken string            `json:"-"`
	AccessTokenExpiresAt  time.Time         `json:"accessTokenExpiresAt"`
	LastRefreshedAt       time.Time         `json:"lastRefreshedAt,omitempty"`
	LastSyncStatus        string            `json:"lastSyncStatus,omitempty"`
	CreatedAt             time.Time         `json:"createdAt"`
	UpdatedAt             time.Time         `json:"updatedAt"`
	DeletedAt             *time.Time        `json:"deletedAt,omitempty"`
}

type ConnectionFilter struct {
	OrganizationID string
	ProviderKey    string
	ConnectorType  string
	UserID         string
}

type ConnectionConsent struct {
	ID             string         `json:"id"`
	OrganizationID string         `json:"organizationId"`
	ConnectionID   string         `json:"connectionId"`
	UserID         string         `json:"userId"`
	ProviderKey    string         `json:"providerKey"`
	Source         string         `json:"source"`
	Purpose        string         `json:"purpose"`
	Granted        bool           `json:"granted"`
	Metadata       map[string]any `json:"metadata,omitempty"`
	ExpiresAt      *time.Time     `json:"expiresAt,omitempty"`
	CreatedAt      time.Time      `json:"createdAt"`
	UpdatedAt      time.Time      `json:"updatedAt"`
	RevokedAt      *time.Time     `json:"revokedAt,omitempty"`
}

type SyncJob struct {
	ID             string         `json:"id"`
	OrganizationID string         `json:"organizationId"`
	ConnectionID   string         `json:"connectionId"`
	UserID         string         `json:"userId"`
	ProviderKey    string         `json:"providerKey"`
	Status         string         `json:"status"`
	Reason         string         `json:"reason,omitempty"`
	Mode           string         `json:"mode,omitempty"`
	Checkpoint     map[string]any `json:"checkpoint,omitempty"`
	Metadata       map[string]any `json:"metadata,omitempty"`
	CreatedAt      time.Time      `json:"createdAt"`
	UpdatedAt      time.Time      `json:"updatedAt"`
	StartedAt      *time.Time     `json:"startedAt,omitempty"`
	CompletedAt    *time.Time     `json:"completedAt,omitempty"`
}

type SyncJobFilter struct {
	OrganizationID string
	ConnectionID   string
	ProviderKey    string
	Status         string
}

type SyncJobClaim struct {
	Consumer       string
	Target         string
	OrganizationID string
	ProviderKey    string
	Checkpoint     map[string]any
	Metadata       map[string]any
}

type SyncEvent struct {
	ID        string         `json:"id"`
	JobID     string         `json:"jobId"`
	Type      string         `json:"type"`
	Message   string         `json:"message,omitempty"`
	Metadata  map[string]any `json:"metadata,omitempty"`
	CreatedAt time.Time      `json:"createdAt"`
}

type WebhookEvent struct {
	ID             string         `json:"id"`
	OrganizationID string         `json:"organizationId,omitempty"`
	ProviderKey    string         `json:"providerKey"`
	EventType      string         `json:"eventType"`
	SignatureHash  string         `json:"signatureHash,omitempty"`
	Payload        map[string]any `json:"payload"`
	ReceivedAt     time.Time      `json:"receivedAt"`
}

type TokenLease struct {
	ID             string    `json:"id"`
	OrganizationID string    `json:"organizationId"`
	ConnectionID   string    `json:"connectionId"`
	UserID         string    `json:"userId,omitempty"`
	ProviderKey    string    `json:"providerKey"`
	ConnectorType  string    `json:"connectorType"`
	Consumer       string    `json:"consumer,omitempty"`
	ExpiresAt      time.Time `json:"expiresAt"`
	CreatedAt      time.Time `json:"createdAt"`
}

type SCIMToken struct {
	ID             string     `json:"id"`
	OrganizationID string     `json:"organizationId"`
	Name           string     `json:"name,omitempty"`
	TokenPrefix    string     `json:"tokenPrefix"`
	CreatedBy      string     `json:"createdBy,omitempty"`
	LastUsedAt     *time.Time `json:"lastUsedAt,omitempty"`
	ExpiresAt      *time.Time `json:"expiresAt,omitempty"`
	RevokedAt      *time.Time `json:"revokedAt,omitempty"`
	CreatedAt      time.Time  `json:"createdAt"`
	UpdatedAt      time.Time  `json:"updatedAt"`
}

type AuditEvent struct {
	ID             string
	OrganizationID string
	UserID         string
	ConnectionID   string
	EventType      string
	ProviderKey    string
	Metadata       map[string]any
	CreatedAt      time.Time
}

type Repository interface {
	CreateConnectSession(ctx context.Context, session ConnectSession) error
	GetConnectSessionByStateHash(ctx context.Context, stateHash string) (ConnectSession, error)
	GetConnectSessionByID(ctx context.Context, id string) (ConnectSession, error)
	MarkConnectSessionConsumed(ctx context.Context, id string, errorCode, errorDescription string) error
	UpsertConnection(ctx context.Context, connection Connection) (Connection, error)
	ListConnections(ctx context.Context, filter ConnectionFilter) ([]Connection, error)
	GetConnection(ctx context.Context, id string) (Connection, error)
	FindActiveConnection(ctx context.Context, organizationID, connectorType string) (Connection, error)
	MarkConnectionDeleted(ctx context.Context, id string) (Connection, error)
	UpdateConnectionCapabilities(ctx context.Context, id string, capabilities []string) (Connection, error)
	UpsertConnectionConsent(ctx context.Context, consent ConnectionConsent) (ConnectionConsent, error)
	ListConnectionConsents(ctx context.Context, connectionID string) ([]ConnectionConsent, error)
	CreateSyncJob(ctx context.Context, job SyncJob) (SyncJob, error)
	GetSyncJob(ctx context.Context, id string) (SyncJob, error)
	ListSyncJobs(ctx context.Context, filter SyncJobFilter) ([]SyncJob, error)
	ClaimSyncJob(ctx context.Context, claim SyncJobClaim) (SyncJob, error)
	UpdateSyncJob(ctx context.Context, job SyncJob) (SyncJob, error)
	InsertSyncEvent(ctx context.Context, event SyncEvent) error
	ListSyncEvents(ctx context.Context, jobID string) ([]SyncEvent, error)
	InsertWebhookEvent(ctx context.Context, event WebhookEvent) error
	InsertTokenLease(ctx context.Context, lease TokenLease) error
	CreateSCIMToken(ctx context.Context, token SCIMToken, tokenHash string) (SCIMToken, error)
	ListSCIMTokens(ctx context.Context, organizationID string) ([]SCIMToken, error)
	FindActiveSCIMTokenByHash(ctx context.Context, organizationID, tokenHash string) (SCIMToken, error)
	MarkSCIMTokenUsed(ctx context.Context, id string, usedAt time.Time) error
	RevokeSCIMToken(ctx context.Context, organizationID, id string, revokedAt time.Time) (SCIMToken, error)
	InsertAuditEvent(ctx context.Context, event AuditEvent) error
	Close()
}

type ConnectionRefreshLocker interface {
	WithConnectionRefreshLock(ctx context.Context, connectionID string, fn func(context.Context) error) error
}
