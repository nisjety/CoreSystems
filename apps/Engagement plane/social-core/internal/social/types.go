package social

import (
	"context"
	"encoding/json"
	"errors"
	"time"
)

const (
	PostStatusDraft      = "draft"
	PostStatusScheduled  = "scheduled"
	PostStatusPublishing = "publishing"
	PostStatusPublished  = "published"
	PostStatusFailed     = "failed"
	PostStatusBlocked    = "blocked"
	PostStatusArchived   = "archived"

	ApprovalPending     = "pending"
	ApprovalApproved    = "approved"
	ApprovalRejected    = "rejected"
	ApprovalNotRequired = "not_required"

	CampaignStatusDraft     = "draft"
	CampaignStatusActive    = "active"
	CampaignStatusCompleted = "completed"
	CampaignStatusArchived  = "archived"

	JobStatusQueued    = "queued"
	JobStatusRunning   = "running"
	JobStatusCompleted = "completed"
	JobStatusFailed    = "failed"
	JobStatusBlocked   = "blocked"
	JobStatusCanceled  = "canceled"

	AttemptStatusQueued    = "queued"
	AttemptStatusRunning   = "running"
	AttemptStatusSucceeded = "succeeded"
	AttemptStatusFailed    = "failed"
	AttemptStatusBlocked   = "blocked"

	AccountStatusConnected    = "connected"
	AccountStatusDisconnected = "disconnected"
	AccountStatusExpired      = "expired"
	AccountStatusError        = "error"

	SubjectAccountSynced       = "verevon.application.social.account.synced"
	SubjectCampaignCreated     = "verevon.application.social.campaign.created"
	SubjectApprovalRequested   = "verevon.application.social.approval.requested"
	SubjectApprovalDecided     = "verevon.application.social.approval.decided"
	SubjectPostCreated         = "verevon.application.social.post.created"
	SubjectPostScheduled       = "verevon.application.social.post.scheduled"
	SubjectPublishJobQueued    = "verevon.application.social.publish_job.queued"
	SubjectPublishJobCompleted = "verevon.application.social.publish_job.completed"
	SubjectPublishJobFailed    = "verevon.application.social.publish_job.failed"
	SubjectPublishJobBlocked   = "verevon.application.social.publish_job.blocked"
	SubjectMetricsSnapshotted  = "verevon.application.social.metrics.snapshotted"
)

var (
	ErrInvalidInput = errors.New("invalid input")
	ErrNotFound     = errors.New("social resource not found")
	// ErrApprovalRequired is returned when a publish or schedule is attempted for
	// a post that requires human approval but has no approved approval record.
	ErrApprovalRequired = errors.New("post is not approved for publishing")
)

type Repository interface {
	ListAccounts(ctx context.Context, orgID string) ([]Account, error)
	UpsertAccounts(ctx context.Context, orgID string, accounts []Account) error
	ListCampaigns(ctx context.Context, filter ListCampaignsFilter) ([]Campaign, error)
	CreateCampaign(ctx context.Context, input CreateCampaignInput) (*Campaign, error)
	ListPosts(ctx context.Context, filter ListPostsFilter) ([]Post, error)
	CreatePost(ctx context.Context, input CreatePostInput) (*Post, error)
	ListApprovals(ctx context.Context, filter ListApprovalsFilter) ([]Approval, error)
	DecideApproval(ctx context.Context, input DecideApprovalInput) (*Approval, error)
	// PostApprovalStatus reports whether the post requires approval and whether a
	// genuine approved approval record exists for it. ErrNotFound if absent.
	PostApprovalStatus(ctx context.Context, orgID, postID string) (requiresApproval bool, approved bool, err error)
	UpdatePostSchedule(ctx context.Context, input SchedulePostInput) (*Post, error)
	EnqueuePublishJob(ctx context.Context, input EnqueuePublishInput) (*PublishJob, error)
	GetPublishJob(ctx context.Context, orgID, jobID string) (*PublishJob, error)
	ClaimDuePublishJobs(ctx context.Context, now time.Time, workerID string, limit int) ([]PublishJob, error)
	FinishPublishJob(ctx context.Context, input FinishPublishJobInput) (*PublishJob, error)
}

type AccountSource interface {
	ListSocialAccounts(ctx context.Context, orgID string) ([]Account, error)
}

type TokenBroker interface {
	AccessToken(ctx context.Context, request TokenRequest) (*TokenLease, error)
}

type ActionExecutor interface {
	ExecuteAction(ctx context.Context, request ActionRequest) (*ActionResult, error)
}

type MetricsStore interface {
	ListAccountOrgIDs(ctx context.Context) ([]string, error)
	UpsertProviderMetrics(ctx context.Context, metrics []ProviderMetric) (int, error)
	ListProviderMetrics(ctx context.Context, filter ProviderMetricsFilter) ([]ProviderMetric, error)
}

