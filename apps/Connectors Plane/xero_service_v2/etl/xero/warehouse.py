"""Validated destinations for ordinary Xero warehouse data."""

import os
import re


DEFAULT_ORDINARY_STAGING_DATASET = "dw_staging_xero"
_DATASET_IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def ordinary_staging_dataset() -> str:
    """Return the configured ordinary Xero staging dataset."""
    dataset = os.environ.get("XERO_STAGING_DATASET", DEFAULT_ORDINARY_STAGING_DATASET)
    if not _DATASET_IDENTIFIER.fullmatch(dataset):
        raise ValueError("XERO_STAGING_DATASET must be a valid BigQuery dataset identifier")
    return dataset
