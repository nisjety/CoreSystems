"""RemoteTriggerTool — fire a NATS event on the agent.triggers.* subject.

Used to kick off remote agent sessions, webhooks, or cross-plane automations
without waiting for a response (fire-and-forget).
"""

from __future__ import annotations

import json
import logging
from typing import Any

from app.nats_client import NatsManager
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

_TRIGGER_SUBJECT_PREFIX = "velion.agent.triggers"

# Injected at session startup via set_nats().
_nats: NatsManager | None = None


def set_nats(nats: NatsManager) -> None:
    global _nats
    _nats = nats


class RemoteTriggerTool:
    """Fire a fire-and-forget NATS event on the agent.triggers.* subject."""

    name = "remote_trigger"
    description = (
        "Publish a NATS trigger event on 'velion.agent.triggers.<target>'. "
        "Use to kick off remote agents, webhooks, or cross-plane automations "
        "without blocking for a response."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "target": {
                "type": "string",
                "description": "Trigger target key (appended to velion.agent.triggers.).",
            },
            "payload": {
                "type": "object",
                "description": "Arbitrary JSON payload to include with the trigger.",
                "default": {},
            },
            "source_run_id": {
                "type": "string",
                "description": "Run ID of the agent firing the trigger (for tracing).",
            },
        },
        "required": ["target"],
    }
    search_hint = "trigger remote nats event fire webhook automation"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Fire a NATS trigger event to kick off remote agents or automations. "
            "Fire-and-forget — does not wait for a result."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        target = input_data.get("target", "").strip()
        if not target:
            raise ValueError("'target' is required and must be non-empty")
        # Disallow subject injection via wildcards
        if any(c in target for c in ("*", ">", " ")):
            raise ValueError(f"Invalid trigger target '{target}': must not contain *, >, or spaces")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        nats = _nats
        if nats is None:
            return ToolResult(error="NATS not initialised — RemoteTriggerTool unavailable.")

        target = input_data["target"].strip()
        subject = f"{_TRIGGER_SUBJECT_PREFIX}.{target}"
        payload = {
            "target": target,
            "source_run_id": input_data.get("source_run_id"),
            **input_data.get("payload", {}),
        }

        try:
            await nats.publish_jetstream(subject, payload)
            logger.info(
                "remote_trigger_fired",
                extra={"subject": subject, "source_run_id": payload.get("source_run_id")},
            )
            return ToolResult(
                output=f"Trigger fired on {subject}",
                metadata={"subject": subject},
            )
        except Exception as exc:
            logger.exception("remote_trigger_failed", extra={"subject": subject})
            return ToolResult(error=f"Failed to fire trigger: {exc}")
