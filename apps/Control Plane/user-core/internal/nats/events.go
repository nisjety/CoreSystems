package nats

import "time"

// Event types from auth-service
const (
	SubjectAuthUserProviderLinked        = "auth.user.provider_linked"
	SubjectAuthOrganizationMemberAdded   = "auth.organization.member_added"
	SubjectAuthOrganizationMemberRemoved = "auth.organization.member_removed"

	// Auth event subjects
	SubjectAuthUserRegistered     = "auth.user.registered"
	SubjectAuthUserLogin          = "auth.user.login"
	SubjectAuthUserLogout         = "auth.user.logout"
	SubjectAuthUserProfileUpdated = "auth.user.profile_updated"
	SubjectAuthSessionCreated     = "auth.session.created"
	SubjectAuthSessionEnded       = "auth.session.ended"

	// User service event subjects
	SubjectUserCreated               = "user.created"
	SubjectUserUpdated               = "user.updated"
	SubjectUserDeleted               = "user.deleted"
	SubjectUserBlocked               = "user.blocked"
	SubjectUserUnblocked             = "user.unblocked"
	SubjectUserSuspended             = "user.suspended"
	SubjectUserUnsuspended           = "user.unsuspended"
	SubjectUserActivated             = "user.activated"
	SubjectUserDeactivated           = "user.deactivated"
	SubjectProfileUpdated            = "user.profile.updated"
	SubjectSessionCreated            = "user.session.created"
	SubjectSessionInvalidated        = "user.session.invalidated"
	SubjectActivityLogged            = "user.activity.logged"
	SubjectRoleAssigned              = "user.role.assigned"
	SubjectRoleRemoved               = "user.role.removed"
	SubjectDeviceRegistered          = "user.device.registered"
	SubjectDeviceDeactivated         = "user.device.deactivated"
	SubjectOrganizationMemberAdded   = "organization.member.added"
	SubjectOrganizationMemberRemoved = "organization.member.removed"

	// Stream names
	StreamAuthEvents = "AUTH_EVENTS"
	StreamUserEvents = "USER_EVENTS"
)

// UserRegisteredEvent from auth-service
type UserRegisteredEvent struct {
	Type              string                 `json:"type"`
	UserID            string                 `json:"userId"`
	Email             string                 `json:"email"`
	Name              string                 `json:"name,omitempty"`
	Provider          string                 `json:"provider"`
	EmailVerified     bool                   `json:"emailVerified"`
	TenantID          string                 `json:"tenantId,omitempty"`
	MicrosoftTenantID string                 `json:"microsoftTenantId,omitempty"`
	EmailFromProvider string                 `json:"emailFromProvider,omitempty"`
	ScopesGranted     []string               `json:"scopesGranted,omitempty"`
	TokenRef          string                 `json:"tokenRef,omitempty"`
	ProfileHints      *ProviderProfileHints  `json:"profileHints,omitempty"`
	Metadata          map[string]interface{} `json:"metadata,omitempty"`
	Timestamp         time.Time              `json:"timestamp"`
}

// UserLoginEvent from auth-service
type UserLoginEvent struct {
	Type       string    `json:"type"`
	UserID     string    `json:"userId"`
	Email      string    `json:"email"`
	SessionID  string    `json:"sessionId"`
	DeviceInfo string    `json:"deviceInfo,omitempty"`
	IPAddress  string    `json:"ipAddress,omitempty"`
	UserAgent  string    `json:"userAgent,omitempty"`
	Provider   string    `json:"provider,omitempty"`
	Timestamp  time.Time `json:"timestamp"`
}

// UserLogoutEvent from auth-service
type UserLogoutEvent struct {
	Type      string    `json:"type"`
	UserID    string    `json:"userId"`
	Email     string    `json:"email"`
	SessionID string    `json:"sessionId"`
	Reason    string    `json:"reason,omitempty"` // manual, timeout, force
	Timestamp time.Time `json:"timestamp"`
}

