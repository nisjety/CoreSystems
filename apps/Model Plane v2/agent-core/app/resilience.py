"""Resilience primitives — circuit breaker, retry decorator, timeout.

Ported from CC patterns (autoCompact circuit breaker, retry logic)
into reusable Python decorators for LLM calls, MCP calls, and
tool execution.
"""

from __future__ import annotations

import asyncio
import functools
import logging
import random
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Callable, TypeVar

logger = logging.getLogger(__name__)

F = TypeVar("F", bound=Callable[..., Any])


# ---------------------------------------------------------------------------
# Circuit Breaker (mirrors CC autoCompact consecutive-failure pattern)
# ---------------------------------------------------------------------------


class CircuitState(str, Enum):
    CLOSED = "closed"       # normal operation
    OPEN = "open"           # failing, reject calls
    HALF_OPEN = "half_open" # testing if service recovered


@dataclass
class CircuitBreaker:
    """Async-safe circuit breaker with configurable thresholds.

    Ported from CC's MAX_CONSECUTIVE_AUTOCOMPACT_FAILURES = 3 pattern
    in autoCompact.ts.

    Usage:
        breaker = CircuitBreaker(name="llm", failure_threshold=3)

        async def call_llm(...):
            if breaker.should_reject():
                raise CircuitOpenError(breaker.name)
            try:
                result = await actual_llm_call(...)
                breaker.record_success()
                return result
            except Exception as exc:
                breaker.record_failure()
                raise
    """

    name: str = "default"
    failure_threshold: int = 3       # consecutive failures before opening
    recovery_timeout: float = 60.0   # seconds in OPEN before trying HALF_OPEN
    half_open_max: int = 1           # max concurrent calls in HALF_OPEN

    # Internal state
    _state: CircuitState = field(default=CircuitState.CLOSED, init=False)
    _consecutive_failures: int = field(default=0, init=False)
    _last_failure_time: float = field(default=0.0, init=False)
    _half_open_calls: int = field(default=0, init=False)

    @property
    def state(self) -> CircuitState:
        if self._state == CircuitState.OPEN:
            if time.monotonic() - self._last_failure_time >= self.recovery_timeout:
                self._state = CircuitState.HALF_OPEN
                self._half_open_calls = 0
        return self._state

    def should_reject(self) -> bool:
        """Return True if the call should be rejected."""
        st = self.state
        if st == CircuitState.CLOSED:
            return False
        if st == CircuitState.OPEN:
            return True
        # HALF_OPEN: allow limited probes
        return self._half_open_calls >= self.half_open_max

    def record_success(self) -> None:
        """Record a successful call."""
        if self._state == CircuitState.HALF_OPEN:
            logger.info("circuit_recovered", extra={"breaker": self.name})
        self._state = CircuitState.CLOSED
        self._consecutive_failures = 0
        self._half_open_calls = 0

    def record_failure(self) -> None:
        """Record a failed call."""
        self._consecutive_failures += 1
        self._last_failure_time = time.monotonic()

        if self._state == CircuitState.HALF_OPEN:
            self._state = CircuitState.OPEN
            logger.warning(
                "circuit_reopened",
                extra={"breaker": self.name, "failures": self._consecutive_failures},
            )
        elif self._consecutive_failures >= self.failure_threshold:
            self._state = CircuitState.OPEN
            logger.warning(
                "circuit_opened",
                extra={"breaker": self.name, "failures": self._consecutive_failures},
            )

        if self._state == CircuitState.HALF_OPEN:
            self._half_open_calls += 1

    def reset(self) -> None:
        """Manually reset the circuit to CLOSED."""
        self._state = CircuitState.CLOSED
        self._consecutive_failures = 0
        self._half_open_calls = 0


class CircuitOpenError(Exception):
    """Raised when a call is rejected by an open circuit breaker."""

    def __init__(self, breaker_name: str) -> None:
        super().__init__(f"Circuit breaker '{breaker_name}' is open")
        self.breaker_name = breaker_name


# ---------------------------------------------------------------------------
# Retry decorator with exponential backoff + jitter
# ---------------------------------------------------------------------------

# Default retryable exceptions
RETRYABLE_EXCEPTIONS: tuple[type[Exception], ...] = (
    ConnectionError,
    TimeoutError,
    asyncio.TimeoutError,
    OSError,
)


def retry(
    *,
    max_retries: int = 3,
    base_delay: float = 1.0,
    max_delay: float = 30.0,
    backoff_factor: float = 2.0,
    jitter: bool = True,
    retryable: tuple[type[Exception], ...] = RETRYABLE_EXCEPTIONS,
    breaker: CircuitBreaker | None = None,
    on_retry: Callable[[int, Exception], None] | None = None,
) -> Callable[[F], F]:
    """Decorator for asyncio functions with exponential backoff retry.

    Mirrors CC's retry patterns with added circuit-breaker integration.

    Usage:
        @retry(max_retries=3, breaker=llm_breaker)
        async def call_llm(prompt: str) -> str:
            ...
    """

    def decorator(func: F) -> F:
        @functools.wraps(func)
        async def wrapper(*args: Any, **kwargs: Any) -> Any:
            if breaker and breaker.should_reject():
                raise CircuitOpenError(breaker.name)

            last_exc: Exception | None = None
            for attempt in range(max_retries + 1):
                try:
                    result = await func(*args, **kwargs)
                    if breaker:
                        breaker.record_success()
                    return result
                except retryable as exc:
                    last_exc = exc
                    if breaker:
                        breaker.record_failure()

                    if attempt >= max_retries:
                        break

                    delay = min(base_delay * (backoff_factor ** attempt), max_delay)
                    if jitter:
                        delay *= 0.5 + random.random()  # noqa: S311

                    if on_retry:
                        on_retry(attempt + 1, exc)
                    else:
                        logger.warning(
                            "retry_attempt",
                            extra={
                                "func": func.__name__,
                                "attempt": attempt + 1,
                                "delay": f"{delay:.1f}s",
                                "error": str(exc),
                            },
                        )
                    await asyncio.sleep(delay)

                except Exception as exc:
                    # Non-retryable exception → fail immediately
                    if breaker:
                        breaker.record_failure()
                    raise

            raise last_exc  # type: ignore[misc]

        return wrapper  # type: ignore[return-value]

    return decorator


# ---------------------------------------------------------------------------
# Timeout wrapper
# ---------------------------------------------------------------------------


def with_timeout(seconds: float) -> Callable[[F], F]:
    """Decorator that wraps async calls with asyncio.wait_for timeout."""

    def decorator(func: F) -> F:
        @functools.wraps(func)
        async def wrapper(*args: Any, **kwargs: Any) -> Any:
            return await asyncio.wait_for(func(*args, **kwargs), timeout=seconds)

        return wrapper  # type: ignore[return-value]

    return decorator
