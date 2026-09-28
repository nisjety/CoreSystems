-- Durable backend for internal/lease and internal/snapshot, replacing the
-- map + sync.RWMutex in-memory stores. See the S3.2 close-out design's own
-- named "restart" gap and
-- apps/Frontend Plane/verevonv3/docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md
-- section 2 ("this is also where sandbox-manager's own lease.go/snapshot.go
-- ... get ported onto the same Postgres store").
--
-- state stores model_plane.v1.SandboxLifecycleState's own numeric wire
-- values directly (1=SCRATCH, 2=ACTIVE, 3=SNAPSHOTTING, 4=DESTROYED) rather
-- than a second, redundant string encoding.
CREATE TABLE IF NOT EXISTS leases (
    id          TEXT PRIMARY KEY,
    scope_id    TEXT NOT NULL,
    scope_type  TEXT NOT NULL,
    org_id      TEXT NOT NULL,
    owner_id    TEXT NOT NULL DEFAULT '',
    endpoint    TEXT NOT NULL,
    space_id    TEXT NOT NULL DEFAULT '',
    backend_id  TEXT NOT NULL DEFAULT '',
    state       SMALLINT NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS leases_org_idx ON leases (org_id);

CREATE TABLE IF NOT EXISTS snapshots (
    id          TEXT PRIMARY KEY,
    lease_id    TEXT NOT NULL,
    label       TEXT NOT NULL,
    object_key  TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS snapshots_lease_idx ON snapshots (lease_id);
