package org

import "time"

type Organization struct {
	ID            string         `json:"id"`
	Name          string         `json:"name"`
	Slug          string         `json:"slug,omitempty"`
	Plan          string         `json:"plan"`
	Status        string         `json:"status"`
	PrimaryDomain string         `json:"primary_domain,omitempty"`
	Region        string         `json:"region,omitempty"`
	DefaultLocale string         `json:"default_locale,omitempty"`
	Metadata      map[string]any `json:"metadata,omitempty"`
	CreatedAt     time.Time      `json:"created_at"`
	UpdatedAt     time.Time      `json:"updated_at"`
	DeletedAt     *time.Time     `json:"deleted_at,omitempty"`
	// Norwegian Enhetsregisteret verification
	OrgNumber          *string        `json:"org_number,omitempty"`
	VerificationStatus string         `json:"verification_status"` // "unverified" | "verified"
	BrregData          map[string]any `json:"brreg_data,omitempty"`
}

type Entitlement struct {
	Key       string    `json:"key"`
	Enabled   bool      `json:"enabled"`
	UpdatedAt time.Time `json:"updated_at"`
}

type Quota struct {
	OrgID       string     `json:"org_id"`
	Key         string     `json:"key"`
	Value       int64      `json:"value"`
	Limit       int64      `json:"limit"`
	ResetPeriod string     `json:"reset_period"` // 'daily', 'monthly', 'none'
	LastResetAt *time.Time `json:"last_reset_at,omitempty"`
	UpdatedAt   time.Time  `json:"updated_at"`
}

type Billing struct {
	OrgID              string         `json:"org_id"`
	BillingEmail       string         `json:"billing_email,omitempty"`
	PaymentMethodID    string         `json:"payment_method_id,omitempty"`
	SubscriptionID     string         `json:"subscription_id,omitempty"`
	SubscriptionStatus string         `json:"subscription_status"` // 'active', 'past_due', 'canceled', 'trialing'
	TrialEndsAt        *time.Time     `json:"trial_ends_at,omitempty"`
	CurrentPeriodStart *time.Time     `json:"current_period_start,omitempty"`
	CurrentPeriodEnd   *time.Time     `json:"current_period_end,omitempty"`
	AutoRenew          bool           `json:"auto_renew"`
	BillingAddress     map[string]any `json:"billing_address,omitempty"`
	TaxID              string         `json:"tax_id,omitempty"`
	CreatedAt          time.Time      `json:"created_at"`
	UpdatedAt          time.Time      `json:"updated_at"`
}

type Compliance struct {
	OrgID                 string         `json:"org_id"`
	DataResidency         string         `json:"data_residency"` // 'us', 'eu', 'asia', etc.
	GDPRCompliant         bool           `json:"gdpr_compliant"`
	HIPAACompliant        bool           `json:"hipaa_compliant"`
	SOC2Compliant         bool           `json:"soc2_compliant"`
	DataRetentionDays     int            `json:"data_retention_days,omitempty"`
	RequireMFA            bool           `json:"require_mfa"`
	IPAllowlist           map[string]any `json:"ip_allowlist,omitempty"`
	AuditLogRetentionDays int            `json:"audit_log_retention_days"`
	EncryptionAtRest      bool           `json:"encryption_at_rest"`
	EncryptionInTransit   bool           `json:"encryption_in_transit"`
	CreatedAt             time.Time      `json:"created_at"`
	UpdatedAt             time.Time      `json:"updated_at"`
}

