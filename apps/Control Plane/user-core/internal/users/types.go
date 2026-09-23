package users

import (
	"time"

	pb "github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/proto/user/v1"
	"google.golang.org/protobuf/types/known/structpb"
	"google.golang.org/protobuf/types/known/timestamppb"
)

// UserStatus represents the status of a user
type UserStatus string

const (
	UserStatusActive    UserStatus = "active"
	UserStatusInactive  UserStatus = "inactive"
	UserStatusBlocked   UserStatus = "blocked"
	UserStatusSuspended UserStatus = "suspended"
)

// User represents a user in the system
type User struct {
	ID                 string
	Email              string
	Name               string
	PasswordHash       string
	Avatar             string
	Status             UserStatus
	EmailVerified      bool
	OnboardingComplete bool
	CreatedAt          time.Time
	UpdatedAt          time.Time
	LastLoginAt        *time.Time
}

// UserProfile represents extended user profile information
type UserProfile struct {
	UserID    string
	Bio       string
	Phone     string
	Location  string
	Timezone  string
	Language  string
	Metadata  map[string]any
	UpdatedAt time.Time
}

// ToProto converts User to protobuf message
func (u *User) ToProto() *pb.User {
	pbUser := &pb.User{
		Id:            u.ID,
		Email:         u.Email,
		Name:          u.Name,
		Avatar:        u.Avatar,
		Status:        statusToProto(u.Status),
		EmailVerified: u.EmailVerified,
		CreatedAt:     timestamppb.New(u.CreatedAt),
		UpdatedAt:     timestamppb.New(u.UpdatedAt),
	}

	if u.LastLoginAt != nil {
		pbUser.LastLoginAt = timestamppb.New(*u.LastLoginAt)
	}

	return pbUser
}

// ToProto converts UserProfile to protobuf message
func (p *UserProfile) ToProto() *pb.UserProfile {
	return &pb.UserProfile{
		UserId:    p.UserID,
		Bio:       p.Bio,
		Phone:     p.Phone,
		Location:  p.Location,
		Timezone:  p.Timezone,
		Language:  p.Language,
		UpdatedAt: timestamppb.New(p.UpdatedAt),
	}
}

// statusToProto converts UserStatus to protobuf enum
func statusToProto(status UserStatus) pb.UserStatus {
	switch status {
	case UserStatusActive:
		return pb.UserStatus_USER_STATUS_ACTIVE
	case UserStatusInactive:
		return pb.UserStatus_USER_STATUS_INACTIVE
	case UserStatusBlocked:
		return pb.UserStatus_USER_STATUS_BLOCKED
	case UserStatusSuspended:
		return pb.UserStatus_USER_STATUS_SUSPENDED
	default:
		return pb.UserStatus_USER_STATUS_UNSPECIFIED
	}
}

// CreateUserParams contains parameters for creating a user
type CreateUserParams struct {
	Email    string
	Name     string
	Password string
	Avatar   string
}

// UpdateUserParams contains parameters for updating a user
type UpdateUserParams struct {
	ID     string
	Name   *string
	Avatar *string
	Email  *string
}

// ListUsersParams contains parameters for listing users
type ListUsersParams struct {
	Page   int
	Limit  int
	Status *UserStatus
}

// UpdateProfileParams contains parameters for updating a user profile
type UpdateProfileParams struct {
	UserID   string
	Bio      *string
	Phone    *string
	Location *string
	Timezone *string
	Language *string
	Metadata map[string]any
}

// ============================================
// USER SETTINGS TYPES
// ============================================

// UserSettings represents a single per-category settings row
type UserSettings struct {
	ID        string
	UserID    string
	Category  string
	Settings  map[string]any
	CreatedAt time.Time
	UpdatedAt time.Time
}

// UpsertSettingsParams contains parameters for upserting settings
type UpsertSettingsParams struct {
	UserID   string
	Category string
	Settings map[string]any
}

// ============================================
// PROVIDER ACCOUNT TYPES
// ============================================

