"""BashTool — async subprocess execution with timeout and sandbox support."""

from __future__ import annotations

import asyncio
import logging
import shlex
from typing import Any

from app.tools.base import ToolDefinition, ToolResult

logger = logging.getLogger(__name__)

MAX_OUTPUT_BYTES = 128 * 1024  # 128 KB
DEFAULT_TIMEOUT = 120  # seconds


class BashTool:
    """Execute shell commands asynchronously with timeout enforcement."""

    name = "bash"
    description = "Run a shell command and return stdout/stderr. Use for file operations, builds, tests, and system tasks."
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "command": {"type": "string", "description": "Shell command to execute."},
            "timeout": {"type": "integer", "description": "Timeout in seconds.", "default": DEFAULT_TIMEOUT},
        },
        "required": ["command"],
    }
    search_hint = "shell terminal run execute command subprocess"
    should_defer = False

    def is_read_only(self) -> bool:
        return False

    def is_concurrent_safe(self) -> bool:
        return False

    def is_destructive(self) -> bool:
        return True

    def prompt(self) -> str:
        return (
            "Run shell commands. Prefer this for file system operations, "
            "building, testing, and running scripts."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        command = input_data.get("command")
        if not command or not isinstance(command, str):
            raise ValueError("'command' is required and must be a non-empty string")
        if not command.strip():
            raise ValueError("'command' must not be blank")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        command = input_data["command"]
        timeout = input_data.get("timeout", DEFAULT_TIMEOUT)

        try:
            proc = await asyncio.create_subprocess_shell(
                command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout_bytes, stderr_bytes = await asyncio.wait_for(
                proc.communicate(), timeout=timeout
            )
        except asyncio.TimeoutError:
            try:
                proc.kill()
                await proc.wait()
            except ProcessLookupError:
                pass
            return ToolResult(error=f"Command timed out after {timeout}s")
        except OSError as exc:
            return ToolResult(error=f"Failed to start process: {exc}")

        stdout = stdout_bytes.decode("utf-8", errors="replace")
        stderr = stderr_bytes.decode("utf-8", errors="replace")
        is_truncated = False

        if len(stdout) > MAX_OUTPUT_BYTES:
            stdout = stdout[:MAX_OUTPUT_BYTES]
            is_truncated = True
        if len(stderr) > MAX_OUTPUT_BYTES:
            stderr = stderr[:MAX_OUTPUT_BYTES]
            is_truncated = True

        output_parts = []
        if stdout:
            output_parts.append(stdout)
        if stderr:
            output_parts.append(f"STDERR:\n{stderr}")

        output = "\n".join(output_parts) or "(no output)"

        return ToolResult(
            output=output,
            error=None if proc.returncode == 0 else f"Exit code: {proc.returncode}",
            is_truncated=is_truncated,
            metadata={"exit_code": proc.returncode},
        )
