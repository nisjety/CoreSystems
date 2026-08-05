// Package subscribers manages the local subscriber identity cache. Each
// row in `notification_subscribers` is one user known to notification-core.
// Identity columns (email, names, phone, etc.) are kept warm so we don't
// round-trip user-core on every Novu trigger — the upstream NATS event
// stream syncs them when auth-core / user-core publish updates.
//
// U5-2 (verevon ui-ux-verevon-gap.md §10).
package subscribers

import "time"

// Subscriber mirrors a row in `notification_subscribers`. JSON tags are
// snake_case to match verevon's existing wire vocabulary.
type Subscriber struct {
	UserID           string     `json:"user_id"`
	NovuSubscriberID string     `json:"novu_subscriber_id"`
	Email            string     `json:"email"`
	Phone            string     `json:"phone,omitempty"`
	FirstName        string     `json:"first_name,omitempty"`
	LastName         string     `json:"last_name,omitempty"`
	Avatar           string     `json:"avatar,omitempty"`
	Locale           string     `json:"locale,omitempty"`
	Timezone         string     `json:"timezone,omitempty"`
	OrgID            string     `json:"org_id,omitempty"`
	Role             string     `json:"role,omitempty"`
	CreatedAt        time.Time  `json:"created_at"`
	UpdatedAt        time.Time  `json:"updated_at"`
	LastSyncedAt     *time.Time `json:"last_synced_at,omitempty"`
}

// UpsertParams is what callers pass to Service.Upsert. Empty strings on
// optional fields are treated as "no change" — repository merges with the
// stored row.
type UpsertParams struct {
	UserID           string
	NovuSubscriberID string // defaults to UserID when empty
	Email            string
	Phone            string
	FirstName        string
	LastName         string
	Avatar           string
	Locale           string
	Timezone         string
	OrgID            string
	Role             string
}

const (
	MembershipStatusActive  = "active"
	MembershipStatusRemoved = "removed"
)

type Membership struct {
	OrganizationID       string    `json:"organization_id"`
	UserID               string    `json:"user_id"`
	ProviderSubscriberID string    `json:"provider_subscriber_id"`
	Role                 string    `json:"role,omitempty"`
	Status               string    `json:"status"`
	AuthorityRevision    *int64    `json:"authority_revision,omitempty"`
	SourceEventID        string    `json:"source_event_id,omitempty"`
	OccurredAt           time.Time `json:"occurred_at"`
	UpdatedAt            time.Time `json:"updated_at"`
}

type MembershipParams struct {
	OrganizationID       string
	UserID               string
	ProviderSubscriberID string
	Role                 string
	Status               string
	AuthorityRevision    *int64
	SourceEventID        string
	OccurredAt           time.Time
}
