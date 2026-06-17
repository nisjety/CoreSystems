# Billing Core

Billing Core is the financial control plane for CoreSystem.

## Responsibilities

- Internal ownership of plans, quotas, credits, entitlements and org billing state.
- Usage ingestion from product events through NATS (`usage.>`).
- Provider adapter orchestration:
  - Invoice + metering adapter (`Lago`)
  - Payment adapter (`Hyperswitch`, with Stripe fallback)
- Uniform API so product services never call Stripe/Lago directly.

## Adapter Configuration

- `PAYMENT_PROVIDER=hyperswitch|stripe|auto`
- `HYPERSWITCH_BASE_URL`, `HYPERSWITCH_API_KEY`, `HYPERSWITCH_PUBLISHABLE_KEY`
- Optional: `HYPERSWITCH_PROFILE_ID`, `HYPERSWITCH_CLIENT_URL`, `HYPERSWITCH_BACKEND_URL`
- Stripe fallback: `STRIPE_BASE_URL`, `STRIPE_API_KEY`
- `LAGO_BASE_URL`, `LAGO_API_KEY`
- `ADAPTER_TIMEOUT_SECONDS`

When API keys are not provided, adapters run in non-charging/non-sync mode for local development.

## Run Locally

```bash
go run ./cmd/server
```

The service exposes:

- `GET /health`
- `GET /api/v1/billing/orgs/:orgId/account`
- `PUT /api/v1/billing/orgs/:orgId/account`
- `POST /api/v1/billing/orgs/:orgId/usage`
- `GET /api/v1/billing/orgs/:orgId/entitlements/:feature`
- `GET /api/v1/billing/orgs/:orgId/quotas/:metric`
- `POST /api/v1/billing/orgs/:orgId/invoices`
- `POST /api/v1/billing/orgs/:orgId/checkout-session`
- `POST /api/v1/billing/orgs/:orgId/checkout-session/confirm`

Usage ingestion supports idempotency via `event_id` and replay-safe processing for NATS events.

## Retry and Dead-Letter

- Provider failures (Stripe charge or Lago usage sync) are persisted in `billing_retry_jobs`.
- Background retry processor uses exponential backoff and configurable polling/batch size.
- After max attempts, jobs are marked `dead_letter` and emitted as `billing.dlq` events.
