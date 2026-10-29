import json
import io
import shutil
import subprocess
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import MagicMock, patch
from urllib.error import HTTPError


PROJECT_DIR = Path(__file__).resolve().parents[1]
DATAFORM_COMPILE_TIMEOUT = "60s"
sys.path.insert(0, str(PROJECT_DIR))

import sync_to_gcp
from sync_to_gcp import source_files
import pull_from_gcp
import run_workflow


class XeroSyncTest(unittest.TestCase):
    @patch.object(pull_from_gcp, "directory_entries")
    def test_remote_files_recurses_through_workspace_directories(self, entries):
        entries.side_effect = [
            [{"file": "README.md"}, {"directory": "definitions"}],
            [{"file": "definitions/bronze/raw_history.js"}],
        ]
        self.assertEqual(
            pull_from_gcp.remote_files("token"),
            ["README.md", "definitions/bronze/raw_history.js"],
        )

    def test_workflow_target_is_limited_to_the_xero_project(self):
        self.assertEqual(
            run_workflow.parse_target("aquatiq-datavarehus.dw_bronze_xero.xero_raw_record_history"),
            {
                "database": "aquatiq-datavarehus",
                "schema": "dw_bronze_xero",
                "name": "xero_raw_record_history",
            },
        )
        with self.assertRaises(ValueError):
            run_workflow.parse_target("prj-dw-dev.dw_bronze_xero.xero_raw_record_history")
        with self.assertRaises(ValueError):
            run_workflow.parse_target("dw_bronze_xero.xero_raw_record_history")

    @patch.object(run_workflow, "request")
    def test_workflow_invocation_keeps_dependency_and_refresh_settings_explicit(self, request):
        request.return_value = {"name": "workflow"}
        workflow = run_workflow.create_workflow_invocation(
            "token",
            "compilation",
            [{"database": "aquatiq-datavarehus", "schema": "dw_bronze_xero", "name": "xero_raw_record_history"}],
            True,
            True,
        )
        self.assertEqual(workflow, "workflow")
        payload = request.call_args.args[3]
        self.assertTrue(
            payload["invocationConfig"]["fullyRefreshIncrementalTablesEnabled"]
        )
        self.assertTrue(payload["invocationConfig"]["transitiveDependenciesIncluded"])

    def test_only_dataform_project_files_are_uploaded(self):
        files = [path.relative_to(PROJECT_DIR).as_posix() for path in source_files()]
        self.assertIn("workflow_settings.yaml", files)
        self.assertIn("README.md", files)
        self.assertIn("definitions/bronze/typed_observations.js", files)
        self.assertNotIn("tests/test_project.py", files)

    @patch.object(sync_to_gcp, "request")
    def test_reads_remote_file_and_handles_only_not_found(self, request):
        request.return_value = {"fileContents": "aGVsbG8="}
        self.assertEqual(sync_to_gcp.remote_contents("token", "README.md"), b"hello")
        request.side_effect = HTTPError("", 404, "missing", None, None)
        self.assertIsNone(sync_to_gcp.remote_contents("token", "README.md"))
        request.side_effect = HTTPError("", 403, "denied", None, None)
        with self.assertRaises(HTTPError):
            sync_to_gcp.remote_contents("token", "README.md")

    @patch.object(sync_to_gcp, "request")
    def test_existing_directories_are_accepted(self, request):
        request.side_effect = HTTPError("", 409, "exists", None, None)
        sync_to_gcp.ensure_directory("token", "definitions/silver/fact_payment.sqlx")
        self.assertEqual(request.call_count, 2)
        request.side_effect = HTTPError("", 403, "denied", None, None)
        with self.assertRaises(HTTPError):
            sync_to_gcp.ensure_directory("token", "definitions/file.sqlx")

    def test_request_encodes_payload_without_logging_token(self):
        response = MagicMock()
        response.__enter__.return_value.read.return_value = b'{"ok":true}'
        with patch.object(sync_to_gcp, "urlopen", return_value=response) as urlopen:
            result = sync_to_gcp.request("secret", "POST", ":writeFile", {"path": "a"})
        self.assertEqual(result, {"ok": True})
        self.assertEqual(urlopen.call_args.args[0].get_header("Authorization"), "Bearer secret")

    @patch.object(sync_to_gcp, "source_files")
    @patch.object(sync_to_gcp, "remote_contents", return_value=None)
    @patch.object(sync_to_gcp, "gcloud_value", return_value="token")
    @patch.object(sync_to_gcp, "request")
    def test_dry_run_does_not_write(self, request, _gcloud, _remote, files):
        files.return_value = [PROJECT_DIR / "README.md"]
        output = io.StringIO()
        with patch.object(sys, "argv", ["sync_to_gcp.py"]), redirect_stdout(output):
            self.assertEqual(sync_to_gcp.main(), 0)
        self.assertIn("Would upload README.md", output.getvalue())
        request.assert_not_called()

    @patch.object(sync_to_gcp, "source_files")
    @patch.object(sync_to_gcp, "remote_contents", return_value=None)
    @patch.object(sync_to_gcp, "gcloud_value", side_effect=["token", "user@example.com"])
    @patch.object(sync_to_gcp, "ensure_directory")
    @patch.object(sync_to_gcp, "request", return_value={})
    def test_apply_uploads_and_commits(self, request, directory, _gcloud, _remote, files):
        files.return_value = [PROJECT_DIR / "README.md"]
        output = io.StringIO()
        with patch.object(sys, "argv", ["sync_to_gcp.py", "--apply"]), redirect_stdout(output):
            self.assertEqual(sync_to_gcp.main(), 0)
        self.assertEqual(request.call_count, 2)
        directory.assert_called_once_with("token", "README.md")
        self.assertIn("Uploading README.md", output.getvalue())


