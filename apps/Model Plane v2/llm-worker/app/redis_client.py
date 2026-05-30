"""LLM Worker redis client — re-exports from reasoning_runtime.

BUG FIX: original had init_redis()/close_redis() but main.py called
init()/close(). Shared version uses init()/close() correctly.
"""

from reasoning_runtime.redis_rate_limit import (  # noqa: F401
    check_rate_limit,
    close,
    init,
)
