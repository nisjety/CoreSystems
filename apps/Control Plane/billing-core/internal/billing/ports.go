package billing

import "context"

type PaymentAdapter interface {
	ChargeInvoice(ctx context.Context, invoice Invoice) error
	EnsureCustomer(ctx context.Context, input StripeCustomerInput) (string, error)
	CreateCheckoutSession(ctx context.Context, params StripeCheckoutParams) (CheckoutSession, error)
}

type InvoiceAdapter interface {
	ReportUsage(ctx context.Context, usage UsageEvent) error
}

type EventPublisher interface {
	Publish(ctx context.Context, subject string, payload map[string]any) error
}

// SharedEventPublisher is satisfied by *nats.SharedPublisher.
// Defined here (not importing nats) to avoid an import cycle.
type SharedEventPublisher interface {
	PublishAccountUpdated(ctx context.Context, orgID, plan string, seats int)
	PublishInvoiceCreated(ctx context.Context, orgID, invoiceID string, amountCents int64, currency string)
	PublishQuotaExceeded(ctx context.Context, orgID, metric string, limit, current int64)
	PublishPlanChanged(ctx context.Context, orgID, previousPlan, newPlan string)
	// PublishPlain sends a plain NATS core message for notification-core consumption.
	PublishPlain(subject string, payload map[string]any)
}
