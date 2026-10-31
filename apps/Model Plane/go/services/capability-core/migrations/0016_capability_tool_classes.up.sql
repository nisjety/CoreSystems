-- 0016_capability_tool_classes.up.sql
--
-- Declared side-effect classes for tools (WS05-S1). Until now a tool's risk was
-- guessed by name substring in execution-core, and one capability row carried a
-- single risk_level for every tool behind it. This migration gives the registry
-- the vocabulary and the per-tool table; nothing reads either yet, so applying
-- it changes no decision anywhere.
--
-- VOCABULARY (closed; the Rust twin is mp-contracts::tool_class, the Go twin is
-- internal/models/tool_class.go, and both are pinned by the same known answers):
--   side_effect_class  read | sandbox_compute | context_write | external_write
--                      | destructive | owner_effect | unclassified
--   approval_floor     none | ask | durable_owner
-- `unclassified` is the strictest class: a row nobody has classified is treated
-- as the most dangerous thing it could be, never as a read. `owner_effect`
-- always carries `durable_owner`, enforced here by a CHECK so no writer can
-- store the pair the other way round.
--
-- capabilities.side_effect_class / approval_floor hold the CEILING of a
-- capability: the strictest class and floor among the tools behind it. They
-- default to `unclassified` / `none`; the seed below raises the seeded rows and
-- nothing else.
--
-- capability_tools holds one row per (org, capability, tool). The MCP-only
-- nullable columns are for WS05-S2 (observation, digest pin, drift); they carry
-- digests and the server's own hint flags only, never a description or a
-- schema, so a ZDR org's tool text is never stored. S1 writes none of them.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so
-- everything here is idempotent, and the seed can only ever tighten:
--   * capability_tools seed rows are re-asserted `WHERE source = 'seed'`, so a
--     row an operator overrode (source = 'override') is never rewritten;
--   * the capabilities ceiling takes the seed's class only while the row is
--     still at the default `unclassified`, and afterwards only moves up, so a
--     boot never lowers a class a person raised or an earlier boot seeded. An
--     override that lowers a seeded ceiling is therefore undone at the next boot
--     (the seed is the floor), and so is an operator putting a seeded row back
--     to `unclassified`: relax a seeded class in a reviewed migration, and park
--     a capability with its rollout state, not its class.

ALTER TABLE capabilities
    ADD COLUMN IF NOT EXISTS side_effect_class TEXT NOT NULL DEFAULT 'unclassified',
    ADD COLUMN IF NOT EXISTS approval_floor TEXT NOT NULL DEFAULT 'none';

ALTER TABLE capabilities
    DROP CONSTRAINT IF EXISTS capabilities_side_effect_class_check,
    DROP CONSTRAINT IF EXISTS capabilities_approval_floor_check,
    DROP CONSTRAINT IF EXISTS capabilities_owner_effect_floor_check,
    ADD CONSTRAINT capabilities_side_effect_class_check
        CHECK (side_effect_class IN ('read', 'sandbox_compute', 'context_write', 'external_write', 'destructive', 'owner_effect', 'unclassified')),
    ADD CONSTRAINT capabilities_approval_floor_check
        CHECK (approval_floor IN ('none', 'ask', 'durable_owner')),
    ADD CONSTRAINT capabilities_owner_effect_floor_check
        CHECK (side_effect_class <> 'owner_effect' OR approval_floor = 'durable_owner');

