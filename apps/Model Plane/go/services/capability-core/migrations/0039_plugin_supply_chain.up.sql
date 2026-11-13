-- 0039_plugin_supply_chain.up.sql
--
-- Plugin supply-chain pins (F-t3-059, unit W4-PL-1). Dark until PLUGIN_LOADER_ENABLED is the
-- literal true. With the switch off, capability-core writes none of these columns, so every row
-- created while it is off keeps them NULL and plugin registration stays inert CRUD.
--
-- plugin_packages gains the source a plugin is registered with and the digest of its manifest:
--   source_kind      git_commit | archive
--   source_ref       the git or archive URL. The API accepts https only, with no credentials, query or fragment.
--   source_pin       a full 40-hex commit SHA (git_commit) or sha256:<64 hex> (archive). A branch or a
--                    tag never pins a source.
--   manifest_digest  sha256:<64 hex> over the canonical JSON of manifest_json (object keys sorted at
--                    every depth). The API computes it when the row is created, never from the caller.
--   installed_at     set when the loader has listed every component. The API never writes it.
--   catalog_approved an org admin listed the package in the org catalogue (W4-PL-3). Only the catalogue
--                    approve route writes it. The catalogue lists a package only while it is pinned and not
--                    killed, and a kill clears it.
-- plugin_packages_source_check keeps the three source columns consistent: all NULL, or a kind with
-- its matching pin and a URL.
--
-- plugin_kill_list refuses a manifest digest for one org, or for every org when org_id is 'global'.
-- A killed digest cannot be registered or enabled. An entry added after a plugin is enabled does
-- not disable it by itself, so the loader must check the list when it loads a plugin.
--
-- plugin_components lists each child a plugin expands into (skill, mcp_server or command) with the
-- state the loader left it in. Nothing writes it yet.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, so everything here is
-- idempotent: the columns and tables use IF NOT EXISTS, and the table-level CHECK is added only when
-- pg_constraint does not have it yet (ALTER TABLE has no IF NOT EXISTS form for a constraint).

ALTER TABLE plugin_packages
    ADD COLUMN IF NOT EXISTS source_kind TEXT NULL
        CONSTRAINT plugin_packages_source_kind_check CHECK (source_kind IN ('git_commit', 'archive')),
    ADD COLUMN IF NOT EXISTS source_ref TEXT NULL,
    ADD COLUMN IF NOT EXISTS source_pin TEXT NULL,
    ADD COLUMN IF NOT EXISTS manifest_digest TEXT NULL
        CONSTRAINT plugin_packages_manifest_digest_check
            CHECK (manifest_digest IS NULL OR manifest_digest ~ '^sha256:[0-9a-f]{64}$'),
    ADD COLUMN IF NOT EXISTS installed_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS catalog_approved BOOLEAN NOT NULL DEFAULT FALSE;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'plugin_packages_source_check'
    ) THEN
        ALTER TABLE plugin_packages
            ADD CONSTRAINT plugin_packages_source_check
            CHECK ((
                (source_kind IS NULL AND source_ref IS NULL AND source_pin IS NULL)
                OR (source_kind = 'git_commit' AND source_ref IS NOT NULL
                    AND source_pin ~ '^[0-9a-f]{40}$')
                OR (source_kind = 'archive' AND source_ref IS NOT NULL
                    AND source_pin ~ '^sha256:[0-9a-f]{64}$')
            ) IS TRUE);
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS plugin_kill_list (
    digest      TEXT NOT NULL,
    org_id      TEXT NOT NULL DEFAULT 'global',
    reason      TEXT NOT NULL DEFAULT '',
    created_by  TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, digest),
    CONSTRAINT plugin_kill_list_digest_check CHECK (digest ~ '^sha256:[0-9a-f]{64}$')
);

CREATE TABLE IF NOT EXISTS plugin_components (
    plugin_id   TEXT NOT NULL REFERENCES plugin_packages (id),
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    child_ref   TEXT NULL,
    status      TEXT NOT NULL,
    report      TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (plugin_id, kind, name),
    CONSTRAINT plugin_components_kind_check CHECK (kind IN ('skill', 'mcp_server', 'command')),
    CONSTRAINT plugin_components_status_check
        CHECK (status IN ('pending', 'installed', 'refused', 'removed'))
);
