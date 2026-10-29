"""Read-only lifecycle projections over append-only Xero v2 history.

Only stored collection headers prove completeness: an empty header replaces
children, while an omitted/null collection says nothing about known children.
The ingestion-provided source_deleted fact means explicit raw Status=DELETED;
neither missing records nor missing children establish source deletion.
"""

import re


_PROJECT_IDENTIFIER = re.compile(r"[A-Za-z][A-Za-z0-9-]*")
_DATASET_IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
_IDENTITY = ("generation", "tenant_id", "endpoint", "record_id", "version_id")
_RECORD_COLUMNS = _IDENTITY + (
    "observed_at",
    "source_updated_at",
    "run_id",
    "receipt_id",
    "source_status",
    "source_deleted",
    "payload_hash",
    "payload_json",
)
_COLLECTION_COLUMNS = _IDENTITY + (
    "collection_name",
    "observed_at",
    "source_updated_at",
    "child_count",
)
_CHILD_COLUMNS = _IDENTITY + (
    "collection_name",
    "child_key",
    "line_position",
    "native_id",
    "payload_hash",
    "payload_json",
    "observed_at",
    "source_updated_at",
)


_RECORDS_SQL = """WITH ranked AS (
  SELECT
    r.*,
    ROW_NUMBER() OVER record_history AS version_rank,
    LAG(r.version_id) OVER record_history AS superseded_by,
    LAG(r.observed_at) OVER record_history AS superseded_at,
    COALESCE(
      MIN(r.payload_hash) OVER same_revision <> MAX(r.payload_hash) OVER same_revision,
      FALSE
    ) AS revision_conflict
  FROM `{namespace}.v2_record_versions` AS r
  WINDOW
    record_history AS (
      PARTITION BY r.generation, r.tenant_id, r.endpoint, r.record_id
      ORDER BY COALESCE(r.source_updated_at, r.observed_at) DESC,
        r.observed_at DESC, r.version_id DESC
    ),
    same_revision AS (
      PARTITION BY r.generation, r.tenant_id, r.endpoint, r.record_id,
        COALESCE(r.source_updated_at, r.observed_at), r.observed_at
    )
)
SELECT
  {columns},
  r.version_rank = 1 AND NOT r.revision_conflict AS is_current,
  r.version_rank > 1 AS is_superseded,
  COALESCE(r.source_deleted, FALSE) AS is_deleted,
  CASE
    WHEN r.revision_conflict THEN 'conflict'
    WHEN r.source_deleted THEN 'source_deleted'
    WHEN r.version_rank = 1 THEN 'current'
    ELSE 'superseded'
  END AS record_state,
  r.superseded_by,
  r.superseded_at,
  r.revision_conflict
FROM ranked AS r"""


_COLLECTIONS_SQL = """WITH ranked AS (
  SELECT
    c.*,
    ROW_NUMBER() OVER collection_history AS collection_rank,
    LAG(c.version_id) OVER collection_history AS superseded_by,
    LAG(c.observed_at) OVER collection_history AS superseded_at
  FROM `{namespace}.v2_collection_versions` AS c
  WINDOW collection_history AS (
    PARTITION BY c.generation, c.tenant_id, c.endpoint, c.record_id, c.collection_name
    ORDER BY COALESCE(c.source_updated_at, c.observed_at) DESC,
      c.observed_at DESC, c.version_id DESC
  )
), latest_parent AS (
  SELECT * FROM `{namespace}.v2_records` WHERE NOT is_superseded
), classified AS (
  SELECT
    c.*,
    COALESCE(p.source_deleted AND NOT p.revision_conflict, FALSE) AS source_deleted,
    COALESCE(o.revision_conflict, FALSE)
      OR COALESCE(p.revision_conflict, FALSE) AS revision_conflict,
    c.collection_rank = 1
      AND COALESCE(p.is_current, FALSE)
      AND NOT COALESCE(p.is_deleted, FALSE)
      AND o.version_id IS NOT NULL
      AND NOT COALESCE(o.revision_conflict, FALSE) AS is_current
  FROM ranked AS c
  LEFT JOIN `{namespace}.v2_records` AS o
    ON c.generation = o.generation AND c.tenant_id = o.tenant_id
    AND c.endpoint = o.endpoint AND c.record_id = o.record_id
    AND c.version_id = o.version_id
  LEFT JOIN latest_parent AS p
    ON c.generation = p.generation AND c.tenant_id = p.tenant_id
    AND c.endpoint = p.endpoint AND c.record_id = p.record_id
)
SELECT
  {columns},
  c.source_deleted,
  c.is_current,
  c.collection_rank > 1 AS is_superseded,
  c.source_deleted AS is_deleted,
  CASE
    WHEN c.revision_conflict THEN 'conflict'
    WHEN c.source_deleted THEN 'source_deleted'
    WHEN c.is_current THEN 'current'
    ELSE 'superseded'
  END AS record_state,
  c.superseded_by,
  c.superseded_at,
  c.revision_conflict
FROM classified AS c"""