CREATE TABLE IF NOT EXISTS capability_tools (
    org_id               TEXT NOT NULL,
    capability_id        TEXT NOT NULL,
    tool_name            TEXT NOT NULL,
    side_effect_class    TEXT NOT NULL DEFAULT 'unclassified',
    approval_floor       TEXT NOT NULL DEFAULT 'none',
    -- The one argument-dependent refinement: execute_provider_action is a read
    -- or a write by its `operation`, so its row is the write class and this
    -- marker lets a call whose operation is catalogued as a read lower to read.
    call_refinement      TEXT,
    source               TEXT NOT NULL DEFAULT 'derived',
    -- MCP-only, filled by WS05-S2.
    server_id            TEXT,
    qualified_name       TEXT,
    pinned_digest        TEXT,
    observed_digest      TEXT,
    description_sha256   TEXT,
    schema_sha256        TEXT,
    declared_side_effect TEXT,
    declared_annotations JSONB,
    review_state         TEXT,
    observed_at          TIMESTAMPTZ,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, capability_id, tool_name),
    CONSTRAINT capability_tools_side_effect_class_check
        CHECK (side_effect_class IN ('read', 'sandbox_compute', 'context_write', 'external_write', 'destructive', 'owner_effect', 'unclassified')),
    CONSTRAINT capability_tools_approval_floor_check
        CHECK (approval_floor IN ('none', 'ask', 'durable_owner')),
    CONSTRAINT capability_tools_owner_effect_floor_check
        CHECK (side_effect_class <> 'owner_effect' OR approval_floor = 'durable_owner'),
    CONSTRAINT capability_tools_call_refinement_check
        CHECK (call_refinement IS NULL OR call_refinement = 'provider_operation'),
    CONSTRAINT capability_tools_source_check
        CHECK (source IN ('seed', 'derived', 'override')),
    CONSTRAINT capability_tools_declared_side_effect_check
        CHECK (declared_side_effect IS NULL OR declared_side_effect IN ('read_only', 'non_destructive_write', 'destructive', 'undeclared')),
    CONSTRAINT capability_tools_review_state_check
        CHECK (review_state IS NULL OR review_state IN ('approved', 'pending', 'drifted'))
);

-- A qualified MCP name (mcp__<server>__<tool>) resolves to exactly one row per
-- org; the registry lookup, never name parsing, decides the binding.
CREATE UNIQUE INDEX IF NOT EXISTS capability_tools_qualified_name_uq
    ON capability_tools (org_id, qualified_name)
    WHERE qualified_name IS NOT NULL;

CREATE INDEX IF NOT EXISTS capability_tools_capability_idx
    ON capability_tools (capability_id);

