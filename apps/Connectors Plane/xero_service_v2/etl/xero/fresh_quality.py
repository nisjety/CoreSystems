"""Stage-only gates over exact versions, never monetary or downstream assumptions."""

import re


def issue_query(project, dataset):
    if (not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", project)
            or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", dataset)):
        raise ValueError("invalid BigQuery identifier")
    ns = f"{project}.{dataset}"
    return f"""WITH actual_children AS (
  SELECT generation, tenant_id, endpoint, record_id, version_id, collection_name, COUNT(*) AS child_count
  FROM `{ns}.v2_child_versions`
  GROUP BY generation, tenant_id, endpoint, record_id, version_id, collection_name
), issues AS (
  SELECT generation, tenant_id, endpoint, record_id, version_id,
    'duplicate_record_version' AS issue, COUNT(*) - 1 AS violation_count
  FROM `{ns}.v2_record_versions`
  GROUP BY generation, tenant_id, endpoint, record_id, version_id HAVING COUNT(*) > 1
  UNION ALL
  SELECT generation, tenant_id, endpoint, record_id, version_id,
    'duplicate_child_position', CASE WHEN line_position IS NULL THEN COUNT(*) ELSE COUNT(*) - 1 END
  FROM `{ns}.v2_child_versions`
  GROUP BY generation, tenant_id, endpoint, record_id, version_id, collection_name, line_position
  HAVING COUNT(*) > 1 OR line_position IS NULL
  UNION ALL
  SELECT c.generation, c.tenant_id, c.endpoint, c.record_id, c.version_id, 'orphan_child', COUNT(*)
  FROM `{ns}.v2_child_versions` c
  LEFT JOIN `{ns}.v2_record_versions` p USING (generation, tenant_id, endpoint, record_id, version_id)
  LEFT JOIN `{ns}.v2_collection_versions` a USING (generation, tenant_id, endpoint, record_id, version_id, collection_name)
  WHERE p.version_id IS NULL OR a.version_id IS NULL
  GROUP BY c.generation, c.tenant_id, c.endpoint, c.record_id, c.version_id
  UNION ALL
  SELECT c.generation, c.tenant_id, c.endpoint, c.record_id, c.version_id,
    'child_count_mismatch', ABS(c.child_count - COALESCE(a.child_count, 0))
  FROM `{ns}.v2_collection_versions` c
  LEFT JOIN actual_children a USING (generation, tenant_id, endpoint, record_id, version_id, collection_name)
  WHERE c.child_count != COALESCE(a.child_count, 0)
  UNION ALL
  SELECT generation, tenant_id, endpoint, record_id, version_id, 'revision_conflict', 1
  FROM `{ns}.v2_records` WHERE revision_conflict
  UNION ALL
  SELECT generation, tenant_id, endpoint, NULL, NULL, 'receipt_count_mismatch',
    ABS(expected_records - record_count)
  FROM `{ns}.v2_ingest_receipts`
  WHERE http_status = 200 AND expected_records != record_count
    AND NOT STARTS_WITH(endpoint, 'accounting_get_')
    AND NOT ENDS_WITH(endpoint, '_list_snapshots') AND endpoint != 'attachment_metadata'
)
SELECT * FROM issues"""