_CHILDREN_SQL = """WITH latest_parent AS (
  SELECT * FROM `{namespace}.v2_records` WHERE NOT is_superseded
), latest_collection AS (
  SELECT * FROM `{namespace}.v2_collections` WHERE NOT is_superseded
), membership AS (
  SELECT DISTINCT
    generation, tenant_id, endpoint, record_id, version_id, collection_name, child_key
  FROM `{namespace}.v2_child_versions`
), classified AS (
  SELECT
    c.*,
    o.observed_at,
    o.source_updated_at,
    COALESCE(p.source_deleted AND NOT p.revision_conflict, FALSE) AS source_deleted,
    COALESCE(o.revision_conflict, FALSE)
      OR COALESCE(p.revision_conflict, FALSE)
      OR COALESCE(l.revision_conflict, FALSE) AS revision_conflict,
    COALESCE(c.version_id <> l.version_id, FALSE) AS is_superseded,
    COALESCE(h.is_current, FALSE)
      AND COALESCE(c.version_id = l.version_id, FALSE)
      AND COALESCE(p.is_current, FALSE)
      AND NOT COALESCE(p.is_deleted, FALSE)
      AND o.version_id IS NOT NULL
      AND NOT COALESCE(o.revision_conflict, FALSE) AS is_current,
    l.version_id IS NOT NULL AND p.version_id IS NOT NULL
      AND NOT COALESCE(l.revision_conflict, FALSE)
      AND NOT COALESCE(p.revision_conflict, FALSE)
      AND m.child_key IS NULL AS is_removed
  FROM `{namespace}.v2_child_versions` AS c
  LEFT JOIN `{namespace}.v2_records` AS o
    ON c.generation = o.generation AND c.tenant_id = o.tenant_id
    AND c.endpoint = o.endpoint AND c.record_id = o.record_id
    AND c.version_id = o.version_id
  LEFT JOIN `{namespace}.v2_collections` AS h
    ON c.generation = h.generation AND c.tenant_id = h.tenant_id
    AND c.endpoint = h.endpoint AND c.record_id = h.record_id
    AND c.version_id = h.version_id AND c.collection_name = h.collection_name
  LEFT JOIN latest_parent AS p
    ON c.generation = p.generation AND c.tenant_id = p.tenant_id
    AND c.endpoint = p.endpoint AND c.record_id = p.record_id
  LEFT JOIN latest_collection AS l
    ON c.generation = l.generation AND c.tenant_id = l.tenant_id
    AND c.endpoint = l.endpoint AND c.record_id = l.record_id
    AND c.collection_name = l.collection_name
  LEFT JOIN membership AS m
    ON c.generation = m.generation AND c.tenant_id = m.tenant_id
    AND c.endpoint = m.endpoint AND c.record_id = m.record_id
    AND l.version_id = m.version_id AND c.collection_name = m.collection_name
    AND c.child_key = m.child_key
)
SELECT
  {columns},
  c.source_deleted,
  c.is_current,
  c.is_superseded,
  c.is_removed,
  c.source_deleted OR c.is_removed AS is_deleted,
  CASE
    WHEN c.revision_conflict THEN 'conflict'
    WHEN c.source_deleted THEN 'source_deleted'
    WHEN c.is_removed THEN 'removed'
    WHEN c.is_current THEN 'current'
    ELSE 'superseded'
  END AS record_state,
  CASE
    WHEN c.source_deleted THEN 'source_deleted'
    WHEN c.is_removed THEN 'missing_from_complete_parent'
    ELSE NULL
  END AS deletion_reason,
  c.revision_conflict
FROM classified AS c"""


def _columns(alias: str, fields: tuple[str, ...]) -> str:
    return ",\n  ".join(f"{alias}.{field}" for field in fields)


def view_queries(project: str, dataset: str) -> dict[str, str]:
    """Return dependency-ordered GoogleSQL SELECTs; never execute or emit DDL.

    Projects allow letters, digits and dashes, starting with a letter. Datasets
    allow letters, digits and underscores, starting with a letter or underscore.
    Ambiguous newest revisions block current projections without falling back
    to older versions; version_id breaks ties only for identical payload hashes.
    """
    if not isinstance(project, str) or not _PROJECT_IDENTIFIER.fullmatch(project):
        raise ValueError("invalid BigQuery project identifier")
    if not isinstance(dataset, str) or not _DATASET_IDENTIFIER.fullmatch(dataset):
        raise ValueError("invalid BigQuery dataset identifier")
    namespace = f"{project}.{dataset}"
    return {
        "v2_records": _RECORDS_SQL.format(
            namespace=namespace,
            columns=_columns("r", _RECORD_COLUMNS),
        ),
        "v2_collections": _COLLECTIONS_SQL.format(
            namespace=namespace,
            columns=_columns("c", _COLLECTION_COLUMNS),
        ),
        "v2_children": _CHILDREN_SQL.format(
            namespace=namespace,
            columns=_columns("c", _CHILD_COLUMNS),
        ),
        "v2_records_current": f"SELECT * FROM `{namespace}.v2_records` WHERE is_current",
        "v2_children_current": (
            f"SELECT * FROM `{namespace}.v2_children` WHERE is_current AND NOT is_deleted"
        ),
    }
