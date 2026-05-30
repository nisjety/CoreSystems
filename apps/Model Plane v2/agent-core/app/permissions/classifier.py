"""Classifier approvals — auto-approve safe tool calls via rule matching.

Ported from CC's utils/classifierApprovals.ts.

Two classifier types:
- bash: regex rules for safe bash commands (read-only, info gathering)
- auto_mode: transcript-based analysis for safe mutations

The classifier stores per-tool-use approval decisions so the UI
(or webhook) can display *why* a tool was auto-approved.
"""

from __future__ import annotations

import logging
import re
from dataclasses import dataclass, field
from enum import Enum
from typing import Any

from app.messages.types import MessageType, RunEvent, build_event

logger = logging.getLogger(__name__)


class ClassifierType(str, Enum):
    BASH = "bash"
    AUTO_MODE = "auto_mode"


@dataclass(frozen=True)
class ClassifierApproval:
    """Record of a classifier's decision to auto-approve a tool use."""

    classifier: ClassifierType
    matched_rule: str = ""
    reason: str = ""


# Global approval store: tool_use_id → ClassifierApproval
_APPROVALS: dict[str, ClassifierApproval] = {}
# Tool use IDs currently being checked by a classifier
_CHECKING: set[str] = set()


# ---------------------------------------------------------------------------
# Bash classifier rules (ported from CC destructiveCommandWarning.ts)
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class _BashRule:
    pattern: re.Pattern[str]
    label: str
    safe: bool  # True = auto-approve; False = block / warn


# Commands that are always safe (read-only / informational)
_SAFE_BASH_RULES: list[_BashRule] = [
    _BashRule(re.compile(r"^\s*(ls|cat|head|tail|wc|grep|find|which|type|echo|pwd|date|whoami)\b"), "read_only_cmd", True),
    _BashRule(re.compile(r"^\s*git\s+(status|log|diff|show|branch|tag)\b"), "git_read", True),
    _BashRule(re.compile(r"^\s*git\s+--no-pager\b"), "git_no_pager", True),
    _BashRule(re.compile(r"^\s*(python3?|node|ruby|go)\s+--version\b"), "version_check", True),
    _BashRule(re.compile(r"^\s*curl\s+"), "curl", True),
    _BashRule(re.compile(r"^\s*docker\s+(ps|images|inspect|logs)\b"), "docker_read", True),
    _BashRule(re.compile(r"^\s*kubectl\s+(get|describe|logs)\b"), "kubectl_read", True),
]

# Commands that are destructive (mirrors CC destructiveCommandWarning patterns)
_DESTRUCTIVE_PATTERNS: list[_BashRule] = [
    _BashRule(re.compile(r"\bgit\s+reset\s+--hard\b"), "git_reset_hard", False),
    _BashRule(re.compile(r"\bgit\s+push\b[^;&|\n]*\s+(--force|--force-with-lease|-f)\b"), "git_force_push", False),
    _BashRule(re.compile(r"\bgit\s+clean\b(?![^;&|\n]*(?:-[a-zA-Z]*n|--dry-run))[^;&|\n]*-[a-zA-Z]*f"), "git_clean_force", False),
    _BashRule(re.compile(r"\bgit\s+stash\s+(drop|clear)\b"), "git_stash_drop", False),
    _BashRule(re.compile(r"\bgit\s+(commit|push|merge)\b[^;&|\n]*--no-verify\b"), "git_no_verify", False),
    _BashRule(re.compile(r"\bgit\s+commit\b[^;&|\n]*--amend\b"), "git_amend", False),
    _BashRule(re.compile(r"(^|[;&|\n]\s*)rm\s+-[a-zA-Z]*[rR][a-zA-Z]*f"), "rm_rf", False),
    _BashRule(re.compile(r"(^|[;&|\n]\s*)rm\s+-[a-zA-Z]*f"), "rm_force", False),
    _BashRule(re.compile(r"\b(DROP|TRUNCATE)\s+(TABLE|DATABASE|SCHEMA)\b", re.IGNORECASE), "drop_db", False),
    _BashRule(re.compile(r"\bDELETE\s+FROM\s+\w+\s*(;|\"|'|\n|$)", re.IGNORECASE), "delete_all_rows", False),
    _BashRule(re.compile(r"\bdocker\s+system\s+prune\b"), "docker_prune", False),
    _BashRule(re.compile(r"\bkubectl\s+delete\b"), "kubectl_delete", False),
    _BashRule(re.compile(r"\bterraform\s+destroy\b"), "terraform_destroy", False),
]


# ---------------------------------------------------------------------------
# Destructive command warning (informational, ported from CC)
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class DestructiveWarning:
    """A warning about a potentially destructive command."""

    command: str
    rule_label: str
    warning: str


