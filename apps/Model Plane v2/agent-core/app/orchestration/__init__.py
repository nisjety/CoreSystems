"""PydanticAI orchestration layer — Phase 1.2.

Wraps the v2 LLM + tool infrastructure behind PydanticAI's Agent
abstraction so we get typed outputs, structured tool dispatch,
dependency injection, and automatic retry/validation.

Modules:
  deps          – AgentDeps dataclass injected into every PydanticAI run
  velion_model  – Custom PydanticAI Model backed by LLMClient → llm-worker
  agent         – create_velion_agent() factory for building configured agents
"""

from app.orchestration.deps import AgentDeps
from app.orchestration.velion_model import VelionModel
from app.orchestration.agent import create_velion_agent

__all__ = ["AgentDeps", "VelionModel", "create_velion_agent"]
