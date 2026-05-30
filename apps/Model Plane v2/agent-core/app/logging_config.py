"""Structured JSON logging for agent-core v2.

Replaces the default text-based logging with JSON-formatted output.
Each log line is a valid JSON object with:
  - timestamp, level, logger, message
  - service_name, environment
  - Extra fields from LogRecord.extra
  - Correlation IDs (run_id, session_id, org_id) when available

Compatible with Azure Monitor, Datadog, ELK, and any JSON log aggregator.
"""

from __future__ import annotations

import json
import logging
import sys
import traceback
from datetime import datetime, timezone
from typing import Any

from app.config import settings

# Fields to exclude from extra data
_EXCLUDE_FIELDS = frozenset({
    "name", "msg", "args", "created", "filename", "funcName",
    "levelname", "levelno", "lineno", "module", "msecs",
    "pathname", "process", "processName", "relativeCreated",
    "stack_info", "thread", "threadName", "exc_info", "exc_text",
    "message", "taskName",
})


class JSONFormatter(logging.Formatter):
    """Format log records as single-line JSON objects."""

    def format(self, record: logging.LogRecord) -> str:
        log_entry: dict[str, Any] = {
            "timestamp": datetime.fromtimestamp(record.created, tz=timezone.utc).isoformat(),
            "level": record.levelname,
            "logger": record.name,
            "message": record.getMessage(),
            "service": settings.service_name,
            "environment": settings.environment,
        }

        # Add location info for errors
        if record.levelno >= logging.WARNING:
            log_entry["location"] = {
                "file": record.pathname,
                "line": record.lineno,
                "function": record.funcName,
            }

        # Add exception info
        if record.exc_info and record.exc_info[1]:
            log_entry["exception"] = {
                "type": type(record.exc_info[1]).__name__,
                "message": str(record.exc_info[1]),
                "traceback": traceback.format_exception(*record.exc_info),
            }

        # Add extra fields (correlation IDs, custom data)
        extra: dict[str, Any] = {}
        for key, value in record.__dict__.items():
            if key not in _EXCLUDE_FIELDS and not key.startswith("_"):
                try:
                    json.dumps(value)
                    extra[key] = value
                except (TypeError, ValueError):
                    extra[key] = str(value)

        if extra:
            log_entry["extra"] = extra

        return json.dumps(log_entry, default=str)


def configure_logging() -> None:
    """Configure structured JSON logging for the application.

    In development mode, uses a more readable format.
    In production, uses pure JSON for machine parsing.
    """
    root = logging.getLogger()
    root.handlers.clear()

    handler = logging.StreamHandler(sys.stdout)

    if settings.environment in ("development", "local", "test") and settings.debug:
        # Human-readable format in dev with debug
        formatter = logging.Formatter(
            "%(asctime)s %(levelname)-8s %(name)-25s %(message)s",
            datefmt="%H:%M:%S",
        )
    else:
        # JSON in all other environments (including dev without debug)
        formatter = JSONFormatter()

    handler.setFormatter(formatter)
    root.addHandler(handler)

    # Set level
    level = logging.DEBUG if settings.debug else logging.INFO
    root.setLevel(level)

    # Quiet noisy libraries
    logging.getLogger("httpx").setLevel(logging.WARNING)
    logging.getLogger("httpcore").setLevel(logging.WARNING)
    logging.getLogger("uvicorn.access").setLevel(logging.WARNING)
    logging.getLogger("nats").setLevel(logging.WARNING)
    logging.getLogger("asyncio").setLevel(logging.WARNING)