_DESTRUCTION_WARNINGS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"\bgit\s+reset\s+--hard\b"), "may discard uncommitted changes"),
    (re.compile(r"\bgit\s+push\b[^;&|\n]*\s+(--force|--force-with-lease|-f)\b"), "may overwrite remote history"),
    (re.compile(r"\bgit\s+clean\b[^;&|\n]*-[a-zA-Z]*f"), "may permanently delete untracked files"),
    (re.compile(r"\bgit\s+stash\s+(drop|clear)\b"), "may permanently remove stashed changes"),
    (re.compile(r"\bgit\s+(commit|push|merge)\b[^;&|\n]*--no-verify\b"), "may skip safety hooks"),
    (re.compile(r"\bgit\s+commit\b[^;&|\n]*--amend\b"), "may rewrite the last commit"),
    (re.compile(r"(^|[;&|\n]\s*)rm\s+-[a-zA-Z]*[rR][a-zA-Z]*f"), "may recursively force-remove files"),
    (re.compile(r"\b(DROP|TRUNCATE)\s+(TABLE|DATABASE|SCHEMA)\b", re.IGNORECASE), "may drop/truncate database objects"),
    (re.compile(r"\bDELETE\s+FROM\s+\w+\s*(;|\"|'|\n|$)", re.IGNORECASE), "may delete all rows from a table"),
    (re.compile(r"\bdocker\s+system\s+prune\b"), "may remove all stopped containers and images"),
    (re.compile(r"\bkubectl\s+delete\b"), "may delete Kubernetes resources"),
    (re.compile(r"\bterraform\s+destroy\b"), "may destroy infrastructure"),
]


def detect_destructive_warnings(command: str) -> list[DestructiveWarning]:
    """Detect potentially destructive commands and return warnings.

    Purely informational — does not block execution.
    """
    warnings: list[DestructiveWarning] = []
    for pattern, msg in _DESTRUCTION_WARNINGS:
        if pattern.search(command):
            warnings.append(
                DestructiveWarning(command=command, rule_label=pattern.pattern[:40], warning=f"Note: {msg}")
            )
    return warnings


# ---------------------------------------------------------------------------
# Classifier API
# ---------------------------------------------------------------------------


def classify_bash_command(tool_use_id: str, command: str) -> ClassifierApproval | None:
    """Classify a bash command as safe or destructive.

    Returns an approval if the command matches a safe pattern,
    or None if it doesn't match any known pattern.
    """
    # Check destructive patterns first (deny takes priority)
    for rule in _DESTRUCTIVE_PATTERNS:
        if rule.pattern.search(command):
            return None  # Not safe — needs human approval

    # Check safe patterns
    for rule in _SAFE_BASH_RULES:
        if rule.pattern.search(command):
            approval = ClassifierApproval(
                classifier=ClassifierType.BASH,
                matched_rule=rule.label,
            )
            set_classifier_approval(tool_use_id, approval)
            return approval

    return None  # Unknown — needs human review


def set_classifier_approval(tool_use_id: str, approval: ClassifierApproval) -> None:
    """Store a classifier approval for a tool use."""
    _APPROVALS[tool_use_id] = approval


def get_classifier_approval(tool_use_id: str) -> ClassifierApproval | None:
    """Get the classifier approval for a tool use (if any)."""
    return _APPROVALS.get(tool_use_id)


def set_auto_mode_approval(tool_use_id: str, reason: str) -> None:
    """Auto-mode classifier approved this tool use."""
    _APPROVALS[tool_use_id] = ClassifierApproval(
        classifier=ClassifierType.AUTO_MODE,
        reason=reason,
    )


def set_classifier_checking(tool_use_id: str) -> None:
    """Mark a tool use as currently being checked by a classifier."""
    _CHECKING.add(tool_use_id)


def clear_classifier_checking(tool_use_id: str) -> None:
    """Remove a tool use from the checking set."""
    _CHECKING.discard(tool_use_id)


def is_classifier_checking(tool_use_id: str) -> bool:
    """Check if a tool use is being evaluated by a classifier."""
    return tool_use_id in _CHECKING


def delete_classifier_approval(tool_use_id: str) -> None:
    """Remove a stored approval."""
    _APPROVALS.pop(tool_use_id, None)


def clear_all_approvals() -> None:
    """Reset all classifier state (e.g. on session clear)."""
    _APPROVALS.clear()
    _CHECKING.clear()


def build_classifier_event(
    run_id: str,
    session_id: str,
    tool_use_id: str,
    turn_index: int = 0,
) -> RunEvent | None:
    """Create a CLASSIFIER_APPROVAL RunEvent if the tool was auto-approved."""
    approval = get_classifier_approval(tool_use_id)
    if approval is None:
        return None

    return build_event(
        run_id=run_id,
        session_id=session_id,
        msg_type=MessageType.CLASSIFIER_APPROVAL,
        turn_index=turn_index,
        data={
            "tool_use_id": tool_use_id,
            "classifier": approval.classifier.value,
            "matched_rule": approval.matched_rule,
            "reason": approval.reason,
        },
    )


def build_destructive_warning_event(
    run_id: str,
    session_id: str,
    command: str,
    turn_index: int = 0,
) -> list[RunEvent]:
    """Create DESTRUCTIVE_WARNING RunEvents for a command."""
    warnings = detect_destructive_warnings(command)
    return [
        build_event(
            run_id=run_id,
            session_id=session_id,
            msg_type=MessageType.DESTRUCTIVE_WARNING,
            turn_index=turn_index,
            data={
                "command": w.command[:500],
                "rule": w.rule_label,
                "warning": w.warning,
            },
        )
        for w in warnings
    ]
