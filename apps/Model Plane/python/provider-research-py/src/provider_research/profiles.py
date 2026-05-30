"""Provider and model profile data structures."""

from __future__ import annotations

from enum import Enum

from pydantic import BaseModel, Field


class ModelTier(str, Enum):
    """Pricing / capability tier for a model."""

    economy = "economy"
    standard = "standard"
    premium = "premium"


class ModelProfile(BaseModel, frozen=True):
    """Specification of a single model offered by a provider."""

    model_id: str
    name: str
    tier: ModelTier
    input_cost_per_1k: float = Field(ge=0.0)
    output_cost_per_1k: float = Field(ge=0.0)
    context_window: int = Field(gt=0)
    supports_streaming: bool = True
    supports_tools: bool = False


class ProviderProfile(BaseModel, frozen=True):
    """Aggregate profile for an LLM provider."""

    provider_id: str
    name: str
    models: list[ModelProfile]
    supported_modalities: list[str] = Field(default_factory=list)
