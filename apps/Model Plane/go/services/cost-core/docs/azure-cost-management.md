# Azure actual cost reporting

Cost Core can import daily Azure Cost Management `ActualCost` rows for a configured subscription or resource-group scope. It keeps these rows separate from the per-request ledger: Cost Core provides user-level request attribution and estimates, while Azure provides delayed billing actuals grouped by Foundry `project` tag and meter.

## Setup

Set these variables on the `cost-core` container:

- `AZURE_COST_TENANT_ID`: Microsoft Entra tenant that owns the Azure subscription.
- `AZURE_COST_CLIENT_ID` and `AZURE_COST_CLIENT_SECRET`: service principal credentials.
- `AZURE_COST_SCOPE`: a single subscription or resource-group scope, or `AZURE_COST_SCOPES_JSON`: a JSON array of multiple scopes, e.g. `["/subscriptions/<sub>/resourceGroups/<resource-group-a>","/subscriptions/<sub>/resourceGroups/<resource-group-b>"]`. Configure only one of these. The Cost Management Query API does not accept an individual Cognitive Services account scope.
- `AZURE_COST_PROJECT_ORG_MAP_JSON`: optional JSON object mapping each Azure Foundry `project` tag value to its canonical Verevon organization ID, for example `{"tenant-a-project":"org_a","tenant-b-project":"org_b"}`. This supports multiple tenants. A project value can map to only one org; a shared project cannot be used to infer which org made an individual request. When no authoritative tenant/project mapping exists yet, omit this variable so imported charges remain unassigned.

Grant the service principal **Cost Management Reader** only at the narrowest scopes that contain the billing data to import. Configure multiple scopes with `AZURE_COST_SCOPES_JSON` when needed; do not widen to the subscription just to combine queries. The Cost Management Query API returns tag groupings as `TagKey`/`TagValue`; only rows whose key is exactly `project` are treated as Foundry project identifiers. Missing project tags and other tag keys remain unassigned. Rows without a meter label are retained under `Unspecified meter`.

Foundry project-level chargeback uses the `project` tag on supported Azure-sold model usage; it is not a generic tag that can reliably split every shared meter. Keep the mapping aligned with tenant-to-Foundry-project assignments enforced by Model Gateway. A shared project must not be mapped to one organization. The importer is disabled when auth/scope settings are absent and fails startup on partial or invalid configuration. It imports the last 90 days at startup and refreshes every six hours; transient throttling uses bounded request retries and a backoff retry schedule. Successful refreshes atomically replace that scope's date window so Azure billing corrections do not leave stale meter rows.

## Reading the numbers

The Insights page exposes mapped Azure actuals only to active organization owners and admins. It displays totals by billing currency and the newest usage date. Rows without an authoritative project mapping remain unassigned and are excluded from tenant totals. If multiple orgs send requests through one shared project/resource, Azure's invoice is a shared platform actual and must not be copied into each org's total. Show per-org request metering separately; reconcile any allocated amount to the shared invoice and label it as an allocation, not an Azure-reported tenant actual. Azure billing can lag; the date shown is the latest source usage date, not a real-time timestamp.

Per-user and per-org request attribution remains sourced from Cost Core request records, whose org/user scope must come from verified identity claims. Do not divide Azure actual charges among users based on tokens and present the result as provider billing: Azure billing data does not identify a Verevon user. Codex/ChatGPT subscription allowance is another separate provider-reported measure and must not be counted as Azure or API dollar spend.

## Operational checks

After setting the environment, rebuild/restart the Model Plane `cost-core` service so migration `0007_azure_actual_costs` is applied and the importer starts. Check cost-core logs for the startup line `Azure Cost Management daily actual-cost sync enabled` and for `Azure Cost Management actual-cost sync completed` (sync errors contain status/context, never credentials). If configuration is absent, logs explicitly say the Azure sync is disabled. A working UI route alone does not prove that Azure permissions, project tags, or billing data are present.
