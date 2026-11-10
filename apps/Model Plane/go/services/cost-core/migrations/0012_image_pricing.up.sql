-- Per-image prices for image generation (W4-IMG-2, F-t3-051): one row per
-- model, size and quality, effective from a date, in USD per generated image.
-- cost-core reads the rows in effect at startup (pricing.LoadFromPool) and
-- prices each image_generation usage event from them.
--
-- Nothing is seeded. A price is an owner or operations input with a recorded
-- source, never a number invented here: a (model, size, quality) with no row is
-- recorded with no image cost and flagged price_unknown. An empty quality is the
-- row for any quality the table has no row of its own for.
--
-- Additive and idempotent: the entrypoint re-applies every up migration on boot.
CREATE TABLE IF NOT EXISTS image_price_rates (
    model          TEXT            NOT NULL,
    size           TEXT            NOT NULL,
    quality        TEXT            NOT NULL DEFAULT '',
    -- USD per generated image.
    usd_per_image  NUMERIC(12, 6)  NOT NULL CHECK (usd_per_image >= 0),
    -- Where the price was sourced from (a dated vendor price sheet reference).
    source         TEXT            NOT NULL,
    effective_from TIMESTAMPTZ     NOT NULL,
    PRIMARY KEY (model, size, quality, effective_from)
);