// ProviderAccount represents a linked OAuth provider identity for a user.
// Each sign-in provider (microsoft, google, github, etc.) creates one row.
type ProviderAccount struct {
	ID                string
	UserID            string
	Provider          string   // "microsoft", "google", "github", etc.
	ProviderUserID    string   // stable ID from the provider
	TenantID          string   // Microsoft AAD tenant / Google Workspace domain
	MicrosoftTenantID string   // canonical Entra tenant id for enterprise routing
	Email             string   // provider-side email
	EmailFromProvider string   // preserved source email from provider profile
	DisplayName       string   // name as returned by provider
	ScopesGranted     []string // granted OAuth scopes snapshot
	TokenRef          string   // opaque reference to token storage in auth-core
	LastSyncedAt      *time.Time
	Metadata          map[string]any // extra provider claims: job title, dept, photo, etc.
	CreatedAt         time.Time
	UpdatedAt         time.Time
}

// UpsertProviderAccountParams contains data from a social sign-in callback
type UpsertProviderAccountParams struct {
	UserID            string
	Provider          string
	ProviderUserID    string
	TenantID          string
	MicrosoftTenantID string
	Email             string
	EmailFromProvider string
	DisplayName       string
	ScopesGranted     []string
	TokenRef          string
	Metadata          map[string]any
}

// UserOrgMembership tracks a user's membership and role in an organization.
// This supports idempotent membership ensures during enterprise auto-provisioning.
type UserOrgMembership struct {
	ID        string
	UserID    string
	OrgID     string
	Role      string // owner|admin|member|viewer
	Status    string // active|invited|pending|suspended
	InvitedBy string
	CreatedAt time.Time
	UpdatedAt time.Time
}

// EnsureMembershipParams contains data for idempotent upsert of user-org membership.
type EnsureMembershipParams struct {
	UserID string
	OrgID  string
	Role   string
	Status string
}

// SessionContext is used by frontend post-login routing to decide where user lands.
type SessionContext struct {
	UserID           string `json:"userId"`
	OrgID            string `json:"orgId,omitempty"`
	Role             string `json:"role,omitempty"`
	OnboardingStatus string `json:"onboardingStatus"`
}

// ============================================
// ACTIVITY LOG TYPES
// ============================================

// ActivityLog represents a user activity log entry
type ActivityLog struct {
	ID        string
	UserID    string
	Action    string
	Resource  string
	Details   map[string]any
	IPAddress string
	UserAgent string
	CreatedAt time.Time
}

// LogActivityParams contains parameters for logging a user activity
type LogActivityParams struct {
	UserID    string
	Action    string
	Resource  string
	Details   map[string]any
	IPAddress string
	UserAgent string
}

// ============================================
// API KEY TYPES
// ============================================

// APIKey represents a user-managed API key stored in user_api_keys.
type APIKey struct {
	ID          string
	UserID      string
	Name        string
	Description string
	KeyPrefix   string // displayed prefix, e.g. "sk_a1b2c3d4"
	Scopes      []string
	ExpiresAt   *time.Time
	RevokedAt   *time.Time
	LastUsedAt  *time.Time
	CreatedAt   time.Time
}

// CreateAPIKeyParams contains data for creating a new API key.
type CreateAPIKeyParams struct {
	UserID      string
	Name        string
	Description string
	Scopes      []string
	ExpiresAt   *time.Time
}

// APIKeyCreateResult bundles the stored APIKey with the one-time plaintext key.
type APIKeyCreateResult struct {
	Key    *APIKey
	RawKey string // plaintext — return to caller once, never stored
}

// ToProto converts ActivityLog to protobuf message
func (a *ActivityLog) ToProto() *pb.Activity {
	act := &pb.Activity{
		Id:        a.ID,
		UserId:    a.UserID,
		Action:    a.Action,
		Resource:  a.Resource,
		IpAddress: a.IPAddress,
		UserAgent: a.UserAgent,
		CreatedAt: timestamppb.New(a.CreatedAt),
	}
	if len(a.Details) > 0 {
		if s, err := structpb.NewStruct(a.Details); err == nil {
			act.Details = s
		}
	}
	return act
}