type RoleMapping struct {
	ID          string    `json:"id"`
	OrgID       string    `json:"org_id"`
	RoleName    string    `json:"role_name"`
	Permissions []string  `json:"permissions"`
	IsCustom    bool      `json:"is_custom"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

type PlanHistory struct {
	ID           string         `json:"id"`
	OrgID        string         `json:"org_id"`
	PreviousPlan string         `json:"previous_plan,omitempty"`
	NewPlan      string         `json:"new_plan"`
	ChangedBy    string         `json:"changed_by,omitempty"`
	ChangeReason string         `json:"change_reason,omitempty"`
	ChangedAt    time.Time      `json:"changed_at"`
	Metadata     map[string]any `json:"metadata,omitempty"`
}

// OrgMember represents a user's membership in an organization.
type OrgMember struct {
	ID           string     `json:"id"`
	OrgID        string     `json:"org_id"`
	UserID       string     `json:"user_id"`
	Role         string     `json:"role"`
	Status       string     `json:"status"` // active | invited | suspended
	InvitedBy    string     `json:"invited_by,omitempty"`
	InvitedEmail string     `json:"invited_email,omitempty"`
	JoinedAt     time.Time  `json:"joined_at"`
	UpdatedAt    *time.Time `json:"updated_at,omitempty"`
}

// MemberSuggestion is the autocomplete-ready shape returned by the member search endpoint.
// Active members are enriched from user-core; invited members use the stored email.
type MemberSuggestion struct {
	UserID      string `json:"user_id"`
	DisplayName string `json:"display_name"`
	Email       string `json:"email"`
	AvatarURL   string `json:"avatar_url,omitempty"`
	Role        string `json:"role"`
	Status      string `json:"status"` // "active" | "invited"
}

// OrgTenantLink maps an external identity provider tenant to an organization.
// Used for deterministic enterprise org resolution.
type OrgTenantLink struct {
	ID                    string    `json:"id"`
	OrgID                 string    `json:"org_id"`
	Provider              string    `json:"provider"`
	MicrosoftTenantID     string    `json:"microsoft_tenant_id"`
	Verified              bool      `json:"verified"`
	Domains               []string  `json:"domains,omitempty"`
	DisplayNameFromTenant string    `json:"display_name_from_tenant,omitempty"`
	CreatedAt             time.Time `json:"created_at"`
	UpdatedAt             time.Time `json:"updated_at"`
}

// OrgOnboardingState tracks zero-input onboarding progression for each org.
type OrgOnboardingState struct {
	OrgID         string         `json:"org_id"`
	Status        string         `json:"status"` // CREATED|PROFILE_READY|CONNECTORS_PENDING|COMPLETED
	Steps         map[string]any `json:"steps,omitempty"`
	LastUpdatedAt time.Time      `json:"last_updated_at"`
}

// OrganizationWithDetails includes all related data
type OrganizationWithDetails struct {
	Organization
	Entitlements []Entitlement `json:"entitlements,omitempty"`
	Quotas       []Quota       `json:"quotas,omitempty"`
	Billing      *Billing      `json:"billing,omitempty"`
	Compliance   *Compliance   `json:"compliance,omitempty"`
	RoleMappings []RoleMapping `json:"role_mappings,omitempty"`
}

var defaultEntitlements = []Entitlement{
	{Key: "feature.chat", Enabled: true},
	{Key: "feature.api_keys", Enabled: true},
	{Key: "feature.audit_logs", Enabled: true},
	{Key: "feature.sso", Enabled: false},
}

var defaultQuotas = map[string]map[string]int64{
	"free": {
		"api_calls":  1000,
		"users":      5,
		"storage_mb": 1000,
	},
	"trial": {
		"api_calls":  1000,
		"users":      5,
		"storage_mb": 1000,
	},
	"hobby": {
		"api_calls":  10000,
		"users":      50,
		"storage_mb": 10000,
	},
	"standard": {
		"api_calls":  10000,
		"users":      50,
		"storage_mb": 10000,
	},
	"pro": {
		"api_calls":  10000,
		"users":      50,
		"storage_mb": 10000,
	},
	"enterprise": {
		"api_calls":  100000,
		"users":      -1, // unlimited
		"storage_mb": -1, // unlimited
	},
}

var defaultRolePermissions = map[string][]string{
	"owner": {
		"org:delete", "org:update", "members:invite",
		"members:remove", "billing:manage", "roles:manage",
	},
	"admin": {
		"org:update", "members:invite", "members:remove", "roles:manage",
	},
	"member": {
		"org:read", "resources:create", "resources:read", "resources:update",
	},
	"viewer": {
		"org:read", "resources:read",
	},
}
