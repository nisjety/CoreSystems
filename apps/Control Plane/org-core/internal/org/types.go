package org

import (
	"strings"
	"time"
)

// MaxSafeAuthRevision is the largest integer represented exactly by every
// Control contract runtime, including JavaScript producers.
const MaxSafeAuthRevision int64 = 9007199254740991

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
	// The organisation's own website, as its administrators gave it. Nil until one is recorded.
	WebsiteURL       *string    `json:"website_url,omitempty"`
	WebsiteUpdatedAt *time.Time `json:"website_updated_at,omitempty"`
	// RetentionPosture is a read-only projection of the org's effective
	// zero-data-retention posture, derived from Plan and Metadata by
	// withRetentionView on every organization the service returns. It is never
	// stored and never written back.
	RetentionPosture *OrgRetentionView `json:"retention_posture,omitempty"`
}

// OrgRetentionView is the org-record form of the effective ZDR posture: the
// same answer GET /internal/orgs/:orgId/retention-posture serves, computed by
// the same resolveOrgPosture, in the org record's snake_case JSON.
type OrgRetentionView struct {
	// EffectiveZDR is whether interactive traffic for the org must be zero retention.
	EffectiveZDR bool `json:"effective_zdr"`
	// Lapsed is true when the org asked for ZDR but its plan no longer entitles it.
	Lapsed bool `json:"lapsed"`
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

type PlanChange struct {
	OrgID        string
	OrgName      string
	PreviousPlan string
	NewPlan      string
	ChangedBy    string
	Reason       string
	Revision     int64
}

type PlanChangeOutboxRow struct {
	PlanChange
	Attempts int
}

const (
	// GDPRErasureAuditSubject is the producer-authoritative v2 subject consumed
	// by audit-core's durable Control Plane JetStream consumer.
	GDPRErasureAuditSubject = "verevon.audit.v2.control.org-core.erasure"
	// GDPRAuditMaxAttempts bounds poison/transient publication retries before the
	// row becomes operator-visible dead-letter state.
	GDPRAuditMaxAttempts = 8
)

type GDPRAuditEvent struct {
	EventID     string
	Subject     string
	OccurredAt  time.Time
	OrgID       string
	UserID      string
	ActorRole   string
	SubjectType string
	SubjectID   string
	Outcome     string
	Details     map[string]any
}

type GDPRAuditOutboxRow struct {
	EventID  string
	OrgID    string
	Subject  string
	Payload  map[string]any
	Attempts int
}

type GDPRAuditFlushResult struct {
	Published    int `json:"published"`
	DeadLettered int `json:"dead_lettered"`
}

// InteractiveRetentionOutboxRow is a content-free cross-plane cleanup intent.
// It is emitted only for a false-to-true ZDR transition, and its consumer may
// remove personal recovery drafts but must not erase durable support records.
type InteractiveRetentionOutboxRow struct {
	EventID  int64
	OrgID    string
	Attempts int
}

type GDPRAuditOutboxStatus struct {
	Pending         int64      `json:"pending"`
	InFlight        int64      `json:"in_flight"`
	DeadLettered    int64      `json:"dead_lettered"`
	OldestPendingAt *time.Time `json:"oldest_pending_at,omitempty"`
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

// DeletionLedgerEntry is one member's GDPR export/acknowledge checkpoint
// row from org_deletion_members (migrations/018_org_deletion_ledger.up.sql),
// captured for the 30-day soft-delete grace window opened by
// DELETE /orgs/:id/gdpr/soft-delete.
type DeletionLedgerEntry struct {
	UserID         string     `json:"user_id"`
	NotifiedAt     *time.Time `json:"notified_at,omitempty"`
	ExportedAt     *time.Time `json:"exported_at,omitempty"`
	AcknowledgedAt *time.Time `json:"acknowledged_at,omitempty"`
}

// OrgPendingDeletionReminder is the minimal shape ListOrgsNeeding7DayReminder
// and ListOrgsNeeding1DayReminder return: enough for the caller to publish
// verevon.org.deletion.reminder without a second lookup.
type OrgPendingDeletionReminder struct {
	OrgID    string    `json:"org_id"`
	OrgName  string    `json:"org_name"`
	Deadline time.Time `json:"deadline"`
}

// DeletionStatus is the shape GET /orgs/:id/gdpr/deletion/status returns:
// whether orgID is currently inside its 30-day soft-delete grace window, the
// calling member's own export/acknowledge checkpoint, and — only populated
// by the HTTP layer for an owner/admin caller (org-level or platform) —
// every member's ledger row.
type DeletionStatus struct {
	Pending      bool                  `json:"pending"`
	Deadline     *time.Time            `json:"deadline,omitempty"`
	OrgName      string                `json:"org_name"`
	MemberStatus *DeletionLedgerEntry  `json:"member_status,omitempty"`
	Members      []DeletionLedgerEntry `json:"members,omitempty"`
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

// OrganizationDomain is a separately verified company-domain claim. A Brreg
// match or website URL is not proof of domain control.
type OrganizationDomain struct {
	OrgID              string     `json:"org_id"`
	NormalizedDomain   string     `json:"normalized_domain"`
	Status             string     `json:"status"` // pending | verified | revoked
	VerificationMethod string     `json:"verification_method,omitempty"`
	VerifiedAt         *time.Time `json:"verified_at,omitempty"`
	VerifiedBy         string     `json:"verified_by,omitempty"`
	AutoInviteEnabled  bool       `json:"auto_invite_enabled"`
	CreatedAt          time.Time  `json:"created_at"`
	UpdatedAt          time.Time  `json:"updated_at"`
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

// InteractiveZdrEntitlementKey is a COMPUTED (never stored) entitlement:
// interactive Zero Data Retention is a premium privacy feature gated by plan.
// It surfaces in the entitlements response so the UI can lock/upsell it.
const InteractiveZdrEntitlementKey = "feature.zero_data_retention"

// zeroDataRetentionPlans lists the plans entitled to enable ZDR. ZDR restricts
// inference to ZDR-verified providers, so it is offered on premium tiers only;
// free/trial/hobby/standard must upgrade. Adjust here to change the paywall line.
var zeroDataRetentionPlans = map[string]bool{
	"pro":        true,
	"enterprise": true,
}

// The computed web entitlements (WS7.H, keys in web_policy.go) are derived from the
// plan like feature.zero_data_retention and are never stored. They gate what an
// organization's web policy may UNLOCK: an org can always tighten.
//
// The lines are product input, assumed here and changed in these three maps:
//   - web.robots_override (a ceiling above "respect"): enterprise only. Who may
//     actually be granted the override is a separate decision (OQ-11, platform
//     operators only by default) enforced by Quarry, not by this plan check.
//   - web.proxy_pool (a ceiling above "none"): pro and enterprise.
//   - web.paid_search (search.paid_provider stays allowed): every paid or trial
//     plan; free is the only plan without it.
var webRobotsOverridePlans = map[string]bool{
	"enterprise": true,
}

var webProxyPoolPlans = map[string]bool{
	"pro":        true,
	"enterprise": true,
}

var webPaidSearchPlans = map[string]bool{
	"trial":      true,
	"hobby":      true,
	"standard":   true,
	"pro":        true,
	"enterprise": true,
}

// PlanAllowsWebCapability reports whether a plan carries one of the computed web
// entitlement keys (WebRobotsOverrideEntitlementKey and its two siblings). An
// unknown key is not allowed.
func PlanAllowsWebCapability(plan, key string) bool {
	plan = strings.ToLower(strings.TrimSpace(plan))
	switch key {
	case WebRobotsOverrideEntitlementKey:
		return webRobotsOverridePlans[plan]
	case WebProxyPoolEntitlementKey:
		return webProxyPoolPlans[plan]
	case WebPaidSearchEntitlementKey:
		return webPaidSearchPlans[plan]
	}
	return false
}

// WebEntitlementsForPlan derives the three web entitlements from a plan.
func WebEntitlementsForPlan(plan string) WebEntitlements {
	return WebEntitlements{
		RobotsOverride: PlanAllowsWebCapability(plan, WebRobotsOverrideEntitlementKey),
		ProxyPool:      PlanAllowsWebCapability(plan, WebProxyPoolEntitlementKey),
		PaidSearch:     PlanAllowsWebCapability(plan, WebPaidSearchEntitlementKey),
	}
}

// PlanLapsePolicy names how a lapsed plan is treated when the org's stored ZDR
// intent is on but its plan no longer entitles zero data retention
// (decision OD-LAPSE): "intent_stays_zdr" keeps the org zero-retention and
// flags it lapsed. Flipping OD-LAPSE means changing this constant together
// with the matrix rows (policy.plan_lapse, o_plan_lapsed_keeps_zdr and both
// d_plan_lapsed_* rows) in zdr-posture-matrix.json.
const PlanLapsePolicy = "intent_stays_zdr"

// resolveOrgPosture is the one place the org ZDR rule lives (the matrix
// org_posture_rows with lookup ok): the effective posture is the stored intent
// alone, and the org is lapsed when it holds that intent without a plan that
// entitles it. Every reader of the posture reads this answer instead of
// recomputing it.
func resolveOrgPosture(zdrIntent, entitled bool) (effective, lapsed bool) {
	return zdrIntent, zdrIntent && !entitled
}

// withRetentionView sets org.RetentionPosture from the org's own plan and
// metadata. It derives the stored intent exactly as
// Repository.GetRetentionPostureState does (metadata.interactiveRetention.zdr
// is the JSON boolean true; anything else is not intent) and the entitlement
// from PlanAllowsZeroDataRetention. The field is a projection: it is
// recomputed on every read and never written. A nil org is left alone.
func withRetentionView(org *Organization) *Organization {
	if org == nil {
		return nil
	}
	intent := false
	if ir, ok := org.Metadata["interactiveRetention"].(map[string]any); ok {
		if v, isBool := ir["zdr"].(bool); isBool {
			intent = v
		}
	}
	effective, lapsed := resolveOrgPosture(intent, PlanAllowsZeroDataRetention(org.Plan))
	org.RetentionPosture = &OrgRetentionView{EffectiveZDR: effective, Lapsed: lapsed}
	return org
}

// RetentionPosture is the minimal, server-computed view of an organization's
// interactive-retention posture: the facts a token issuer or any other posture
// reader needs and nothing else about the organization. It is what a machine
// principal holding the org:retention:read:any scope may read
// (GET /internal/orgs/:orgId/retention-posture), so it must never grow
// organization data (name, plan, members, metadata) without a new scope.
//
//   - ZDRIntent is the org admin's stored choice
//     (metadata.interactiveRetention.zdr, written by SetInteractiveRetention).
//   - Entitled is whether the org's CURRENT plan may use zero data retention
//     (PlanAllowsZeroDataRetention), so the plan allowlist stays here and no
//     reader re-implements it.
//   - EffectiveZDR is the answer readers must use: resolveOrgPosture applied
//     to the two inputs (a lapsed plan keeps ZDR under PlanLapsePolicy).
//   - Lapsed is true when the org holds the intent without the entitlement.
//   - PlanLapsePolicy names the lapse rule that produced EffectiveZDR.
//
// The wire field names are a contract with Auth Core
// (internal/http/testdata/auth-core-org-core-retention-posture.contract.json);
// the three derived fields are additive.
type RetentionPosture struct {
	ZDRIntent       bool   `json:"zdrIntent"`
	Entitled        bool   `json:"entitled"`
	EffectiveZDR    bool   `json:"effectiveZdr"`
	Lapsed          bool   `json:"lapsed"`
	PlanLapsePolicy string `json:"planLapsePolicy"`
}

// NewRetentionPosture derives the posture from the two stored inputs.
func NewRetentionPosture(plan string, zdrIntent bool) RetentionPosture {
	return RetentionPosture{ZDRIntent: zdrIntent, Entitled: PlanAllowsZeroDataRetention(plan)}.Resolved()
}

// Resolved returns a copy whose derived fields (EffectiveZDR, Lapsed,
// PlanLapsePolicy) are recomputed from ZDRIntent and Entitled, so a reader that
// builds a posture by hand can never serve derived fields that disagree with
// the two inputs.
func (p RetentionPosture) Resolved() RetentionPosture {
	p.EffectiveZDR, p.Lapsed = resolveOrgPosture(p.ZDRIntent, p.Entitled)
	p.PlanLapsePolicy = PlanLapsePolicy
	return p
}

// Effective reports whether the org has turned zero retention on AND its plan
// still allows it: the entitlement-gated enable test, used only by the
// enable-transition signal (internal/nats/zdr_enabled_signal.go). It is NOT the
// posture answer: posture readers must read EffectiveZDR, which keeps a lapsed
// plan zero-retention.
func (p RetentionPosture) Effective() bool {
	return p.ZDRIntent && p.Entitled
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
