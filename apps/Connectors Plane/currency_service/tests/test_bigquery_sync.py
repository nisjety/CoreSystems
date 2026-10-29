import unittest
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from bigquery_sync import (
    HISTORY_POWERBI_VIEW,
    RATE_POWERBI_VIEW,
    ensure_powerbi_views_exist,
    ensure_tables_exist,
    get_history_powerbi_view_sql,
    get_rate_powerbi_view_sql,
)


class BigQueryViewSqlTests(unittest.TestCase):
    def test_rate_powerbi_view_sql_contains_explicit_casts(self):
        sql = get_rate_powerbi_view_sql("project", "dataset")

        self.assertIn(f"CREATE OR REPLACE VIEW `project.dataset.{RATE_POWERBI_VIEW}`", sql)
        self.assertIn("SAFE_CAST(rate AS FLOAT64) AS rate", sql)
        self.assertIn("FROM `project.dataset.Rate`", sql)

    def test_history_powerbi_view_sql_contains_explicit_casts(self):
        sql = get_history_powerbi_view_sql("project", "dataset")

        self.assertIn(f"CREATE OR REPLACE VIEW `project.dataset.{HISTORY_POWERBI_VIEW}`", sql)
        self.assertIn("SAFE_CAST(rate AS FLOAT64) AS rate", sql)
        self.assertIn("FROM `project.dataset.Historyrates`", sql)


class EnsurePowerBIViewsTests(unittest.TestCase):
    @patch("bigquery_sync.get_gcp_client")
    def test_ensure_powerbi_views_exist_runs_both_queries(self, mock_get_gcp_client):
        mock_client = MagicMock()
        mock_client.project_id = "project"
        mock_client.dataset_id = "dataset"
        mock_query_job = MagicMock()
        mock_query_job.result.return_value = None
        mock_client.bigquery.query.return_value = mock_query_job
        mock_get_gcp_client.return_value = mock_client

        result = ensure_powerbi_views_exist()

        self.assertTrue(result)
        self.assertEqual(mock_client.bigquery.query.call_count, 2)

    @patch("bigquery_sync.get_gcp_client")
    def test_ensure_powerbi_views_exist_returns_false_on_query_failure(self, mock_get_gcp_client):
        mock_client = MagicMock()
        mock_client.project_id = "project"
        mock_client.dataset_id = "dataset"
        mock_client.bigquery.query.side_effect = RuntimeError("boom")
        mock_get_gcp_client.return_value = mock_client

        result = ensure_powerbi_views_exist()

        self.assertFalse(result)

    @patch("bigquery_sync.get_gcp_client")
    def test_ensure_powerbi_views_exist_returns_false_when_second_query_fails(self, mock_get_gcp_client):
        mock_client = MagicMock()
        mock_client.project_id = "project"
        mock_client.dataset_id = "dataset"

        first_query_job = MagicMock()
        first_query_job.result.return_value = None
        mock_client.bigquery.query.side_effect = [first_query_job, RuntimeError("boom")]
        mock_get_gcp_client.return_value = mock_client

        result = ensure_powerbi_views_exist()

        self.assertFalse(result)


class EnsureTablesExistTests(unittest.TestCase):
    @patch("bigquery_sync.ensure_powerbi_views_exist")
    @patch("bigquery_sync.get_gcp_client")
    def test_ensure_tables_exist_creates_views_after_tables(self, mock_get_gcp_client, mock_ensure_views):
        mock_client = MagicMock()
        mock_client.create_bigquery_table.side_effect = [
            SimpleNamespace(full_table_id="project:dataset.Rate"),
            SimpleNamespace(full_table_id="project:dataset.Historyrates"),
        ]
        mock_get_gcp_client.return_value = mock_client
        mock_ensure_views.return_value = True

        result = ensure_tables_exist()

        self.assertTrue(result)
        mock_ensure_views.assert_called_once_with()

    @patch("bigquery_sync.ensure_powerbi_views_exist")
    @patch("bigquery_sync.get_gcp_client")
    def test_ensure_tables_exist_fails_when_view_creation_fails(self, mock_get_gcp_client, mock_ensure_views):
        mock_client = MagicMock()
        mock_client.create_bigquery_table.side_effect = [
            SimpleNamespace(full_table_id="project:dataset.Rate"),
            SimpleNamespace(full_table_id="project:dataset.Historyrates"),
        ]
        mock_get_gcp_client.return_value = mock_client
        mock_ensure_views.return_value = False

        result = ensure_tables_exist()

        self.assertFalse(result)


if __name__ == "__main__":
    unittest.main()
