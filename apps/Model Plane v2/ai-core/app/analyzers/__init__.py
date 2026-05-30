"""Analyzer abstraction layer.

Provides a unified interface for document/content analysis routing
between Azure Document Intelligence, Azure Content Understanding,
and Mistral Document AI.
"""

from app.analyzers.base import AnalyzerBackend, AnalyzerRequest, AnalyzerResult
from app.analyzers.router import get_analyzer, analyze

__all__ = [
    "AnalyzerBackend",
    "AnalyzerRequest",
    "AnalyzerResult",
    "get_analyzer",
    "analyze",
]
