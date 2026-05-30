"""10-layer request processing pipeline.

Layers:
 1. Request validation
 2. Auth & rate limiting
 3. Intent classification
 4. Capability resolution (provider + model selection)
 5. Context enrichment (system prompt, RAG docs)
 6. Model routing (final provider dispatch config)
 7. Content safety — pre-check
 8. LLM execution (via reasoning_runtime)
 9. Content safety — post-check
10. Output formatting
"""

from __future__ import annotations

from app.pipeline.layers import (  # noqa: F401
    L01_validate,
    L02_rate_limit,
    L03_intent,
    L04_capability,
    L05_context,
    L06_model_route,
    L07_safety_pre,
    L08_execute,
    L09_safety_post,
    L10_format,
)
from app.pipeline.runner import run_pipeline, run_pipeline_stream  # noqa: F401
