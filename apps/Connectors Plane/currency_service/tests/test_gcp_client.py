import os
import unittest
from unittest.mock import MagicMock, patch

from google.api_core.exceptions import NotFound
from google.cloud import bigquery

from shared.gcp_client import GCPClient, get_gcp_client


def make_client(location=None) -> GCPClient:
    return GCPClient(
        project_id="project",
        dataset_id="dataset",
        bucket_name=None,
        bigquery=MagicMock(),
        storage=MagicMock(),
        location=location,
    )


class CreateDatasetLocationTests(unittest.TestCase):
    """A dataset the service has to create must land where the warehouse reads it."""

    def created_dataset(self, client: GCPClient) -> bigquery.Dataset:
        client.bigquery.get_dataset.side_effect = NotFound("no dataset")
        client.bigquery.get_table.side_effect = NotFound("no table")
        client.create_bigquery_table("Rate", schema=[])
        return client.bigquery.create_dataset.call_args.args[0]

    def test_missing_dataset_is_created_in_the_configured_location(self):
        dataset = self.created_dataset(make_client(location="europe-north2"))

        self.assertEqual(dataset.location, "europe-north2")

    def test_without_a_location_the_dataset_is_created_as_before(self):
        dataset = self.created_dataset(make_client(location=None))

        self.assertIsNone(dataset.location)


class GetGcpClientLocationTests(unittest.TestCase):
    ENV = {"GCP_PROJECT_ID": "project", "GCP_DATASET_ID": "dataset"}

    def build(self, extra_env: dict) -> tuple[GCPClient, MagicMock]:
        env = {**self.ENV, **extra_env}
        with patch.dict(os.environ, env, clear=True), \
                patch("shared.gcp_client.bigquery.Client") as bq_client, \
                patch("shared.gcp_client.storage.Client"):
            return get_gcp_client(), bq_client

    def test_location_comes_from_gcp_bq_location(self):
        client, bq_client = self.build({"GCP_BQ_LOCATION": "europe-north2"})

        self.assertEqual(client.location, "europe-north2")
        self.assertEqual(bq_client.call_args.kwargs["location"], "europe-north2")

    def test_location_is_unset_when_the_variable_is_missing_or_empty(self):
        for extra in ({}, {"GCP_BQ_LOCATION": ""}):
            with self.subTest(extra=extra):
                client, bq_client = self.build(extra)

                self.assertIsNone(client.location)
                self.assertIsNone(bq_client.call_args.kwargs["location"])


if __name__ == "__main__":
    unittest.main()
