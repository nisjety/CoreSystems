"""Top-level smoke test — package imports cleanly."""

from __future__ import annotations

import pytest


@pytest.mark.unit
def test_package_imports() -> None:
    import vsv2_tooling
    import vsv2_tooling.bigquery
    import vsv2_tooling.dataform
    import vsv2_tooling.discovery
    import vsv2_tooling.normalizers

    assert vsv2_tooling.__version__ == "0.1.0"
