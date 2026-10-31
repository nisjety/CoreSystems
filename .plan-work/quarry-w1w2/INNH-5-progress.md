# INNH-5 progress (lane b, branch innh/lane-b)

Per unit; the status file is INNH-5-status.md. A cut-off run continues from the tree and this list.

- [x] U1 control tenancy + API: migration 025, WebhooksStore / delivery store (mem + pg), org-scoped fanout (empty org = nobody),
      dispatcher org check + blocked-target dlq, /v1/webhook-subscriptions routes (create/list/get/patch/delete/rotate/deliveries/retry),
      legacy /v1/webhooks + /v1/webhook-deliveries -> 410, GDPR purge, delivery retention sweep, control flag
      QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS (default off). Go tests green, pg tests green on a scratch postgres:16.
- [x] U2 edge passthrough routes behind QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS (default off), ZDR refusal on create, ZDR matrix row + golden. Rust tests green in Docker lane b (b39ff7d02).
- [x] U3 docs: openapi, CONTRACTS section 4, CHANGE_TRACKING, ERROR_REASONS, RUNBOOKS, docs-contract test; status file. Node tests 50 green.
- Item done in tests. Not verified: end-to-end with a real receiver / stack.