@unittest.skipUnless(shutil.which("dataform"), "dataform CLI is required")
class XeroDataformProjectTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        result = subprocess.run(
            [
                "dataform",
                "compile",
                str(PROJECT_DIR),
                "--json",
                "--timeout",
                DATAFORM_COMPILE_TIMEOUT,
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        cls.graph = json.loads(result.stdout)

    def test_compiles_only_into_main_project(self):
        self.assertFalse(self.graph["graphErrors"])
        self.assertEqual(
            self.graph["projectConfig"]["defaultDatabase"], "aquatiq-datavarehus"
        )
        for action in self.graph["tables"] + self.graph["declarations"]:
            self.assertEqual(action["target"]["database"], "aquatiq-datavarehus")

    def test_typed_observations_are_protected_incrementals(self):
        targets = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        for entity in ("accounts", "invoices", "payments"):
            with self.subTest(entity=entity):
                table = targets[("dw_bronze_xero", f"xero_{entity}")]
                self.assertEqual(table["type"], "incremental")
                self.assertIn("SHA256", table["query"])
                self.assertIn("dw_staging_xero", table["query"])

    def test_customer_payments_keep_deleted_rows_visible(self):
        tables = {table["target"]["name"]: table for table in self.graph["tables"]}
        customer = tables["fact_customer_payment"]
        self.assertIn("ACCRECPAYMENT", customer["query"])
        self.assertIn("fact_payment", customer["query"])
        self.assertIn("is_deleted", tables["fact_payment"]["query"])

    def test_payment_amount_has_no_unverified_currency_conversion(self):
        tables = {table["target"]["name"]: table for table in self.graph["tables"]}
        query = tables["fact_payment"]["query"]
        self.assertIn("bank_amount", query)
        self.assertIn("currency_rate", query)
        self.assertNotIn("amount_base", query)

    def test_payment_currency_uses_verified_embedded_invoice_source(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        bronze = tables[("dw_bronze_xero", "xero_payment_invoice_currency")]["query"]
        silver = tables[("dw_1_silver_xero", "fact_payment")]["query"]
        customer = tables[("xero_gold_finance", "fact_customer_payment")]["query"]
        supplier = tables[("xero_gold_finance", "fact_supplier_payment")]["query"]

        self.assertIn("xero_raw_record_current", bronze)
        self.assertIn("Invoice.CurrencyCode", bronze)
        self.assertIn("Invoice.InvoiceID", bronze)
        self.assertIn("source.invoice_id = p.invoice_id", silver)
        self.assertIn("source.observed_at = p.synced_at", silver)
        self.assertIn("source.source_updated_at = p.updated_at", silver)
        self.assertIn("source.payment_amount = p.amount", silver)
        self.assertIn("is_raw_typed_mismatch", silver)
        self.assertIn("is_invoice_type_unresolved", silver)
        self.assertIn("REGEXP_CONTAINS", silver)
        self.assertIn("is_invoice_type_mismatch", silver)
        self.assertIn("payment_embedded_invoice", silver)
        for query in (customer, supplier):
            self.assertIn("embedded_invoice_currency_code", query)
            self.assertIn("is_currency_conflict", query)
            self.assertIn("is_invoice_type_mismatch", query)
            self.assertIn("is_raw_typed_mismatch", query)
            self.assertIn("is_invoice_type_unresolved", query)
            self.assertIn("invoice_currency_source", query)

    def test_non_invoice_payments_have_separate_gold_contract(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        silver = tables[("dw_1_silver_xero", "fact_non_invoice_payment")]["query"]
        gold = tables[("xero_gold_finance", "fact_non_invoice_payment")]["query"]
        self.assertIn("ACCRECPAYMENT", silver)
        self.assertIn("ACCPAYPAYMENT", silver)
        self.assertIn("APCREDITPAYMENT", gold)
        self.assertIn("APOVERPAYMENTPAYMENT", gold)
        self.assertIn("ARCREDITPAYMENT", gold)
        self.assertIn("fact_credit_note", gold)
        self.assertIn("fact_overpayment", gold)
        self.assertIn("is_raw_typed_mismatch", gold)
        self.assertIn("is_parent_currency_mismatch", gold)
        assertions = {a["target"]["name"]: a for a in self.graph["assertions"]}
        reconciliation = assertions["xero_gold_cash_document_grain_reconciliation"]["query"]
        self.assertIn("'non_invoice_payment'", reconciliation)
        coverage = tables[("dw_bronze_xero", "xero_endpoint_warehouse_coverage")]["query"]
        self.assertIn("xero_gold_finance.fact_non_invoice_payment", coverage)

    def test_invoice_lines_preserve_removed_rows_without_reporting_them(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        bronze = tables[("dw_bronze_xero", "xero_invoice_current_line_keys")]["query"]
        silver = tables[("dw_1_silver_xero", "fact_invoice_line")]["query"]
        customer = tables[("xero_gold_finance", "fact_customer_invoice_line")]["query"]
        supplier = tables[("xero_gold_finance", "fact_supplier_invoice_line")]["query"]
        tax = tables[("dw_1_silver_xero", "fact_tax_activity")]["query"]
        self.assertIn("xero_raw_record_current", bronze)
        self.assertIn("LineItemID", bronze)
        self.assertIn("is_removed_from_source", silver)
        self.assertIn("COUNTIF(NOT", silver)
        self.assertIn("is_raw_line_aligned", silver)
        self.assertIn("source.observed_at = h.synced_at", silver)
        self.assertIn("JSON_TYPE", silver)
        for query in (customer, supplier, tax):
            self.assertIn("is_removed_from_source", query)
        assertion_names = {a["target"]["name"] for a in self.graph["assertions"]}
        self.assertIn("xero_invoice_current_line_reconciliation", assertion_names)
        reconciliation = next(a["query"] for a in self.graph["assertions"]
                              if a["target"]["name"] == "xero_invoice_current_line_reconciliation")
        self.assertIn("unaligned_current_line_count", reconciliation)

    def test_commercial_lines_preserve_removed_rows_without_reporting_them(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        bronze = tables[("dw_bronze_xero", "xero_commercial_current_line_keys")]["query"]
        self.assertIn("xero_raw_record_current", bronze)
        for endpoint, silver_name, gold_schema, gold_name in (
            ("quotes", "fact_quote_line", "xero_gold_sales", "fact_quote_line"),
            ("purchase_orders", "fact_purchase_order_line", "xero_gold_procurement", "fact_purchase_order_line"),
        ):
            silver = tables[("dw_1_silver_xero", silver_name)]["query"]
            gold = tables[(gold_schema, gold_name)]["query"]
            self.assertIn(f"endpoint = '{endpoint}'", silver)
            self.assertIn("is_removed_from_source", silver)
            self.assertIn("is_raw_line_aligned", silver)
            self.assertIn("is_current_line_set_observed", silver)
            self.assertIn("is_removed_from_source", gold)
            self.assertIn("NOT l.is_removed_from_source", gold)
        assertions = {a["target"]["name"]: a["query"] for a in self.graph["assertions"]}
        self.assertIn("xero_commercial_current_line_reconciliation", assertions)
        self.assertIn("unaligned_current_line_count", assertions["xero_commercial_current_line_reconciliation"])

    def test_source_keys_and_missing_markers_are_explicit(self):
        assertion_names = {a["target"]["name"] for a in self.graph["assertions"]}
        tables = {table["target"]["name"]: table for table in self.graph["tables"]}
        for entity in ("accounts", "invoices", "payments"):
            self.assertIn(f"xero_{entity}_source_keys", assertion_names)
        for name in ("dim_account", "fact_invoice", "payment_versions"):
            self.assertIn("is_missing_from_staging", tables[name]["query"])

    def test_aging_gold_facts_expose_the_canonical_contact_identifier(self):
        tables = {table["target"]["name"]: table for table in self.graph["tables"]}
        for name in (
            "fact_accounts_payable_aging_report_amount",
            "fact_accounts_receivable_aging_report_amount",
        ):
            with self.subTest(name=name):
                table = tables[name]
                query = table["query"]
                self.assertIn("amount.contact_id,", query)
                source = (
                    PROJECT_DIR / "definitions" / "gold" / "finance" / f"{name}.sqlx"
                ).read_text()
                self.assertIn('"contact_id"', source)

    def test_invoice_attachment_metadata_has_source_lifecycle_and_gold_contract(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        lists = tables[("dw_bronze_xero", "xero_invoice_attachment_lists")]
        bronze = tables[("dw_bronze_xero", "xero_invoice_attachment_observations")]
        silver = tables[("dw_1_silver_xero", "dim_invoice_attachment")]
        gold = tables[("xero_gold_finance", "dim_invoice_attachment")]
        self.assertIn("xero_raw_direct_response_history", lists["query"])
        self.assertIn("getInvoice", lists["query"])
        self.assertIn("JSON_TYPE", lists["query"])
        self.assertIn("payload_invoice_id", lists["query"])
        self.assertIn("parent_record_id", lists["query"])
        self.assertIn("AttachmentID", lists["query"])
        self.assertIn("HasAttachments", lists["query"])
        self.assertIn("TRIM", lists["query"])
        self.assertIn("xero_invoice_attachment_lists", bronze["query"])
        self.assertIn("is_removed_from_source", silver["query"])
        self.assertIn("latest_complete_list", silver["query"])
        self.assertIn("is_removed_from_source", gold["query"])
        self.assertNotIn("file_bytes", gold["query"])
        self.assertNotIn("file_name", gold["query"])
        self.assertNotIn("source_url", gold["query"])

    def test_finance_reporting_readiness_keeps_mapping_and_nok_evidence_explicit(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        mapping_coverage = tables[("dw_1_silver_xero", "audit_account_mapping_coverage")]
        mapping_readiness = tables[("xero_gold_finance", "audit_account_mapping_readiness")]
        fx_readiness = tables[("xero_gold_finance", "audit_nok_conversion_readiness")]
        reporting_readiness = tables[("xero_datamart", "finance_reporting_readiness")]

        self.assertIn("native_account_class_fallback", mapping_coverage["query"])
        self.assertIn("is_group_chart_mapping_ready", mapping_readiness["query"])
        self.assertIn("is_external_fx_feed_available", fx_readiness["query"])
        self.assertIn("is_finance_reporting_ready", reporting_readiness["query"])
        self.assertIn("amount_nok", fx_readiness["query"])
        self.assertIn("is_nok_conversion_ready", fx_readiness["query"])

    def test_external_market_rate_bridge_is_separate_from_xero_source_rates(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        market_rate = tables[("dw_1_silver_xero", "dim_currency_market_rate")]
        daily_rate = tables[("xero_gold_common", "dim_currency_market_rate_to_nok_daily")]
        fx_readiness = tables[("xero_gold_finance", "audit_nok_conversion_readiness")]

        self.assertIn("dw_staging_currency", market_rate["query"])
        self.assertIn("is_inverse_rate", market_rate["query"])
        self.assertIn("canonical_nok_rates", daily_rate["query"])
        self.assertIn("is_rate_filled_forward", daily_rate["query"])
        self.assertIn("is_market_rate_available_for_functional_currency", fx_readiness["query"])
        self.assertIn("is_nok_conversion_ready", fx_readiness["query"])
        self.assertNotIn("FALSE AS is_nok_conversion_ready", fx_readiness["query"])
        self.assertIn("xero_nok_conversion_policy", fx_readiness["query"])

    def test_account_mapping_reference_is_effective_dated_and_fail_safe(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        declarations = {
            (declaration["target"]["schema"], declaration["target"]["name"])
            for declaration in self.graph["declarations"]
        }
        mapping_history = tables[("dw_1_silver_xero", "dim_account_classification_history")]
        mapping_readiness = tables[("xero_gold_finance", "audit_account_mapping_readiness")]
        assertion_names = {assertion["target"]["name"] for assertion in self.graph["assertions"]}

        self.assertIn(("xero_reference", "xero_account_reporting_mapping"), declarations)
        self.assertIn(("xero_reference", "xero_nok_conversion_policy"), declarations)
        self.assertIn("xero_account_reporting_mapping", mapping_history["query"])
        self.assertIn("mapping_id", mapping_history["query"])
        self.assertIn("finance_governed_account_mapping", mapping_history["query"])
        self.assertIn("missing_account_coverage", mapping_readiness["query"])
        self.assertIn("xero_account_reporting_mapping_validity", assertion_names)

    def test_finance_controls_fail_closed_for_invalid_rates_and_ambiguous_reference_rows(self):
        tables = {
            (table["target"]["schema"], table["target"]["name"]): table
            for table in self.graph["tables"]
        }
        daily_rate = tables[("xero_gold_common", "dim_currency_market_rate_to_nok_daily")]
        fx_readiness = tables[("xero_gold_finance", "audit_nok_conversion_readiness")]
        account_history = tables[("dw_1_silver_xero", "dim_account_classification_history")]
        finance_activity = tables[("xero_datamart", "finance_activity")]

        self.assertIn("market_exchange_rate_to_currency > 0", daily_rate["query"])
        self.assertIn("latest_market_rate_observation", fx_readiness["query"])
        self.assertIn("source_timestamp <= CURRENT_TIMESTAMP()", fx_readiness["query"])
        self.assertIn("policy_match_count", fx_readiness["query"])
        self.assertIn("policy_match_count = 1", fx_readiness["query"])
        self.assertIn("mapping_match_count", account_history["query"])
        self.assertIn("mapping_match_count = 1", account_history["query"])
        self.assertIn("mapping_match_count", finance_activity["query"])
        self.assertIn("mapping_match_count = 1", finance_activity["query"])

    def test_release_readiness_requires_source_reconciliation_and_gold_contracts(self):
        tables = {table["target"]["name"]: table for table in self.graph["tables"]}
        readiness = tables["xero_current_accounting_warehouse_release_readiness"]
        self.assertEqual(readiness["target"]["schema"], "dw_bronze_xero")
        self.assertIn("xero_raw_run_reconciliation", readiness["query"])
        self.assertIn("gold_contract_operation_count", readiness["query"])
        self.assertIn("is_ready_for_standard_accounting_warehouse", readiness["query"])
        self.assertIn("source_freshness_state", readiness["query"])
        self.assertIn("is_fresh_within_default_48_hours", readiness["query"])
        self.assertIn("aging_portfolio_release_state", readiness["query"])
        self.assertIn("is_ready_for_aging_portfolio_release", readiness["query"])
        self.assertIn("is_current_aged_contact_reporting_required", readiness["query"])
        assertion_names = {assertion["target"]["name"] for assertion in self.graph["assertions"]}
        self.assertIn("xero_current_accounting_release_state_consistency", assertion_names)

    def test_standard_coverage_uses_latest_complete_accounting_run(self):
        tables = {table["target"]["name"]: table for table in self.graph["tables"]}
        query = tables["xero_endpoint_coverage"]["query"]
        self.assertIn("run_cohort_counts AS", query)
        self.assertIn("latest_standard_cohort AS", query)
        self.assertIn("a.run_id = standard.run_id", query)
        self.assertIn("a.run_mode != 'aged-contact-reports'", query)
        self.assertIn("latest_reporting_cohort AS", query)
        self.assertIn("latest_aged_cohort AS", query)
        self.assertIn("all_latest_attempts AS", query)
        self.assertIn("latest.run_id AS latest_attempt_run_id", query)
        self.assertIn("latest_attempt_failed", query)
        self.assertIn(
            "latest_attempt_failed_operation_count",
            tables["xero_endpoint_coverage_summary"]["query"],
        )


if __name__ == "__main__":
    unittest.main()
