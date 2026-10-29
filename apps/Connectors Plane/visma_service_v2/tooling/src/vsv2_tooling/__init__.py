"""Python tooling for visma_service_v2.

Phases:
    0  discovery   — GraphQL introspection, coverage matrix, rate-limit probe
    1+ normalizers — DataMart record → bronze legacy shape
    4  bigquery    — chunked MERGE loader + shadow-mode diff
    8  dataform    — bronze schema assertions (CI guard)
"""

__version__ = "0.1.0"
