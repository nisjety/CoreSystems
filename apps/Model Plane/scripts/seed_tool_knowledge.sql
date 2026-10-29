-- seed_tool_knowledge.sql
--
-- Idempotent seed data for two session-core tables, run once per org via
-- seed_tool_knowledge.sh (which supplies :orgid):
--
--   agent_skills  — compound, domain-level "how to use these tools together"
--                   guidance. LIVE-wired: model-gateway's inline chat loop
--                   (skills.rs/fetch_skill_context in sse.rs) and
--                   execution-core's RunAgent loop (fetch_skill_context in
--                   runtime_loop/agent.rs) both pull an org's enabled skills
--                   and inject the top keyword-matched ones as system context
--                   before the model decides which tool to call. This is the
--                   part that actually changes model behaviour.
--
--   agent_memory  — one atomic "what this tool does and when to reach for
--                   it" fact per tool (kind='instruction', scope='org').
--                   This is the org's own durable knowledge-base record of
--                   its tools. NOT YET read back into a live chat/agent turn
--                   anywhere (session-core's only current readers of
--                   agent_memory are dreaming.rs's own dedup pass and GDPR
--                   erasure) — seeded now so the data exists and Dreaming's
--                   dedup sees it, but it does not yet steer a live answer
--                   the way agent_skills does. Wiring a live read path is
--                   tracked separately; do not claim this half is "live" to
--                   a customer or in GTM material until it is.
--
-- Content describes the REAL tool catalogues as of 2026-08-04:
--   model-gateway::tool_loop::builtin_tool_defs()      (inline chat loop)
--   execution-core::runtime_loop::agent::offered_tool_defs()  (RunAgent loop)
-- The two surfaces sometimes name the same capability differently
-- (web_fetch/fetch_url, get_shipping_quotes/shipping_get_quotes,
-- list_social_accounts/social_list_accounts) — content says so explicitly
-- rather than picking one name and hiding the other. Re-running this file is
-- always safe: every statement is an idempotent upsert keyed by a stable
-- (org, name)/(org, scope, key), and a value the RPC/UI already let an
-- operator hand-edit (origin/review_state = user-owned) is left untouched.

-- ---------------------------------------------------------------------------
-- agent_skills
-- ---------------------------------------------------------------------------

INSERT INTO agent_skills
    (id, org_id, name, description, content, trigger_keywords,
     trigger_file_patterns, tool_restrictions, enabled, origin, created_at, updated_at)
VALUES (
    'seed-skill-shipping-' || :'orgid', :'orgid',
    'Shipping & freight',
    'Built-in tool guidance for shipping/freight quoting, booking, and tracking.',
$sk$Quote before you book: get_shipping_quotes (or shipping_get_quotes) is read-only and safe to call freely once you have sender+recipient address and package weight/dimensions — ask for whichever of those the user hasn't given, never guess them. book_shipment places a REAL freight order (costs money, a courier is dispatched) and always pauses for human approval; only call it with the exact carrier_code/service_name/price the user picked from a quote, never a value you made up. Cross-border shipments (sender and recipient countries differ) require a customs object with itemized contents — the server rejects the booking otherwise, so ask for that detail up front rather than after a failed attempt. segment (b2b/b2c) is about the RECIPIENT, not the sender. Use shipping_carriers/track_shipment for status/coverage questions — they never book anything.$sk$,
    '["shipping","shipment","freight","parcel","courier","carrier","bring","dhl","postnord","helthjem","porterbuddy","tracking","customs"]'::jsonb,
    '[]'::jsonb, '[]'::jsonb, true, 'background_review', now(), now()
)
ON CONFLICT (org_id, name) DO UPDATE SET
    description = EXCLUDED.description, content = EXCLUDED.content,
    trigger_keywords = EXCLUDED.trigger_keywords,
    trigger_file_patterns = EXCLUDED.trigger_file_patterns,
    tool_restrictions = EXCLUDED.tool_restrictions,
    enabled = EXCLUDED.enabled, updated_at = now()
WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user';

