import sys
from unittest.mock import MagicMock

from etl import bootstrap_fresh


def test_endpoint_bootstrap_does_not_create_legacy_views_or_labels(monkeypatch):
    monkeypatch.setattr(sys, "argv", [
        "bootstrap_fresh", "--project", "prj-dw-dev",
        "--dataset", "dw_staging_xero_main", "--layout", "endpoint",
    ])
    endpoint_writer, old_writer = MagicMock(), MagicMock()
    monkeypatch.setattr(bootstrap_fresh, "EndpointBQWriter", lambda *args: endpoint_writer, raising=False)
    monkeypatch.setattr(bootstrap_fresh, "FreshBQWriter", lambda *args: old_writer)
    monkeypatch.setattr(bootstrap_fresh, "install_view", MagicMock())

    bootstrap_fresh.main()

    endpoint_writer.bootstrap.assert_called_once_with()
    old_writer.bootstrap.assert_not_called()
    bootstrap_fresh.install_view.assert_not_called()


def test_default_bootstrap_uses_current_warehouse_dataset(monkeypatch):
    monkeypatch.setattr(sys, "argv", ["bootstrap_fresh", "--project", "prj-dw-dev"])
    raw_writer, endpoint_writer, old_writer = MagicMock(), MagicMock(), MagicMock()
    raw_factory = MagicMock(return_value=raw_writer)
    monkeypatch.setattr(bootstrap_fresh, "WarehouseRawBQWriter", raw_factory)
    monkeypatch.setattr(bootstrap_fresh, "EndpointBQWriter", lambda *args: endpoint_writer)
    monkeypatch.setattr(bootstrap_fresh, "FreshBQWriter", lambda *args: old_writer)

    bootstrap_fresh.main()

    raw_factory.assert_called_once_with("prj-dw-dev", "dw_staging_xero")
    raw_writer.bootstrap.assert_called_once_with()
    endpoint_writer.bootstrap.assert_not_called()
    old_writer.bootstrap.assert_not_called()
