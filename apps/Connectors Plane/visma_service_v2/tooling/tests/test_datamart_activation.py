"""DataMart pilot routing and candidate-query guards.

Verifies the pilot candidates stay on ERP until shadow parity is proven:
  * each candidate's `datamart.query_file` exists, is non-empty, and
    parses as a GraphQL query whose root selection matches `root_field`;
  * `root_field` is non-empty;
  * a `join_normalizer` is registered for each candidate;
  * a sample DataMart payload maps to the ERP-REST natural key via the named
    normalizer.

These tests intentionally read the real config files so the activation knobs
cannot silently drift.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

import pytest

from vsv2_tooling.normalizers import REGISTRY, general_ledger_balance

# Repo root = .../visma_service_v2 (tests live in tooling/tests/).
REPO_ROOT = Path(__file__).resolve().parents[2]
ENDPOINTS_DIR = REPO_ROOT / "config" / "endpoints"

# Pilot tenant — Aquatiq.
AQUATIQ_TENANT = "33f319b9-4d57-11ee-960c-025417856183"

# Query/normalizer candidates are not live source activations. Budget has no
# live sample, and GL balance shadow keys have not matched ERP yet.
PILOT_CANDIDATES = ["general_ledger_balances_v2", "budgets"]


def _load(endpoint: str) -> dict:
    return json.loads((ENDPOINTS_DIR / f"{endpoint}.json").read_text())


def _query_root_field(query_text: str) -> str:
    """Return the first top-level selection name inside `query <Op>(...) { <root>(...) }`."""
    body = query_text[query_text.index("{") + 1 :]
    m = re.search(r"([A-Za-z_][A-Za-z0-9_]*)\s*[({]", body)
    assert m is not None, "could not locate a root field in query"
    return m.group(1)


@pytest.mark.unit
@pytest.mark.parametrize("endpoint", PILOT_CANDIDATES)
def test_unproven_candidate_stays_on_erp_for_aquatiq(endpoint: str) -> None:
    cfg = _load(endpoint)
    dm = cfg["datamart"]
    assert dm["tenant_overrides"][AQUATIQ_TENANT] is False
    assert dm["ready"] is False
    assert cfg["source"] == "erp_rest"


@pytest.mark.unit
@pytest.mark.parametrize("endpoint", PILOT_CANDIDATES)
def test_candidate_query_file_exists_and_matches_root(endpoint: str) -> None:
    cfg = _load(endpoint)
    dm = cfg["datamart"]

    root_field = dm["root_field"]
    assert isinstance(root_field, str) and root_field, "root_field must be non-empty"

    query_file = dm["query_file"]
    # Activation must point at the canonical paginatable query under datamart/.
    assert query_file.startswith("config/queries/datamart/"), query_file

    query_path = REPO_ROOT / query_file
    assert query_path.exists(), f"query_file does not exist: {query_file}"

    text = query_path.read_text()
    assert text.strip(), "query file is empty"
    # Query must expose the cursor-paginator variables.
    for var in ("$lastId", "$updatedInDataMart", "$pageSize"):
        assert var in text, f"{query_file} missing paginator var {var}"
    assert _query_root_field(text) == root_field


@pytest.mark.unit
@pytest.mark.parametrize("endpoint", PILOT_CANDIDATES)
def test_candidate_normalizer_is_registered(endpoint: str) -> None:
    cfg = _load(endpoint)
    normalizer = cfg["datamart"].get("join_normalizer")
    assert normalizer, f"{endpoint} has no join_normalizer"
    assert normalizer in REGISTRY, f"{normalizer} not in normalizer REGISTRY"


@pytest.mark.unit
def test_journal_transactions_wired_but_not_activated_on_tier1() -> None:
    """journal_transactions now has a registered normalizer wired into its
    datamart block, but stays on ERP-REST: it is tier=1 (hourly SLA) and routing
    it through the lagging DataMart is risky, so its Aquatiq override is kept
    false. A false override means the (otherwise valid) normalizer never fires."""
    cfg = _load("journal_transactions")
    dm = cfg["datamart"]
    # Override deliberately kept false → endpoint is NOT activated.
    assert dm["tenant_overrides"][AQUATIQ_TENANT] is False
    assert dm["ready"] is False
    # Normalizer exists and is wired, ready for a future flip.
    assert dm.get("join_normalizer") == "normalize_journal_transaction"
    assert "normalize_journal_transaction" in REGISTRY
    # Path bug fix still applied.
    assert dm["query_file"] == "config/queries/datamart/journal_transactions.graphql"


@pytest.mark.unit
def test_sales_orders_stay_on_erp_until_lookup_and_line_parity() -> None:
    cfg = _load("sales_orders_v3")
    dm = cfg["datamart"]

    assert cfg["source"] == "erp_rest"
    assert dm["ready"] is False
    assert dm["pilot_override"] is False


@pytest.mark.unit
def test_gl_balance_dm_payload_maps_to_erp_natural_key() -> None:
    """A DataMart GL-balance record normalizes to the ERP natural key
    [account.number, subAccountCd, financialPeriod, ledger.id]."""
    out = general_ledger_balance.normalize_general_ledger_balance(
        {
            "id": "GLB|1",
            "companyId": 99,
            "accountId": 3000,
            "subAccountCd": "00-00",
            "finPeriodId": "202601",
            "ledgerId": 7,
            "beginningBalance": 100.0,
            "yearToDateBalance": 250.0,
            "curyId": "NOK",
            "lastModifiedDateTime": "2026-05-13T09:00:00Z",
            "updatedInDataMartDateTime": "2026-05-13T10:00:00Z",
        }
    )
    assert json.loads(out["account"])["number"] == "3000"
    assert out["subAccountCd"] == "00-00"
    assert out["financialPeriod"] == "202601"
    assert json.loads(out["ledger"])["id"] == "7"
