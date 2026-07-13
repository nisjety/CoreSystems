// Package feed manages the per-user in-app notification feed backed by
// the `notification_feed_items` table. We mirror provider-submitted events
// locally so velion's /notifications page can list, count, mark-read, and
// archive. Delivery is confirmed only by a callback or reconciliation.
//
// Wire contract: matches `Notification` + `NotificationFeed` types in
// velion/src/lib/notifications/types.ts. Field names must stay snake_case.
//
// U5-2 (ui-ux-velion-gap.md §10).
package feed

import "time"

// Channel constants. Match velion's `Notification.channel` union type.
const (
	ChannelInApp = "in_app"
	ChannelEmail = "email"
	ChannelSMS   = "sms"
	ChannelPush  = "push"
)

// DeliveryStatus distinguishes provider acceptance from callback-confirmed
// delivery. A successful trigger is submitted, never delivered by inference.
const (
	DeliverySubmitted = "submitted"
	DeliveryDelivered = "delivered"
	DeliveryPending   = "pending"
	DeliveryFailed    = "failed"
)

// Notification is the public row shape served to the frontend. Field
// names match `Notification` in `velion/src/lib/notifications/types.ts`.
type Notification struct {
	ID                    string         `json:"id"`
	OrganizationID        string         `json:"organization_id"`
	RecipientID           string         `json:"recipient_id"`
	EventType             string         `json:"event_type"`
	Channel               string         `json:"channel"`
	Title                 string         `json:"title"`
	Body                  string         `json:"body"`
	CtaLabel              string         `json:"cta_label,omitempty"`
	CtaHref               string         `json:"cta_href,omitempty"`
	Payload               map[string]any `json:"payload"`
	ActorID               string         `json:"actor_id,omitempty"`
	ActorName             string         `json:"actor_name,omitempty"`
	ActorEmail            string         `json:"actor_email,omitempty"`
	ActorAvatar           string         `json:"actor_avatar,omitempty"`
	Seen                  bool           `json:"seen"`
	Read                  bool           `json:"read"`
	Archived              bool           `json:"archived"`
	DeliveryStatus        string         `json:"delivery_status"`
	SubmittedAt           time.Time      `json:"submitted_at"`
	DeliveredAt           *time.Time     `json:"delivered_at,omitempty"`
	SeenAt                *time.Time     `json:"seen_at,omitempty"`
	ReadAt                *time.Time     `json:"read_at,omitempty"`
	ArchivedAt            *time.Time     `json:"archived_at,omitempty"`
	Provider              string         `json:"provider,omitempty"`
	ProviderTransactionID string         `json:"provider_transaction_id,omitempty"`
	Source                string         `json:"source,omitempty"`
}

// Feed is the envelope returned by GET /notifications. Matches velion's
// `NotificationFeed`.
type Feed struct {
	Notifications []Notification `json:"notifications"`
	TotalCount    int            `json:"total_count"`
	HasMore       bool           `json:"has_more"`
}

// ListParams scopes the feed query.
type ListParams struct {
	OrganizationID string
	RecipientID    string
	Page           int   // zero-indexed
	Limit          int   // capped at 100
	Read           *bool // nil = all, ptr = filter
	Archived       *bool // defaults to false (hide archived)
}

// CreateParams is what we insert when a notification is dispatched.
type CreateParams struct {
	ID                    string
	OrganizationID        string
	RecipientID           string
	EventType             string
	Channel               string
	Title                 string
	Body                  string
	CtaLabel              string
	CtaHref               string
	Payload               map[string]any
	ActorID               string
	ActorName             string
	ActorEmail            string
	ActorAvatar           string
	Provider              string
	ProviderTransactionID string
	Source                string
	DeliveryStatus        string
	SubmittedAt           time.Time
	DeliveredAt           *time.Time
}