// ProviderMetricsFilter scopes a metrics read. OrgID is required; AccountID
// and SnapshotDate are optional narrowing filters (insight-core passes both,
// resolved from the metrics.snapshotted event that triggered the read).
type ProviderMetricsFilter struct {
	OrgID        string
	AccountID    string
	SnapshotDate time.Time
}

type EventPublisher interface {
	Publish(ctx context.Context, subject string, payload any) error
}

type Publisher interface {
	Publish(ctx context.Context, job PublishJob, post Post, account Account) PublishAttempt
}

type Account struct {
	ID             string         `json:"id"`
	OrgID          string         `json:"org_id"`
	ProviderKey    string         `json:"provider_key"`
	ConnectionID   string         `json:"connection_id,omitempty"`
	DisplayName    string         `json:"display_name"`
	Handle         string         `json:"handle,omitempty"`
	Status         string         `json:"status"`
	Capabilities   []string       `json:"capabilities"`
	TokenState     string         `json:"token_state"`
	TokenExpiresAt *time.Time     `json:"token_expires_at,omitempty"`
	Metadata       map[string]any `json:"metadata"`
	CreatedAt      time.Time      `json:"created_at"`
	UpdatedAt      time.Time      `json:"updated_at"`
}

type MediaRef struct {
	ID         string `json:"id,omitempty"`
	Type       string `json:"type"`
	URL        string `json:"url,omitempty"`
	StorageRef string `json:"storage_ref,omitempty"`
	AltText    string `json:"alt_text,omitempty"`
}

type SourceRef struct {
	Kind     string         `json:"kind,omitempty"`
	Label    string         `json:"label,omitempty"`
	Href     string         `json:"href,omitempty"`
	Metadata map[string]any `json:"metadata,omitempty"`
}

type Campaign struct {
	ID          string         `json:"id"`
	OrgID       string         `json:"org_id"`
	Name        string         `json:"name"`
	Brief       string         `json:"brief"`
	Goal        string         `json:"goal"`
	Status      string         `json:"status"`
	Platforms   []string       `json:"platforms"`
	StartsAt    *time.Time     `json:"starts_at,omitempty"`
	EndsAt      *time.Time     `json:"ends_at,omitempty"`
	Source      SourceRef      `json:"source"`
	Metadata    map[string]any `json:"metadata"`
	OwnerUserID string         `json:"owner_user_id,omitempty"`
	CreatedAt   time.Time      `json:"created_at"`
	UpdatedAt   time.Time      `json:"updated_at"`
}

type PlatformPreview struct {
	Platform       string   `json:"platform"`
	Mode           string   `json:"mode"`
	Content        string   `json:"content"`
	CharacterLimit int      `json:"character_limit"`
	Warnings       []string `json:"warnings"`
	MediaRequired  bool     `json:"media_required"`
}

type Post struct {
	ID               string            `json:"id"`
	OrgID            string            `json:"org_id"`
	Title            string            `json:"title"`
	Body             string            `json:"body"`
	Status           string            `json:"status"`
	Platforms        []string          `json:"platforms"`
	Media            []MediaRef        `json:"media"`
	Source           SourceRef         `json:"source"`
	Previews         []PlatformPreview `json:"previews"`
	AIContext        map[string]any    `json:"ai_context"`
	ApprovalRequired bool              `json:"approval_required"`
	ApprovalState    string            `json:"approval_state"`
	ScheduledAt      *time.Time        `json:"scheduled_at,omitempty"`
	CreatedByUserID  string            `json:"created_by_user_id,omitempty"`
	UpdatedByUserID  string            `json:"updated_by_user_id,omitempty"`
	CreatedAt        time.Time         `json:"created_at"`
	UpdatedAt        time.Time         `json:"updated_at"`
}

type Approval struct {
	ID                string         `json:"id"`
	OrgID             string         `json:"org_id"`
	PostID            string         `json:"post_id,omitempty"`
	CampaignID        string         `json:"campaign_id,omitempty"`
	State             string         `json:"state"`
	RequestedByUserID string         `json:"requested_by_user_id,omitempty"`
	RequestedOfUserID string         `json:"requested_of_user_id,omitempty"`
	DecidedByUserID   string         `json:"decided_by_user_id,omitempty"`
	DecisionReason    string         `json:"decision_reason,omitempty"`
	DueAt             *time.Time     `json:"due_at,omitempty"`
	DecidedAt         *time.Time     `json:"decided_at,omitempty"`
	Metadata          map[string]any `json:"metadata"`
	Post              *Post          `json:"post,omitempty"`
	CreatedAt         time.Time      `json:"created_at"`
	UpdatedAt         time.Time      `json:"updated_at"`
}

