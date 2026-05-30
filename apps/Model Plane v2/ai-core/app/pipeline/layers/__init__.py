"""Pipeline layers — each layer receives and returns a PipelineContext."""

from __future__ import annotations

from app.pipeline.layers.L00_multimodal import run as L00_multimodal  # noqa: F401, N812
from app.pipeline.layers.L01_validate import run as L01_validate  # noqa: F401, N812
from app.pipeline.layers.L02_rate_limit import run as L02_rate_limit  # noqa: F401, N812
from app.pipeline.layers.L03_intent import run as L03_intent  # noqa: F401, N812
from app.pipeline.layers.L04_capability import run as L04_capability  # noqa: F401, N812
from app.pipeline.layers.L05_context import run as L05_context  # noqa: F401, N812
from app.pipeline.layers.L06_model_route import run as L06_model_route  # noqa: F401, N812
from app.pipeline.layers.L07_safety_pre import run as L07_safety_pre  # noqa: F401, N812
from app.pipeline.layers.L08_execute import run as L08_execute  # noqa: F401, N812
from app.pipeline.layers.L08b_rag_reflect import run as L08b_rag_reflect  # noqa: F401, N812
from app.pipeline.layers.L09_safety_post import run as L09_safety_post  # noqa: F401, N812
from app.pipeline.layers.L10_format import run as L10_format  # noqa: F401, N812

LAYERS: list[tuple[str, object]] = [
    ("multimodal", L00_multimodal),
    ("validate", L01_validate),
    ("rate_limit", L02_rate_limit),
    ("intent", L03_intent),
    ("capability", L04_capability),
    ("context", L05_context),
    ("model_route", L06_model_route),
    ("safety_pre", L07_safety_pre),
    ("execute", L08_execute),
    ("rag_reflect", L08b_rag_reflect),
    ("safety_post", L09_safety_post),
    ("format", L10_format),
]
