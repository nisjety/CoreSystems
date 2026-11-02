-- 0031_web_crawl_tool_capabilities.up.sql
--
-- E3.7.10 (capability-core half): registers the three capabilities the agent's bulk-acquisition
-- tools are governed by, and the declared class of each tool, in the 0016 seed format:
--
--   cap.web.crawl  knowledge.crawl_site  context_write, medium risk. A crawl that is ingested
--                                         writes pages into the org's own knowledge base, so it
--                                         is a durable context write (as save_memory is), never
--                                         a read.
--   cap.web.map    knowledge.map         read, low risk. It lists the URLs of a site and fetches
--                                         no page content into the corpus.
--   cap.web.batch  knowledge.batch       context_write, medium risk. A batch of URLs scraped and
--                                         ingested, the same write as a crawl with a fixed list.
--
-- WHY NOT cap.tool.http. web_search and web_fetch are bounded single reads under cap.tool.http
-- (medium, variable). A crawl or a batch multiplies pages, spend and egress by a count the model
-- chooses, and the approval above a page count (tighten-only config, default 50) is decided per
-- capability, so each family gets its own capability id instead of widening the http one.
--
-- NO APPROVAL FLOOR IN THE SEED. The page-count approval is a runtime decision of execution-core
-- (EXECUTION_CORE_WEB_CRAWL_TOOLS, default off, with its own tighten-only cap); the seed floor is
-- 'none' and an operator can raise a row with an override, which a boot never rewrites.
--
-- STARTS UNAVAILABLE ON PURPOSE
--
-- Source presence is not runtime health. execution-core attests these capabilities only when
-- EXECUTION_CORE_WEB_CRAWL_TOOLS is on for that instance and Quarry's endpoints answer. This
-- migration never fabricates an attestation, and until execution-core binds the three tool names
-- (capability_policy.rs), nothing resolves to these rows: the migration changes no decision today.
--
-- The rows mirror the 0008 and 0027 layout, so kind-filtered reads and the
-- capabilities_org_kind_name_uq index behave the same as for the other execution-dispatch
-- capabilities. The ceiling (side_effect_class) takes the seed's class only while the row is still
-- at the default 'unclassified'; afterwards it is never lowered by a boot.

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
    health_checked_at,
    side_effect_class,
    approval_floor
) VALUES
(
    'cap.web.crawl',
    'global',
    'tool',
    'web_crawl',
    '1.0.0',
    'Crawl a public site from a start URL under a page, depth and budget cap and ingest the pages into the org knowledge base through Quarry. Approval above the configured page count is execution-core''s.',
    'medium',
    'global',
    false,
    true,
    'execution-dispatch:cap.web.crawl:v1',
    jsonb_build_object(
        'dispatch_name', 'knowledge.crawl_site',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('knowledge.crawl_site'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0031_web_crawl_tool_capabilities',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'variable',
    NULL,
    'context_write',
    'none'
),
(
    'cap.web.map',
    'global',
    'tool',
    'web_map',
    '1.0.0',
    'List the URLs of a public site (sitemap and links) through Quarry without fetching page content into the knowledge base.',
    'low',
    'global',
    false,
    true,
    'execution-dispatch:cap.web.map:v1',
    jsonb_build_object(
        'dispatch_name', 'knowledge.map',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('knowledge.map'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0031_web_crawl_tool_capabilities',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'bounded',
    NULL,
    'read',
    'none'
),
(
    'cap.web.batch',
    'global',
    'tool',
    'web_batch',
    '1.0.0',
    'Scrape a list of public URLs in one bounded batch and ingest the pages into the org knowledge base through Quarry.',
    'medium',
    'global',
    false,
    true,
    'execution-dispatch:cap.web.batch:v1',
    jsonb_build_object(
        'dispatch_name', 'knowledge.batch',
        'owner_plane', 'model',
        'owner_service', 'execution-core',
        'tool_family', jsonb_build_array('knowledge.batch'),
        'host_command_execution', false
    ),
    ARRAY['execution-dispatch'],
    ARRAY['global'],
    'stable',
    'migration:0031_web_crawl_tool_capabilities',
    'unavailable',
    'health_not_attested',
    'Execution capability runtime health has not been attested.',
    'unavailable',
    'variable',
    NULL,
    'context_write',
    'none'
)
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
    side_effect_class = CASE
        WHEN capabilities.side_effect_class = 'unclassified' THEN EXCLUDED.side_effect_class
        ELSE capabilities.side_effect_class
    END,
    updated_at = now();

-- The declared class of each tool, one tuple per line (the registry seed reader and
-- execution-core's tool_class_registry_parity.rs read these rows as text). A row is re-asserted
-- only while it is still the seed's, so an operator override is never rewritten.
INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.web.batch', 'knowledge.batch', 'context_write', 'none', NULL, 'seed'),
    ('global', 'cap.web.crawl', 'knowledge.crawl_site', 'context_write', 'none', NULL, 'seed'),
    ('global', 'cap.web.map', 'knowledge.map', 'read', 'none', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';