type PublishJob struct {
	ID                string     `json:"id"`
	OrgID             string     `json:"org_id"`
	PostID            string     `json:"post_id"`
	Status            string     `json:"status"`
	IdempotencyKey    string     `json:"idempotency_key"`
	RequestedByUserID string     `json:"requested_by_user_id,omitempty"`
	ScheduledFor      time.Time  `json:"scheduled_for"`
	LockedAt          *time.Time `json:"locked_at,omitempty"`
	LockedBy          string     `json:"locked_by,omitempty"`
	Attempts          int        `json:"attempts"`
	LastError         string     `json:"last_error,omitempty"`
	Post              *Post      `json:"post,omitempty"`
	CreatedAt         time.Time  `json:"created_at"`
	UpdatedAt         time.Time  `json:"updated_at"`
}

type PublishAttempt struct {
	ID          string         `json:"id"`
	OrgID       string         `json:"org_id"`
	JobID       string         `json:"job_id"`
	PostID      string         `json:"post_id"`
	ProviderKey string         `json:"provider_key"`
	Status      string         `json:"status"`
	Mode        string         `json:"mode"`
	Endpoint    string         `json:"endpoint,omitempty"`
	ExternalID  string         `json:"external_id,omitempty"`
	Message     string         `json:"message,omitempty"`
	Warnings    []string       `json:"warnings"`
	Response    map[string]any `json:"response"`
	AttemptedAt time.Time      `json:"attempted_at"`
	CreatedAt   time.Time      `json:"created_at"`
}

type LifecycleEvent struct {
	ID          string         `json:"id"`
	Type        string         `json:"type"`
	OrgID       string         `json:"org_id"`
	PostID      string         `json:"post_id,omitempty"`
	JobID       string         `json:"job_id,omitempty"`
	ActorUserID string         `json:"actor_user_id,omitempty"`
	Data        map[string]any `json:"data"`
	OccurredAt  time.Time      `json:"occurred_at"`
}

type TokenRequest struct {
	OrganizationID string
	ConnectionID   string
	ConnectorType  string
	Consumer       string
}

type TokenLease struct {
	ConnectionID string
	ProviderKey  string
	AccessToken  string
	ExpiresAt    time.Time
	Scopes       []string
	Capabilities []string
}

type ActionRequest struct {
	ConnectionID string
	Operation    string
	Params       map[string]any
	Body         map[string]any
}

// ActionResult mirrors integration-corev2's ExecuteResult envelope. Result is
// kept as raw JSON on purpose: provider payload schemas (Graph API, LinkedIn
// Rest.li, ...) are never assumed here — callers decode defensively.
type ActionResult struct {
	ProviderKey string          `json:"providerKey"`
	Operation   string          `json:"operation"`
	Result      json.RawMessage `json:"result"`
}

type ProviderMetric struct {
	OrgID        string         `json:"org_id"`
	AccountID    string         `json:"account_id"`
	ConnectionID string         `json:"connection_id,omitempty"`
	ProviderKey  string         `json:"provider_key"`
	MetricName   string         `json:"metric_name"`
	MetricValue  float64        `json:"metric_value"`
	Dimensions   map[string]any `json:"dimensions"`
	SnapshotDate time.Time      `json:"snapshot_date"`
}

type MetricsSnapshotSummary struct {
	Orgs     int      `json:"orgs"`
	Accounts int      `json:"accounts"`
	Metrics  int      `json:"metrics"`
	Skipped  int      `json:"skipped"`
	Failures []string `json:"failures"`
}

type ListPostsFilter struct {
	OrgID    string
	Status   string
	Platform string
	Limit    int
}

type ListCampaignsFilter struct {
	OrgID  string
	Status string
	Limit  int
}

type CreateCampaignInput struct {
	OrgID       string
	Name        string
	Brief       string
	Goal        string
	Status      string
	Platforms   []string
	StartsAt    *time.Time
	EndsAt      *time.Time
	Source      SourceRef
	Metadata    map[string]any
	ActorUserID string
}

type CreatePostInput struct {
	OrgID            string
	Title            string
	Body             string
	Platforms        []string
	Media            []MediaRef
	Source           SourceRef
	AIContext        map[string]any
	ApprovalRequired *bool
	ScheduledAt      *time.Time
	ActorUserID      string
}

type ListApprovalsFilter struct {
	OrgID      string
	State      string
	PostID     string
	CampaignID string
	Limit      int
}

type DecideApprovalInput struct {
	OrgID          string
	ApprovalID     string
	Decision       string
	DecisionReason string
	ActorUserID    string
	DecidedAt      time.Time
}

type SchedulePostInput struct {
	OrgID       string
	PostID      string
	ScheduledAt time.Time
	ActorUserID string
}

type EnqueuePublishInput struct {
	OrgID             string
	PostID            string
	IdempotencyKey    string
	RequestedByUserID string
	ScheduledFor      time.Time
}

type FinishPublishJobInput struct {
	OrgID       string
	JobID       string
	Status      string
	LastError   string
	Attempts    []PublishAttempt
	CompletedAt time.Time
}

type ScheduleResult struct {
	Post *Post       `json:"post"`
	Job  *PublishJob `json:"job"`
}
