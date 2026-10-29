"""Tables the Visma APIs do not serve, built from tables they do.

v1 builds these in its sync code; v2 lands only what the APIs return, so each derived table has a
function here that reproduces v1's rule, with tests, and can run over any source of rows (local
bronze today, BigQuery once the write path is tested).
"""

from .price_classes import price_classes

__all__ = ["price_classes"]