INSERT INTO agent_skills
    (id, org_id, name, description, content, trigger_keywords,
     trigger_file_patterns, tool_restrictions, enabled, origin, created_at, updated_at)
VALUES (
    'seed-skill-social-' || :'orgid', :'orgid',
    'Social media publishing',
    'Built-in tool guidance for connected social accounts, posts, and campaigns.',
$sk$Always call list_social_accounts (or social_list_accounts) FIRST when asked to post or publish — it tells you which platforms are actually connected, so you don't draft copy for a platform Verevon can't reach. publish_social_post is a REAL action requiring human approval, and even after approval the post still waits under Social → Approvals before it goes live — never tell the user something is published until you've confirmed that, and never claim you cannot post at all (Verevon CAN, it just needs approval). social_list_posts/social_list_campaigns are read-only inventory — use them to check what's queued or scheduled; they cannot create, edit, or publish anything themselves.$sk$,
    '["social","post","publish","linkedin","instagram","facebook","tiktok","twitter","snapchat","campaign","schedule"]'::jsonb,
    '[]'::jsonb, '[]'::jsonb, true, 'background_review', now(), now()
)
ON CONFLICT (org_id, name) DO UPDATE SET
    description = EXCLUDED.description, content = EXCLUDED.content,
    trigger_keywords = EXCLUDED.trigger_keywords,
    trigger_file_patterns = EXCLUDED.trigger_file_patterns,
    tool_restrictions = EXCLUDED.tool_restrictions,
    enabled = EXCLUDED.enabled, updated_at = now()
WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user';

INSERT INTO agent_skills
    (id, org_id, name, description, content, trigger_keywords,
     trigger_file_patterns, tool_restrictions, enabled, origin, created_at, updated_at)
VALUES (
    'seed-skill-provideractions-' || :'orgid', :'orgid',
    'Provider integrations (execute_provider_action)',
    'Built-in tool guidance for the generic third-party provider action dispatcher.',
$sk$execute_provider_action is a generic dispatcher over whichever third-party providers the org has connected (Meta, WhatsApp, Slack, GitHub, Notion, Shopify, ad platforms, ...) — it has no fixed schema of its own. Always call list_provider_actions FIRST to learn the real connection_id and exact operation name (e.g. pages.post, whatsapp.messages.send) for THIS org; never guess either, since they vary per connection. Write/outbound operations always require human approval before they run — say so plainly rather than implying an immediate result. If list_provider_actions shows nothing for what the user wants, say the provider isn't connected instead of attempting a made-up operation name.$sk$,
    '["provider","integration","connected","connection","whatsapp","messenger","slack","github","notion","shopify","ads","campaign"]'::jsonb,
    '[]'::jsonb, '[]'::jsonb, true, 'background_review', now(), now()
)
ON CONFLICT (org_id, name) DO UPDATE SET
    description = EXCLUDED.description, content = EXCLUDED.content,
    trigger_keywords = EXCLUDED.trigger_keywords,
    trigger_file_patterns = EXCLUDED.trigger_file_patterns,
    tool_restrictions = EXCLUDED.tool_restrictions,
    enabled = EXCLUDED.enabled, updated_at = now()
WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user';

INSERT INTO agent_skills
    (id, org_id, name, description, content, trigger_keywords,
     trigger_file_patterns, tool_restrictions, enabled, origin, created_at, updated_at)
