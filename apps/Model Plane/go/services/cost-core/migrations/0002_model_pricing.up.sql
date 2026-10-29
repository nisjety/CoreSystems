-- cost-core model pricing table (Phase 7 B5).
--
-- The price catalogue cost-core uses to turn token counts into a USD figure
-- when a usage event arrives without a pre-computed cost_usd. One row per
-- model key (a model family prefix or an exact id). The resolver matches a
-- reported `model_used` to the longest matching key, falling back to the
-- mandatory `default` row, so an unknown / newly-deployed model is still
-- priced (never silently zero).
--
-- Prices are USD per 1,000,000 tokens (the conventional unit on vendor price
-- sheets). They are public list prices for the model families Verevon routes to
-- via the intent layer (gpt-4o-mini / model-router / claude-sonnet / claude-opus
-- — see routing_policy.rs) plus the Azure speech + embedding helpers.
--
-- Idempotent: re-run on every boot by the container entrypoint (no
-- schema_migrations ledger in cost-core), so CREATE ... IF NOT EXISTS and an
-- upsert keep the catalogue current without ever failing a restart.
CREATE TABLE IF NOT EXISTS model_pricing (
    model              text            PRIMARY KEY,
    -- USD per 1,000,000 input (prompt) tokens.
    input_per_million  numeric(20, 6)  NOT NULL DEFAULT 0,
    -- USD per 1,000,000 output (completion) tokens.
    output_per_million numeric(20, 6)  NOT NULL DEFAULT 0,
    currency           text            NOT NULL DEFAULT 'USD',
    updated_at         timestamptz     NOT NULL DEFAULT now()
);

-- Seed / refresh the catalogue. ON CONFLICT keeps prices in sync on every boot
-- (the row set is the source of truth; edit here to change a price).
INSERT INTO model_pricing (model, input_per_million, output_per_million) VALUES
    -- Mandatory fallback. Priced at the mid (Sonnet-class) tier so an unknown
    -- model is never under-counted to zero.
    ('default',            3.00,  15.00),
    -- OpenAI / Azure OpenAI chat families.
    ('gpt-4o-mini',        0.15,   0.60),
    ('gpt-4o',             2.50,  10.00),
    ('gpt-4.1-mini',       0.40,   1.60),
    ('gpt-4.1',            2.00,   8.00),
    ('o4-mini',            1.10,   4.40),
    ('o3',                 2.00,   8.00),
    -- Azure "model router" deployment: routes to a gpt-4.1-class model; priced
    -- at that tier so router-labelled usage is counted, not zeroed.
    ('model-router',       2.00,   8.00),
    -- Anthropic / Azure-Anthropic families (matched by prefix:
    -- claude-sonnet-4-6 → claude-sonnet, claude-opus-4-8 → claude-opus, etc.).
    ('claude-haiku',       0.80,   4.00),
    ('claude-3-5-haiku',   0.80,   4.00),
    ('claude-sonnet',      3.00,  15.00),
    ('claude-3-5-sonnet',  3.00,  15.00),
    ('claude-opus',       15.00,  75.00),
    ('claude-3-opus',     15.00,  75.00)
ON CONFLICT (model) DO UPDATE SET
    input_per_million  = EXCLUDED.input_per_million,
    output_per_million = EXCLUDED.output_per_million,
    currency           = EXCLUDED.currency,
    updated_at         = now();