-- ---------------------------------------------------------------------------
-- Seed: every built-in tool execution-core can dispatch, org `global`.
--
-- One row per name in capability_policy::trusted_capability_id, classified so
-- that the class reproduces what the name heuristics answer today (the S0
-- oracle in execution-core's permission tests); tool_class_registry_parity.rs
-- parses these rows and fails on any drift or any unseeded arm. Keep one row
-- per line: that test reads them as text.
--
-- plan.propose is `context_write` (the plan's S1 seed table): it writes a plan
-- durably onto the person's approval surface, so a consumer that trusts the
-- declared class (an unattended allowlist with a read-only ceiling) must not
-- see a durable write as a read. The name heuristics still call it a read and
-- plan mode must keep it callable; that disagreement is carried by the explicit
-- exception table in tool_class_registry_parity.rs, and WS05-S4, which switches
-- the permission engine over to declared classes, must keep plan.propose
-- callable in plan mode through an explicit rule.
-- read_subagent_result is a read whose approval is about disclosure, so it is
-- `read` with an `ask` floor. tickets.create is the only owner effect.
-- ---------------------------------------------------------------------------

INSERT INTO capability_tools
    (org_id, capability_id, tool_name, side_effect_class, approval_floor, call_refinement, source)
VALUES
    ('global', 'cap.agent.spawn', 'subagent.research_fan_out', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.agent.spawn', 'subagent.task', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.agent.lineage.read', 'list_subagent_results', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.agent.lineage.read', 'read_subagent_result', 'read', 'ask', NULL, 'seed'),
    ('global', 'cap.browser.open', 'browser.act', 'external_write', 'none', NULL, 'seed'),
    ('global', 'cap.browser.open', 'browser.observe', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.browser.open', 'browser_agent', 'external_write', 'none', NULL, 'seed'),
    ('global', 'cap.command.sandbox', 'code_interpreter', 'sandbox_compute', 'none', NULL, 'seed'),
    ('global', 'cap.command.shell', 'shell', 'destructive', 'none', NULL, 'seed'),
    ('global', 'cap.memory.index', 'plan.propose', 'context_write', 'none', NULL, 'seed'),
    ('global', 'cap.memory.index', 'save_memory', 'context_write', 'none', NULL, 'seed'),
    ('global', 'cap.memory.search', 'recall_memory', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.process.background', 'process_list', 'sandbox_compute', 'none', NULL, 'seed'),
    ('global', 'cap.process.background', 'process_read', 'sandbox_compute', 'none', NULL, 'seed'),
    ('global', 'cap.process.background', 'process_signal', 'sandbox_compute', 'none', NULL, 'seed'),
    ('global', 'cap.process.background', 'process_start', 'sandbox_compute', 'none', NULL, 'seed'),
    ('global', 'cap.process.background', 'process_stdin', 'sandbox_compute', 'none', NULL, 'seed'),
    ('global', 'cap.retrieval.query', 'knowledge_search', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.skill.summarize', 'reattach_skill', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.http', 'web.read', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.http', 'web.search', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.http', 'web_fetch', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.http', 'web_search', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.information.read', 'company_lookup', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.information.read', 'news', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.information.read', 'traffic', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.information.read', 'yr_weather', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.provider.execute', 'execute_provider_action', 'external_write', 'none', 'provider_operation', 'seed'),
    ('global', 'cap.tool.provider.read', 'list_provider_actions', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.shipping.book', 'book_shipment', 'external_write', 'none', NULL, 'seed'),
    ('global', 'cap.tool.shipping.read', 'get_shipping_quotes', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.shipping.read', 'shipping_carriers', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.shipping.track', 'track_shipment', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.social.publish', 'publish_social_post', 'external_write', 'none', NULL, 'seed'),
    ('global', 'cap.tool.social.read', 'list_social_accounts', 'read', 'none', NULL, 'seed'),
    ('global', 'cap.tool.ticket.create', 'tickets.create', 'owner_effect', 'durable_owner', NULL, 'seed')
ON CONFLICT (org_id, capability_id, tool_name) DO UPDATE SET
    side_effect_class = EXCLUDED.side_effect_class,
    approval_floor = EXCLUDED.approval_floor,
    call_refinement = EXCLUDED.call_refinement,
    updated_at = now()
WHERE capability_tools.source = 'seed';

-- The ceiling of each seeded capability: the strictest class and floor among its
-- seeded tools. Ranks follow the vocabulary order, so the arrays below are the
-- rank-to-label maps. A row still at the default `unclassified` takes the
-- seed's class, which is the one place a class moves down, because the default
-- is "nobody has looked yet". Every other row only moves up, and the floor only
-- ever moves up.
WITH seed_ceiling AS (
    SELECT capability_id,
           max(CASE side_effect_class
                   WHEN 'read' THEN 0
                   WHEN 'sandbox_compute' THEN 1
                   WHEN 'context_write' THEN 2
                   WHEN 'external_write' THEN 3
                   WHEN 'destructive' THEN 4
                   WHEN 'owner_effect' THEN 5
                   ELSE 6
               END) AS class_rank,
           max(CASE approval_floor
                   WHEN 'none' THEN 0
                   WHEN 'ask' THEN 1
                   ELSE 2
               END) AS floor_rank
    FROM capability_tools
    WHERE org_id = 'global' AND source = 'seed'
    GROUP BY capability_id
),
seed_target AS (
    SELECT c.id,
           CASE
               WHEN c.side_effect_class = 'unclassified' THEN seed_ceiling.class_rank
               ELSE greatest(seed_ceiling.class_rank, CASE c.side_effect_class
                   WHEN 'read' THEN 0
                   WHEN 'sandbox_compute' THEN 1
                   WHEN 'context_write' THEN 2
                   WHEN 'external_write' THEN 3
                   WHEN 'destructive' THEN 4
                   ELSE 5
               END)
           END AS class_rank,
           greatest(seed_ceiling.floor_rank, CASE c.approval_floor
               WHEN 'none' THEN 0
               WHEN 'ask' THEN 1
               ELSE 2
           END) AS floor_rank
    FROM capabilities c
    JOIN seed_ceiling ON seed_ceiling.capability_id = c.id
    WHERE c.org_id = 'global' AND c.deleted_at IS NULL
)
UPDATE capabilities AS c
SET side_effect_class = (ARRAY['read', 'sandbox_compute', 'context_write', 'external_write', 'destructive', 'owner_effect', 'unclassified'])[seed_target.class_rank + 1],
    approval_floor = (ARRAY['none', 'ask', 'durable_owner'])[seed_target.floor_rank + 1]
FROM seed_target
WHERE c.id = seed_target.id
  AND (c.side_effect_class IS DISTINCT FROM (ARRAY['read', 'sandbox_compute', 'context_write', 'external_write', 'destructive', 'owner_effect', 'unclassified'])[seed_target.class_rank + 1]
       OR c.approval_floor IS DISTINCT FROM (ARRAY['none', 'ask', 'durable_owner'])[seed_target.floor_rank + 1]);
