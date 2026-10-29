-- Slice 10: model registry table backing cap.model.* capabilities.
CREATE TABLE IF NOT EXISTS models (
    id            uuid PRIMARY KEY,
    org_id        uuid,
    scope         text         NOT NULL DEFAULT 'global',
    provider      text         NOT NULL,
    name          text         NOT NULL,
    version       text         NOT NULL,
    config_json   jsonb        NOT NULL DEFAULT '{}'::jsonb,
    enabled       boolean      NOT NULL DEFAULT true,
    risk_level    text         NOT NULL DEFAULT 'low',
    lazy_load     boolean      NOT NULL DEFAULT false,
    description   text,
    created_at    timestamptz  NOT NULL DEFAULT now(),
    updated_at    timestamptz  NOT NULL DEFAULT now(),
    deleted_at    timestamptz
);

CREATE UNIQUE INDEX IF NOT EXISTS models_org_provider_name_unique
    ON models (org_id, provider, name)
    NULLS NOT DISTINCT
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS models_scope_idx
    ON models (scope)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS models_enabled_idx
    ON models (enabled)
    WHERE deleted_at IS NULL;
