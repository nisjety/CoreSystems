"""Permission system — fine-grained tool permission modes.

Mirrors CC's hooks/toolPermission/ system with 4 modes:
- TRUST: all tools auto-approved
- INTERACTIVE: ask user for dangerous tools
- COORDINATOR: delegated permissions from parent run
- SWARM: coordinator approves on behalf of workers

Also tracks denied tools per-session to avoid re-asking the user
about tools they've already rejected (CC's denialTracking.ts).
"""

from __future__ import annotations
