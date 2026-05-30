"""Runner service — core business logic for runner lifecycle management.

Handles:
- Runner registration/deregistration
- Heartbeat processing
- Task claiming (with lease acquisition)
- Task completion/cancellation
- Dead runner detection
"""

from __future__ import annotations

import logging
from typing import Any

from app import repository
from app.artifact_store import generate_download_url, generate_upload_url
from app.domain import (
    ArtifactKind,
    ArtifactMetadata,
    RunnerRecord,
    RunnerRegistration,
    RunnerStatus,
    TaskClaim,
    TaskCancel,
    TaskRecord,
    TaskResult,
)
from app.nats_publisher import RunnerPublisher
from app.redis_client import (
    acquire_task_lease,
    is_runner_alive,
    record_heartbeat,
    release_task_lease,
)
from app.workspace import ensure_workspace

logger = logging.getLogger(__name__)


class RunnerService:
    """Orchestrates runner lifecycle events."""

    def __init__(self, publisher: RunnerPublisher) -> None:
        self._publisher = publisher

    # ------------------------------------------------------------------
    # Registration
    # ------------------------------------------------------------------

    async def handle_register(self, data: dict[str, Any]) -> None:
        """Process a velion.runner.register message."""
        reg = RunnerRegistration.model_validate(data)
        record = RunnerRecord(
            runner_id=reg.runner_id,
            capabilities=[c.value for c in reg.capabilities],
            max_concurrent=reg.max_concurrent,
            labels=reg.labels,
            status=RunnerStatus.IDLE,
            workspace_id=reg.workspace_id,
        )
        await repository.upsert_runner(record)
        await record_heartbeat(reg.runner_id)
        logger.info("runner_registered", extra={"runner_id": reg.runner_id})

    async def handle_deregister(self, runner_id: str) -> bool:
        """Remove a runner from the pool."""
        deleted = await repository.delete_runner(runner_id)
        if deleted:
            await self._publisher.publish_runner_deregistered(runner_id, "explicit")
            logger.info("runner_deregistered", extra={"runner_id": runner_id})
        return deleted

    # ------------------------------------------------------------------
    # Heartbeat
    # ------------------------------------------------------------------

    async def handle_heartbeat(self, runner_id: str, data: dict[str, Any]) -> None:
        """Process a velion.runner.heartbeat.{runner_id} message."""
        status_str = data.get("status", "idle")
        try:
            status = RunnerStatus(status_str)
        except ValueError:
            status = RunnerStatus.IDLE

        current_task_id = data.get("current_task_id")

        await record_heartbeat(runner_id)
        await repository.update_runner_heartbeat(runner_id, status, current_task_id)

    # ------------------------------------------------------------------
    # Task claiming
    # ------------------------------------------------------------------

    async def handle_claim(self, data: dict[str, Any]) -> None:
        """Process a velion.runner.claim message — assign task to best runner."""
        claim = TaskClaim.model_validate(data)

        # Create the task record
        task = TaskRecord(
            task_id=claim.task_id,
            run_id=claim.run_id,
            session_id=claim.session_id,
            workspace_id=claim.workspace_id,
            tool_name=claim.tool_name,
            tool_input=claim.tool_input,
            priority=claim.priority,
            timeout_seconds=claim.timeout_seconds,
        )
        await repository.insert_task(task)

        # Find an idle runner with matching capabilities
        idle_runners = await repository.list_runners(status=RunnerStatus.IDLE)

        assigned = False
        for runner in idle_runners:
            # Check runner is still alive (Redis heartbeat exists)
            if not await is_runner_alive(runner.runner_id):
                continue

            # Try to acquire a lease on the task
            if await acquire_task_lease(claim.task_id, runner.runner_id, claim.timeout_seconds):
                # Mark task as claimed
                await repository.update_task_claimed(claim.task_id, runner.runner_id)
                await repository.update_runner_heartbeat(
                    runner.runner_id, RunnerStatus.CLAIMED, claim.task_id
                )

                # Ensure workspace exists
                ensure_workspace(claim.workspace_id)

                # Notify runner
                await self._publisher.publish_task_assigned(
                    claim.task_id,
                    runner.runner_id,
                    {
                        "run_id": claim.run_id,
                        "session_id": claim.session_id,
                        "workspace_id": claim.workspace_id,
                        "tool_name": claim.tool_name,
                        "tool_input": claim.tool_input,
                        "timeout_seconds": claim.timeout_seconds,
                    },
                )
                assigned = True
                logger.info(
                    "task_assigned",
                    extra={"task_id": claim.task_id, "runner_id": runner.runner_id},
                )
                break

        if not assigned:
            logger.warning("no_idle_runner_available", extra={"task_id": claim.task_id})

    # ------------------------------------------------------------------
    # Task completion
    # ------------------------------------------------------------------

    async def handle_complete(self, task_id: str, data: dict[str, Any]) -> None:
        """Process a velion.runner.complete.{task_id} message."""
        result = TaskResult.model_validate({"task_id": task_id, **data})

        # Release the lease
        await release_task_lease(task_id, result.runner_id)

        # Update task in DB
        await repository.update_task_completed(
            task_id=task_id,
            success=result.success,
            output=result.output,
            error=result.error,
            duration_ms=result.duration_ms,
        )

        # Mark runner as idle again
        await repository.update_runner_heartbeat(
            result.runner_id, RunnerStatus.IDLE, None
        )

        # Publish result event for agent-core to consume
        await self._publisher.publish_task_result(task_id, result.model_dump(mode="json"))

        logger.info(
            "task_completed",
            extra={
                "task_id": task_id,
                "runner_id": result.runner_id,
                "success": result.success,
                "duration_ms": result.duration_ms,
            },
        )

    # ------------------------------------------------------------------
    # Task cancellation
    # ------------------------------------------------------------------

    async def handle_cancel(self, task_id: str, data: dict[str, Any]) -> None:
        """Process a velion.runner.cancel.{task_id} message."""
        cancel = TaskCancel.model_validate({"task_id": task_id, **data})

        task = await repository.get_task(task_id)
        if task and task.runner_id:
            await release_task_lease(task_id, task.runner_id)
            await repository.update_runner_heartbeat(
                task.runner_id, RunnerStatus.IDLE, None
            )

        await repository.update_task_cancelled(task_id, cancel.reason)
        logger.info("task_cancelled", extra={"task_id": task_id, "reason": cancel.reason})

    # ------------------------------------------------------------------
    # Dead runner detection
    # ------------------------------------------------------------------

    async def sweep_dead_runners(self, stale_seconds: int = 600) -> int:
        """Mark runners as dead if their heartbeat has expired.

        Called periodically (e.g. every 60s) by the background task.
        """
        count = await repository.mark_runners_dead(stale_seconds)
        if count > 0:
            # Publish events for each dead runner
            dead = await repository.list_runners(status=RunnerStatus.DEAD)
            for runner in dead:
                await self._publisher.publish_runner_dead(runner.runner_id)
        return count

    # ------------------------------------------------------------------
    # Artifacts
    # ------------------------------------------------------------------

    async def create_upload_url(
        self,
        task_id: str,
        workspace_id: str,
        filename: str,
        kind: ArtifactKind = ArtifactKind.OUTPUT,
        content_type: str = "application/octet-stream",
    ) -> dict[str, str]:
        """Generate a presigned upload URL and record artifact metadata."""
        url, storage_key = generate_upload_url(workspace_id, task_id, filename, content_type)
        meta = ArtifactMetadata(
            task_id=task_id,
            workspace_id=workspace_id,
            kind=kind,
            filename=filename,
            content_type=content_type,
            storage_key=storage_key,
        )
        await repository.insert_artifact(meta)
        return {"upload_url": url, "artifact_id": meta.artifact_id, "storage_key": storage_key}

    async def create_download_url(self, storage_key: str) -> str:
        """Generate a presigned download URL for an artifact."""
        return generate_download_url(storage_key)

    async def list_task_artifacts(self, task_id: str) -> list[ArtifactMetadata]:
        return await repository.list_artifacts(task_id)