// UserProfileUpdatedEvent from auth-service
type UserProfileUpdatedEvent struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"userId"`
	Email     string                 `json:"email"`
	Changes   map[string]interface{} `json:"changes"`
	Timestamp time.Time              `json:"timestamp"`
}

// UserProviderLinkedEvent from auth-service — fired when an existing user links a new OAuth provider
type UserProviderLinkedEvent struct {
	Type              string                `json:"type"`
	UserID            string                `json:"userId"`
	Email             string                `json:"email"`
	Provider          string                `json:"provider"`
	ProviderAccountID string                `json:"providerAccountId,omitempty"`
	TenantID          string                `json:"tenantId,omitempty"`
	MicrosoftTenantID string                `json:"microsoftTenantId,omitempty"`
	EmailFromProvider string                `json:"emailFromProvider,omitempty"`
	ScopesGranted     []string              `json:"scopesGranted,omitempty"`
	TokenRef          string                `json:"tokenRef,omitempty"`
	ProfileHints      *ProviderProfileHints `json:"profileHints,omitempty"`
	Timestamp         time.Time             `json:"timestamp"`
}

type ProviderProfileHints struct {
	DisplayName string `json:"displayName,omitempty"`
	Avatar      string `json:"avatar,omitempty"`
	Locale      string `json:"locale,omitempty"`
	Timezone    string `json:"timezone,omitempty"`
}

// SessionCreatedEvent from auth-service
type SessionCreatedEvent struct {
	Type       string    `json:"type"`
	SessionID  string    `json:"sessionId"`
	UserID     string    `json:"userId"`
	Email      string    `json:"email"`
	DeviceInfo string    `json:"deviceInfo,omitempty"`
	IPAddress  string    `json:"ipAddress,omitempty"`
	UserAgent  string    `json:"userAgent,omitempty"`
	ExpiresAt  time.Time `json:"expiresAt"`
	Timestamp  time.Time `json:"timestamp"`
}

// SessionEndedEvent from auth-service
type SessionEndedEvent struct {
	Type      string    `json:"type"`
	SessionID string    `json:"sessionId"`
	UserID    string    `json:"userId"`
	Email     string    `json:"email"`
	Reason    string    `json:"reason"` // logout, expired, revoked
	Timestamp time.Time `json:"timestamp"`
}

type OrganizationMembershipEvent struct {
	OrganizationID   string
	OrganizationName string
	UserID           string
	UserEmail        string
	Role             string
}

// User service outgoing events

// UserCreatedEvent published by user-service
type UserCreatedEvent struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"userId"`
	Email     string                 `json:"email"`
	Name      string                 `json:"name"`
	Status    string                 `json:"status"`
	Metadata  map[string]interface{} `json:"metadata,omitempty"`
	Timestamp time.Time              `json:"timestamp"`
}

// UserUpdatedEvent published by user-service
type UserUpdatedEvent struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"userId"`
	Email     string                 `json:"email"`
	Changes   map[string]interface{} `json:"changes"`
	Timestamp time.Time              `json:"timestamp"`
}

// UserDeletedEvent published by user-service
type UserDeletedEvent struct {
	Type      string    `json:"type"`
	UserID    string    `json:"userId"`
	Email     string    `json:"email"`
	Timestamp time.Time `json:"timestamp"`
}

// UserStatusChangedEvent published by user-service
type UserStatusChangedEvent struct {
	Type      string    `json:"type"`
	UserID    string    `json:"userId"`
	Email     string    `json:"email"`
	OldStatus string    `json:"oldStatus"`
	NewStatus string    `json:"newStatus"`
	Reason    string    `json:"reason,omitempty"`
	Timestamp time.Time `json:"timestamp"`
}

// UserProfileUpdatedEventOut published by user-service
type UserProfileUpdatedEventOut struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"userId"`
	Changes   map[string]interface{} `json:"changes"`
	Timestamp time.Time              `json:"timestamp"`
}

// UserSessionCreatedEvent published by user-service
type UserSessionCreatedEvent struct {
	Type       string    `json:"type"`
	SessionID  string    `json:"sessionId"`
	UserID     string    `json:"userId"`
	DeviceInfo string    `json:"deviceInfo,omitempty"`
	IPAddress  string    `json:"ipAddress,omitempty"`
	UserAgent  string    `json:"userAgent,omitempty"`
	ExpiresAt  time.Time `json:"expiresAt"`
	Timestamp  time.Time `json:"timestamp"`
}

// UserActivityLoggedEvent published by user-service
type UserActivityLoggedEvent struct {
	Type      string                 `json:"type"`
	UserID    string                 `json:"userId"`
	Action    string                 `json:"action"`
	Resource  string                 `json:"resource,omitempty"`
	Details   map[string]interface{} `json:"details,omitempty"`
	IPAddress string                 `json:"ipAddress,omitempty"`
	UserAgent string                 `json:"userAgent,omitempty"`
	Timestamp time.Time              `json:"timestamp"`
}

// UserRoleAssignedEvent published by user-service
type UserRoleAssignedEvent struct {
	Type      string    `json:"type"`
	UserID    string    `json:"userId"`
	RoleID    string    `json:"roleId"`
	RoleName  string    `json:"roleName"`
	Timestamp time.Time `json:"timestamp"`
}

// UserRoleRemovedEvent published by user-service
type UserRoleRemovedEvent struct {
	Type      string    `json:"type"`
	UserID    string    `json:"userId"`
	RoleID    string    `json:"roleId"`
	RoleName  string    `json:"roleName"`
	Timestamp time.Time `json:"timestamp"`
}

// UserDeviceRegisteredEvent published by user-service
type UserDeviceRegisteredEvent struct {
	Type       string    `json:"type"`
	UserID     string    `json:"userId"`
	DeviceID   string    `json:"deviceId"`
	DeviceName string    `json:"deviceName"`
	DeviceType string    `json:"deviceType"`
	Timestamp  time.Time `json:"timestamp"`
}
