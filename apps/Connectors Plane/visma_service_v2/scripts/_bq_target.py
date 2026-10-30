"""The BigQuery project and staging dataset the scripts work against.

The same settings the pipeline uses: `VSV2_BIGQUERY_PROJECT` and `VSV2_BIGQUERY_DATASET_STAGING`,
from the environment or from `.env`. Pointing the pipeline at another project therefore points the
scripts there too. Without either, the fallbacks below apply: the data warehouse project the
pipeline has written to since 2026-10-09 (`aquatiq-datavarehus`), and the staging dataset in it.
"""

from __future__ import annotations

import os
from pathlib import Path

FALLBACK_PROJECT = "aquatiq-datavarehus"
FALLBACK_DATASET = "dw_staging_visma"

_ENV_FILE = Path(__file__).resolve().parent.parent / ".env"


def _setting(name: str, fallback: str) -> str:
    value = os.environ.get(name)
    if not value and _ENV_FILE.is_file():
        for line in _ENV_FILE.read_text().splitlines():
            key, _, text = line.partition("=")
            if key.strip() == name:
                value = text.strip().strip("\"'")
                break
    return value or fallback


PROJECT = _setting("VSV2_BIGQUERY_PROJECT", FALLBACK_PROJECT)
DATASET = _setting("VSV2_BIGQUERY_DATASET_STAGING", FALLBACK_DATASET)
