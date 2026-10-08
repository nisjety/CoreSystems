# Tenant metering, cost attribution, and prompt privacy

## Decision

Use Cost Core as Verevon's tenant-scoped usage ledger and billing projection. Do not make a single Azure subscription, resource, or Foundry project the customer identity. The authenticated organization is the billing tenant; Azure is a provider/source of cost evidence.

The request ledger and Azure invoice answer different questions:

- **Verevon usage:** which authenticated org and user invoked which provider/model, when, and with what token counts. Record this at the Model Gateway boundary, using verified token claims and an idempotent request key. Never accept org/user attribution from arbitrary client metadata.
- **Provider actual:** what Azure billed for a meter/project/date/currency. Attribute it to an org only where an Azure Foundry project is explicitly and authoritatively assigned to that org. A shared project/resource invoice remains shared.
- **Showback/chargeback:** if an org needs a share of a shared resource invoice, calculate it from recorded request usage using a documented allocation policy and reconcile allocations to the invoice. Label it as allocated. Do not report a token-price estimate as Azure actual cost.
- **Subscription allowance:** report Codex/ChatGPT subscription usage from its provider allowance snapshot separately from token-priced API costs and Azure invoice actuals.

## Tenant isolation contract

1. Model Gateway derives `org_id` and `user_id` from the verified Control Plane identity token. The Cost Core record path validates those claims and rejects cross-scope records. Tenant scope is immutable through provider routing and usage publication.
2. Cost reads always include the authenticated org. A user principal is additionally pinned to its own user ID; org-wide views require an authorized org role/service scope. No caller-supplied org filter can widen the verified scope.
3. Provider calls use server-side credentials and are proxied through Model Gateway. Do not put prompts, completions, API keys, or raw provider payloads in the Cost Core ledger, analytics dimensions, logs, or Azure billing metadata. Keep only accounting dimensions such as org/user IDs, request ID, provider/model, token counts, price version, cost basis, and timestamps.
4. Route a tenant through a tenant-assigned Foundry project where Azure supports project-level attribution. Resolve that assignment from authoritative tenant/provider configuration, not from a user-editable request field. Keep a separate mapping of Foundry project ID/tag to canonical org ID for imported billing evidence. A project/tag must never be mapped to multiple orgs.
5. Enforce the same org scope for budgets, limits, meter reads, exports, and deletion. Cross-tenant integration tests must prove that identical time windows and dimensions never return another org's rows.

## Metering model to borrow from Polar

Adopt the product concept, not Polar's immutable-event storage rule or hosted billing boundary:

- A normalized usage event is the source of truth; meters aggregate events by tenant, user, provider/model, metric, and billing period.
- Events have a stable idempotency key and explicit unit/basis. Preserve the existing distinctions between metered API usage and subscription usage.
- Version prices and meter definitions so historical totals can be explained after rates change. Keep usage quantity and financial amount distinct.
- Use one tenant-scoped meter projection for Insights, budgets, and future invoices. Report late or incomplete source data as coverage state, not as zero spend.
- Support correction/reconciliation records and audit who changed a rate or allocation rule. Do not mutate provider evidence silently.
- Retain the ability to erase/anonymize tenant/user attribution under the applicable retention policy. Polar documents that ingested usage events cannot be edited or deleted; that immutability cannot be copied literally where erasure applies.

Polar's public usage model is events → meters → metered prices → invoice lines. It also describes live per-customer meter state, while warning that ingested events are immutable and cannot be deleted. The useful Verevon analogue is Cost Core + tenant-scoped meters + Control Plane billing, not a second customer/billing authority.

## Privacy boundary and Venice-inspired behavior

Verevon can implement the proxy/no-app-storage portion of Venice's published design: send prompts from the client to the authenticated Model Gateway, keep provider credentials server-side, avoid storing prompt/response content in the Cost Core and operational logs, and apply explicit request retention modes. This is not end-to-end encryption: Verevon's gateway and the selected model provider process plaintext to perform inference.

For Azure models sold by Microsoft, Microsoft's published data documentation says prompts/completions are stateless for inference and are not used to train base models. However, default abuse monitoring may select flagged prompts/completions for storage and human review. Modified Abuse Monitoring requires Microsoft approval; eligible subscriptions can verify `ContentLogging=false`. ZDR therefore requires both (a) application-side no-persistence/logging and (b) a verified eligible Azure provider configuration. Never label an Azure route “zero retention” solely because our application does not save the prompt. Preview/model-marketplace deployments and other providers may have different terms.

Do not claim Venice's TEE/E2EE protections for ordinary Azure proxy inference. Those require different cryptographic and hardware-isolation properties. Present per-route privacy capabilities accurately and reject a strict ZDR request if no provider route satisfies the policy.

## Erasure and retention

The organization erasure fan-out must purge all Cost Core rows that carry that org ID, including usage, pricing/billing projections, and attribution mappings. User erasure must delete or anonymize user-linked ledger dimensions without deleting an entire organization. Azure invoice evidence may have separate financial-retention requirements; preserve it only under an approved retention policy and remove the tenant association when the tenant mapping is erased. Erasure tests must cover both ledger and invoice-attribution tables.

Prompt content must not enter billing telemetry. Verify this at the gateway event producer and consumer, not only in the UI. Audit logs should record policy decisions and request identifiers, not content.

## Current implementation status and gates

- Cost Core already stores token/cost entries with org and user dimensions, derives authenticated scope at its HTTP boundary, and supports organization-wide aggregates only for authorized principals.
- Azure ActualCost ingestion is separate from request usage and supports a multi-project-to-org map. It cannot split one shared Azure project into orgs. Project-level chargeback availability depends on the Azure model/product path.
- The local Azure importer now queries `Core-Ai-RG` and `rg-Cloude-ai` under Cost Management Reader. The first successful 90-day sync imported 462 provider rows, but none has an authoritative tenant mapping: `core-ai` is shared and the other rows are untagged. Keep them out of tenant totals. Do not map the shared project to Aquatiq; first route each organization through an isolated Foundry project and enforce that assignment at Model Gateway.
- Before showing per-org Azure billed actuals, prove tenant-to-project routing and reconcile imported rows against the Azure Cost Management view. Until then show tenant request metering and shared Azure actuals separately.
- Before promising Azure ZDR, verify the deployed model route, `ContentLogging=false` for the exact Azure resources/subscription, no content-bearing application logs, and ZDR behavior through every fallback/retry path.
- Before charging customers, product/finance must approve rate cards, shared-capacity allocation rules, invoice authority, rounding, corrections, and how refunds/credits are represented.

## References

- Polar usage-based billing: https://polar.sh/features/usage-billing
- Polar event ingestion and its immutability behavior: https://docs.polar.sh/features/usage-based-billing/event-ingestion
- Microsoft Foundry cost allocation: https://learn.microsoft.com/en-us/azure/foundry/concepts/manage-costs
- Microsoft Foundry model data/privacy behavior: https://learn.microsoft.com/en-us/azure/foundry/responsible-ai/openai/data-privacy
- Venice privacy architecture and modes: https://venice.ai/privacy
