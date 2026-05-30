"""PII field stripping — remove sensitive data before non-privileged sinks.

CC pattern: fields prefixed ``_PII_`` or in a denylist are stripped
from event props before sending to external/non-privileged sinks.
"""

from __future__ import annotations

import re
from typing import Any

# Fields that always contain PII and should be stripped for external sinks
_PII_DENYLIST = frozenset({
    "email",
    "ip_address",
    "user_agent",
    "full_name",
    "phone",
    "password",
    "token",
    "api_key",
    "secret",
})

_PII_PREFIX = "_PII_"


def strip_pii(props: dict[str, Any]) -> dict[str, Any]:
    """Return a copy of *props* with PII fields removed.

    A field is considered PII if:
    - Its key starts with ``_PII_``
    - Its key is in the built-in denylist
    """
    return {
        k: v
        for k, v in props.items()
        if not k.startswith(_PII_PREFIX) and k.lower() not in _PII_DENYLIST
    }


def has_pii(props: dict[str, Any]) -> bool:
    """Return True if any PII fields are present."""
    for k in props:
        if k.startswith(_PII_PREFIX) or k.lower() in _PII_DENYLIST:
            return True
    return False
