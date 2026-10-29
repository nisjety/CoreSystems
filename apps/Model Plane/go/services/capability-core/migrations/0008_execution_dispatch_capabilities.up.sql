-- Canonical execution-core tool -> capability registry entries. These rows
-- intentionally start unavailable: source presence is not runtime health.
-- An authenticated health authority must attest each capability after its
-- backing dependency and execution path pass live checks.

WITH dispatch_capability (
    id, dispatch_name, description, risk_level, cost_class
) AS (
    VALUES
        ('cap.command.shell', 'shell', 'Run a command in the governed execution sandbox.', 'high', 'variable'),
        ('cap.agent.spawn', 'subagent.*', 'Spawn a bounded governed execution subagent.', 'medium', 'variable'),
        ('cap.browser.open', 'browser_agent', 'Drive a governed Quarry browser session.', 'high', 'variable'),
        ('cap.tool.http', 'web_search/web_fetch', 'Search or fetch public web content through Quarry.', 'medium', 'variable'),
        ('cap.retrieval.query', 'knowledge_search', 'Hybrid tenant-scoped knowledge retrieval.', 'low', 'bounded'),
        ('cap.tool.information.read', 'yr_weather/traffic/news/company_lookup', 'Read governed information-provider data.', 'low', 'bounded'),
        ('cap.tool.shipping.track', 'track_shipment', 'Read a tenant-scoped shipment status.', 'low', 'bounded'),
        ('cap.tool.shipping.read', 'get_shipping_quotes/shipping_carriers', 'Read governed shipping options.', 'low', 'variable'),
        ('cap.tool.shipping.book', 'book_shipment', 'Create a real shipment after durable approval.', 'high', 'variable'),
        ('cap.tool.social.read', 'list_social_accounts', 'Read connected social account metadata.', 'low', 'bounded'),
        ('cap.tool.social.publish', 'publish_social_post', 'Create and request publication of a social post.', 'high', 'variable'),
        ('cap.tool.provider.read', 'list_provider_actions', 'Read configured provider actions.', 'low', 'bounded'),
        ('cap.tool.provider.execute', 'execute_provider_action', 'Execute a governed external provider action.', 'high', 'variable')
)
INSERT INTO capabilities (
    id,
    org_id,
    kind,
    name,
    version,
    description,
    risk_level,
    scope,
    lazy_load,
    enabled,
    idempotency_key,
    config_json,
    tags,
    enabled_for_scopes,
    rollout_state,
    created_by,
    availability_state,
    availability_reason_code,
    availability_reason,
    execution_mode,
    cost_class,
    health_checked_at
)
SELECT
    id,
    'global',
    'tool',
    dispatch_name,
    '1.0.0',
    description,
    risk_level,
    'global',
    false,
    true,
    'execution-dispatch:' || id || ':v1',
    jsonb_build_object(
        'dispatch_name', dispatch_name,
        'owner_plane', 'model',
        'owner_service', 'execution-core'
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0008_execution_dispatch_capabilities',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    cost_class,
    NULL
FROM dispatch_capability
ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    version = EXCLUDED.version,
    description = EXCLUDED.description,
    risk_level = EXCLUDED.risk_level,
    scope = EXCLUDED.scope,
    enabled = EXCLUDED.enabled,
    idempotency_key = EXCLUDED.idempotency_key,
    config_json = EXCLUDED.config_json,
    tags = EXCLUDED.tags,
    enabled_for_scopes = EXCLUDED.enabled_for_scopes,
    rollout_state = EXCLUDED.rollout_state,
    cost_class = EXCLUDED.cost_class,
    updated_at = now();
