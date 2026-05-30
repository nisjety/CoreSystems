package billing

import "time"

type SubscriptionState string
type RetryJobKind string
type RetryJobStatus string

const (
	SubscriptionStateTrialing SubscriptionState = "trialing"
	SubscriptionStateActive   SubscriptionState = "active"
	SubscriptionStatePastDue  SubscriptionState = "past_due"
	SubscriptionStateCanceled SubscriptionState = "canceled"
)

const (
	RetryJobKindLagoUsage    RetryJobKind = "lago_usage_report"
	RetryJobKindStripeCharge RetryJobKind = "stripe_invoice_charge"
)

const (
	RetryJobStatusPending    RetryJobStatus = "pending"
	RetryJobStatusProcessing RetryJobStatus = "processing"
	RetryJobStatusSucceeded  RetryJobStatus = "succeeded"
	RetryJobStatusDeadLetter RetryJobStatus = "dead_letter"
)

type Account struct {
	OrgID              string                 `json:"org_id"`
	Plan               string                 `json:"plan"`
	SubscriptionState  SubscriptionState      `json:"subscription_state"`
	Credits            int64                  `json:"credits"`
	Products           map[string]bool        `json:"products"`
	FeatureFlags       map[string]bool        `json:"feature_flags"`
	Entitlements       map[string]bool        `json:"entitlements"`
	QuotaLimits        map[string]float64     `json:"quota_limits"`
	ProviderCustomerID map[string]string      `json:"provider_customer_id"`
	Metadata           map[string]interface{} `json:"metadata"`
	UpdatedAt          time.Time              `json:"updated_at"`
	CreatedAt          time.Time              `json:"created_at"`
}

type UsageEvent struct {
	EventID    string                 `json:"event_id"`
	OrgID      string                 `json:"org_id"`
	Metric     string                 `json:"metric"`
	Quantity   float64                `json:"quantity"`
	Source     string                 `json:"source"`
	OccurredAt time.Time              `json:"occurred_at"`
	Metadata   map[string]interface{} `json:"metadata"`
}

type QuotaStatus struct {
	OrgID       string  `json:"org_id"`
	Metric      string  `json:"metric"`
	Limit       float64 `json:"limit"`
	Used        float64 `json:"used"`
	Remaining   float64 `json:"remaining"`
	IsExceeded  bool    `json:"is_exceeded"`
	Utilization float64 `json:"utilization"`
}

type StripeCustomerInput struct {
	OrgID              string
	OrganizationName   string
	BillingEmail       string
	ExistingCustomerID string
	Metadata           map[string]string
}

type StripeCheckoutParams struct {
	OrgID        string
	Plan         string
	CustomerID   string
	SuccessURL   string
	CancelURL    string
	Organization string
	Metadata     map[string]string
}

type CheckoutSession struct {
	ID  string `json:"id"`
	URL string `json:"url"`
}

type Invoice struct {
	InvoiceID    string                 `json:"invoice_id"`
	OrgID        string                 `json:"org_id"`
	Provider     string                 `json:"provider"`
	AmountCents  int64                  `json:"amount_cents"`
	Currency     string                 `json:"currency"`
	Status       string                 `json:"status"`
	IssuedAt     time.Time              `json:"issued_at"`
	DueAt        time.Time              `json:"due_at"`
	Metadata     map[string]interface{} `json:"metadata"`
	CreatedAt    time.Time              `json:"created_at"`
	LastModified time.Time              `json:"last_modified"`
}

type RetryJob struct {
	ID            int64                  `json:"id"`
	Kind          RetryJobKind           `json:"kind"`
	DedupeKey     string                 `json:"dedupe_key"`
	Payload       map[string]interface{} `json:"payload"`
	Status        RetryJobStatus         `json:"status"`
	AttemptCount  int                    `json:"attempt_count"`
	NextAttemptAt time.Time              `json:"next_attempt_at"`
	LastError     string                 `json:"last_error"`
	CreatedAt     time.Time              `json:"created_at"`
	UpdatedAt     time.Time              `json:"updated_at"`
}
