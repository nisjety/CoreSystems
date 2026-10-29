import pytest

from etl.xero.fresh_quality import issue_query


def test_quality_gates_preserve_history_and_do_not_audit_downstream():
    sql = issue_query("prj-dw-dev", "dw_staging_xero")
    for gate in ("duplicate_record_version", "duplicate_child_position", "orphan_child",
                 "child_count_mismatch", "revision_conflict", "receipt_count_mismatch"):
        assert gate in sql
    assert "LEFT JOIN" in sql
    assert "v2_collection_versions" in sql
    assert "DELETE" not in sql and "UPDATE" not in sql
    assert "silver" not in sql and "ods" not in sql


def test_quality_rejects_invalid_identifiers():
    with pytest.raises(ValueError, match="identifier"):
        issue_query("project", "dataset`;DROP")
