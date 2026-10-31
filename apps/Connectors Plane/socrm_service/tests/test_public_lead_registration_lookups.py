import os
import unittest
from unittest.mock import patch


os.environ.setdefault("MICROSOFT_CLIENT_ID", "test-microsoft-client")
os.environ.setdefault("MICROSOFT_CLIENT_SECRET", "test-microsoft-secret")
os.environ.setdefault("SOCRM_CLIENT_SECRET", "test-superoffice-secret")

import app as app_module


class _FakeLookupClient:
    def __init__(self, associate_pages=None, sale_types=None):
        self._associate_pages = associate_pages or [[]]
        self._sale_types = sale_types or []

    def get_associates(self, page=0, page_size=200, select=None):
        if page < len(self._associate_pages):
            return self._associate_pages[page]
        return []

    def get_sale_types(self, include_deleted=False):
        return self._sale_types


class PublicLeadRegistrationLookupTests(unittest.TestCase):
    def setUp(self):
        app_module.app.config["TESTING"] = True
        self.client = app_module.app.test_client()

    @patch.object(app_module, "_create_public_lookup_client")
    def test_lookup_endpoint_returns_sorted_options(self, mock_create_client):
        mock_create_client.return_value = _FakeLookupClient(
            associate_pages=[
                [
                    {"AssociateId": 9, "FullName": "Nora Nordmann", "IsActive": True},
                    {"AssociateId": 4, "FullName": "Arne Andersen", "IsActive": True},
                    {"AssociateId": 4, "FullName": "Arne Andersen", "IsActive": True},
                    {"AssociateId": 12, "FullName": "Deleted User", "Deleted": True},
                ],
                [],
            ],
            sale_types=[
                {"Id": 12, "Name": "Trade Show"},
                {"Id": 2, "Name": "Follow-up Meeting"},
            ],
        )

        response = self.client.get(
            "/api/public/lead-registration/lookups",
            headers={"Origin": "https://online3.superoffice.com"},
        )

        self.assertEqual(response.status_code, 200)
        self.assertEqual(
            response.get_json(),
            {
                "owners": [
                    {"id": "4", "label": "Arne Andersen"},
                    {"id": "9", "label": "Nora Nordmann"},
                ],
                "saleTypes": [
                    {"id": "2", "label": "Follow-up Meeting"},
                    {"id": "12", "label": "Trade Show"},
                ],
            },
        )
        self.assertEqual(
            response.headers.get("Access-Control-Allow-Origin"),
            "https://online3.superoffice.com",
        )

    def test_lookup_endpoint_fails_fast_when_context_identifier_is_missing(self):
        with patch.object(app_module.settings, "system_user_token", "system-user-token"), \
             patch.object(app_module.settings, "client_secret", "superoffice-client-secret"), \
             patch.object(app_module.settings, "system_user_context_identifier", ""):
            response = self.client.get("/api/public/lead-registration/lookups")

        self.assertEqual(response.status_code, 503)
        self.assertEqual(response.get_json()["error"], "Lead registration lookups are unavailable.")
        self.assertEqual(
            response.get_json()["message"],
            "The lookup service is not configured correctly.",
        )


if __name__ == "__main__":
    unittest.main()