VALUES (
    'seed-skill-knowledgebase-' || :'orgid', :'orgid',
    'Organization knowledge base',
    'Built-in tool guidance for the org''s own ingested knowledge base tools.',
$sk$Three different tools answer three different questions about the org's own data — pick by what's actually being asked, not by habit. knowledge_search answers 'what does it say' (returns passages/content); knowledge_list_documents answers 'what do we have' (inventory: titles, sources, counts — it deliberately returns NO document content, so don't use it to answer a content question). insights_overview is the Insights dashboard's own metrics and connector wiring status, not a live connections list — never report a connector marked 'planned' as something the org has actually connected. None of these three touches the public web; for that, use web_search instead.$sk$,
    '["knowledge","document","documents","ingested","our data","company data","our docs"]'::jsonb,
    '[]'::jsonb, '[]'::jsonb, true, 'background_review', now(), now()
)
ON CONFLICT (org_id, name) DO UPDATE SET
    description = EXCLUDED.description, content = EXCLUDED.content,
    trigger_keywords = EXCLUDED.trigger_keywords,
    trigger_file_patterns = EXCLUDED.trigger_file_patterns,
    tool_restrictions = EXCLUDED.tool_restrictions,
    enabled = EXCLUDED.enabled, updated_at = now()
WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user';

INSERT INTO agent_skills
    (id, org_id, name, description, content, trigger_keywords,
     trigger_file_patterns, tool_restrictions, enabled, origin, created_at, updated_at)
VALUES (
    'seed-skill-webresearch-' || :'orgid', :'orgid',
    'Public web research',
    'Built-in tool guidance for public web search/fetch versus the org''s own data.',
$sk$web_search is for facts that may have changed since training — prices, news, current versions, current office-holders — never for the organization's own data (use knowledge_search) or for weather (use get_weather/yr_weather, which returns real structured forecast data instead of scraped pages). web_fetch/fetch_url reads ONE specific URL you already have; it does not search. Treat an empty result or a fetch error as inconclusive, never as proof something doesn't exist — reformulate the query (add a year, a source name, a specific term) before concluding a search "found nothing".$sk$,
    '["news","current","latest","search the web","look up","recent","today"]'::jsonb,
    '[]'::jsonb, '[]'::jsonb, true, 'background_review', now(), now()
)
ON CONFLICT (org_id, name) DO UPDATE SET
    description = EXCLUDED.description, content = EXCLUDED.content,
    trigger_keywords = EXCLUDED.trigger_keywords,
    trigger_file_patterns = EXCLUDED.trigger_file_patterns,
    tool_restrictions = EXCLUDED.tool_restrictions,
    enabled = EXCLUDED.enabled, updated_at = now()
WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user';

INSERT INTO agent_skills
    (id, org_id, name, description, content, trigger_keywords,
     trigger_file_patterns, tool_restrictions, enabled, origin, created_at, updated_at)
VALUES (
    'seed-skill-noreference-' || :'orgid', :'orgid',
    'Weather, traffic, and Norwegian company lookups',
    'Built-in tool guidance for Norway-specific reference lookups.',
$sk$get_weather covers only ten named Norwegian cities (Oslo, Bergen, Trondheim, Stavanger, Tromsø, Kristiansand, Drammen, Fredrikstad, Sandnes, Sarpsborg) via structured live data — for anywhere else, use web_search instead of guessing. The execution-core agent surface instead offers yr_weather, which takes a raw lat/lon coordinate and works for any point in Norway. traffic returns Statens Vegvesen volume/speed data for a coordinate. company_lookup (Brønnøysundregistrene/Enhetsregisteret) resolves a Norwegian company by name or 9-digit org number — real registry data, not a general company-research tool; for anything beyond registry facts (news about a company, its products), use web_search.$sk$,
    '["weather","forecast","traffic","vegvesen","company","org.nr","organisasjonsnummer","enhetsregisteret","brreg"]'::jsonb,
    '[]'::jsonb, '[]'::jsonb, true, 'background_review', now(), now()
)
ON CONFLICT (org_id, name) DO UPDATE SET
    description = EXCLUDED.description, content = EXCLUDED.content,
    trigger_keywords = EXCLUDED.trigger_keywords,
    trigger_file_patterns = EXCLUDED.trigger_file_patterns,
    tool_restrictions = EXCLUDED.tool_restrictions,
    enabled = EXCLUDED.enabled, updated_at = now()
WHERE agent_skills.origin <> 'user' OR EXCLUDED.origin = 'user';

-- ---------------------------------------------------------------------------
-- agent_memory — one atomic fact per tool, kind='instruction', scope='org'
-- ---------------------------------------------------------------------------

INSERT INTO agent_memory
    (id, org_id, session_id, scope, key, content, kind, confidence, owner,
     review_state, created_at, updated_at)
VALUES
    ('seed-mem-web_search-' || :'orgid', :'orgid', NULL, 'org', 'tool:web_search',
$m$Searches the public web for current information (prices, news, versions, current facts) and returns ranked title/url/snippet results. Not for the organization's own data (use knowledge_search) or weather (use get_weather/yr_weather).$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-get_weather-' || :'orgid', :'orgid', NULL, 'org', 'tool:get_weather',
$m$Returns live structured weather (temperature, wind, precipitation, forecast) for one of ten named Norwegian cities (Oslo, Bergen, Trondheim, Stavanger, Tromsø, Kristiansand, Drammen, Fredrikstad, Sandnes, Sarpsborg). On the execution-core agent surface the equivalent tool is yr_weather, which takes a raw lat/lon instead and covers any point in Norway. Use web_search for any other location.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-code_interpreter-' || :'orgid', :'orgid', NULL, 'org', 'tool:code_interpreter',
$m$Runs real Python or POSIX sh in an isolated, network-less sandbox with a ~30s timeout. Use it for exact computation and for GENERATING downloadable files (.xlsx via openpyxl, .docx via python-docx, .pdf via reportlab, charts via matplotlib). Cannot download anything or call an API — use web_search/web_fetch for that instead.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-create_artifact-' || :'orgid', :'orgid', NULL, 'org', 'tool:create_artifact',
$m$Creates a substantial, self-contained work product (a document, code file, or HTML page) shown in a side panel instead of buried in chat prose. Use only for content long enough or reusable enough to be worth saving — not for short answers or conversational replies.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-update_artifact-' || :'orgid', :'orgid', NULL, 'org', 'tool:update_artifact',
$m$Replaces the content of an artifact created earlier with create_artifact, producing a new version the user can step back through. Always reuse the SAME artifact id for a revision — never create a second artifact for a change to the same thing.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-web_fetch-' || :'orgid', :'orgid', NULL, 'org', 'tool:web_fetch',
$m$Fetches one specific URL and returns its cleaned text content. On the model-gateway inline chat surface the equivalent tool is named fetch_url. Neither searches — give it a URL you already have, not a query.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-knowledge_search-' || :'orgid', :'orgid', NULL, 'org', 'tool:knowledge_search',
$m$Searches the organization's OWN ingested documents via Data Plane's hybrid retrieval and returns relevant passages. Answers "what does it say", not "what do we have" (use knowledge_list_documents for inventory) or public-web questions (use web_search).$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-get_shipping_quotes-' || :'orgid', :'orgid', NULL, 'org', 'tool:get_shipping_quotes',
$m$Compares shipping/freight quotes across Verevon's connected carrier fleet for a given origin, destination, package, and segment (b2b/b2c, based on the RECIPIENT). Read-only — never books anything. Always call this before book_shipment and always ask the user for the address/weight details rather than guessing them. On the model-gateway inline chat surface the equivalent tool is named shipping_get_quotes.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-knowledge_list_documents-' || :'orgid', :'orgid', NULL, 'org', 'tool:knowledge_list_documents',
$m$Lists WHICH documents exist in the org's knowledge base (title, source, type, status, total count) — pure inventory, with NO document content. Use for "how many documents do we have" style questions; use knowledge_search for anything about what a document actually says.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-insights_overview-' || :'orgid', :'orgid', NULL, 'org', 'tool:insights_overview',
$m$Returns the org's own Insights dashboard numbers: scorecards, per-surface event rollups, connector lag, and the supported-connector catalogue with wiring status. The connector list is the catalogue of what CAN be connected, not a live-connections list — a "planned" entry is not an active connection. Not for public benchmarks (use web_search) or social account/post data (use social_list_accounts/social_list_posts).$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-list_social_accounts-' || :'orgid', :'orgid', NULL, 'org', 'tool:list_social_accounts',
$m$Lists the organization's actually-connected social accounts (provider, handle, status, granted capabilities, token health). Always call this before drafting or publishing a social post, so you know which platforms are really reachable. On the model-gateway inline chat surface the equivalent tool is named social_list_accounts.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-social_list_posts-' || :'orgid', :'orgid', NULL, 'org', 'tool:social_list_posts',
$m$Lists the org's own social posts (draft/scheduled/published/failed) with status and approval state. Read-only — cannot create, schedule, or publish; if asked to publish something, say it needs approval instead of calling this.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-social_list_campaigns-' || :'orgid', :'orgid', NULL, 'org', 'tool:social_list_campaigns',
$m$Lists the org's own social campaigns with goal, status, platforms, and dates. Read-only; for the individual posts inside a campaign use social_list_posts instead.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-publish_social_post-' || :'orgid', :'orgid', NULL, 'org', 'tool:publish_social_post',
$m$Creates a social post and requests publish to the chosen platforms. Always requires human approval first, and the post THEN waits under Social → Approvals before it actually goes live — never report it as already published. Use platform keys exactly as returned by list_social_accounts.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-book_shipment-' || :'orgid', :'orgid', NULL, 'org', 'tool:book_shipment',
$m$Places a REAL freight order with a carrier — costs money, dispatches a courier, and always requires human approval. Only call it with the exact carrier_code/service_name/price from a quote the user already picked via get_shipping_quotes; never invent those values. Cross-border shipments (sender/recipient countries differ) require a customs object or the server rejects the booking.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-shipping_carriers-' || :'orgid', :'orgid', NULL, 'org', 'tool:shipping_carriers',
$m$Lists which carriers are registered in Verevon's shipping aggregator and whether each is running demo prices or live agreement prices. Read-only reference — does not fetch quotes (use get_shipping_quotes) or book anything (use book_shipment).$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-track_shipment-' || :'orgid', :'orgid', NULL, 'org', 'tool:track_shipment',
$m$Tracks one parcel by tracking number against the Bring/Posten carrier API. Needs only the tracking number — nothing else.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-company_lookup-' || :'orgid', :'orgid', NULL, 'org', 'tool:company_lookup',
$m$Looks up a Norwegian company in the public Brønnøysund Enhetsregisteret by name or 9-digit org number — real registry data (name, org.nr, address, status), not a general company-research tool. For news or products about a company, use web_search instead.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-list_provider_actions-' || :'orgid', :'orgid', NULL, 'org', 'tool:list_provider_actions',
$m$Lists the org's connected third-party providers and the exact operations available on each. Always call this before execute_provider_action — connection_id and operation names vary per org and must never be guessed.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-execute_provider_action-' || :'orgid', :'orgid', NULL, 'org', 'tool:execute_provider_action',
$m$Runs ONE operation on a connected provider (publish a Page post, send a WhatsApp message, create a GitHub issue, etc.) using the connection_id/operation from list_provider_actions. Write/outbound operations always require human approval before they run.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-traffic-' || :'orgid', :'orgid', NULL, 'org', 'tool:traffic',
$m$Returns Statens Vegvesen traffic registration data (volume, speed) near a coordinate. Norway-specific; needs a lat/lon, with an optional search radius.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-news-' || :'orgid', :'orgid', NULL, 'org', 'tool:news',
$m$Returns the latest Norwegian/industry news articles, optionally filtered by category. For anything not in that feed, fall back to web_search.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now()),

    ('seed-mem-subagent_task-' || :'orgid', :'orgid', NULL, 'org', 'tool:subagent.task',
$m$Delegates a self-contained sub-task to a subagent that shares your tools but runs in an isolated context, returning only its final answer. Use it for a sub-task that needs many tool calls whose intermediate output you don't need to see — never for something you can finish in a call or two yourself, since its rounds are charged against the SAME run budget.$m$,
     'instruction', 1.0, 'system', 'accepted', now(), now())

ON CONFLICT (org_id, scope, owner, key) WHERE session_id IS NULL DO UPDATE SET
    content = EXCLUDED.content,
    confidence = EXCLUDED.confidence,
    updated_at = now()
WHERE agent_memory.review_state <> 'rejected';
