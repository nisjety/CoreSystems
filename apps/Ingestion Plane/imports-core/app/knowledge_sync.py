"""GitHub / Slack knowledge content-sync worker.

Mirrors leads-core's provider-lead Syncer: discover connections via the actions
gateway, gate each on the content-read capability, pull real content through the
gateway (never touching provider tokens), and produce ImportDocuments that the
existing import pipeline forwards to Data Plane v2. Per-connection failures are
recorded as skips; a whole-provider gateway-list failure emits an audit event
(leads-core previously produced NO audit at all on that path — that omission is
fixed and explicitly tested here).

Operations + capabilities are the frozen contract in
docs/actions-surface-operations.md:
  - github.repos (repo.public.read) -> github.readme.get (repo.contents.read, sensitive)
  - slack.channels.list (channels.read) -> slack.messages.list (channels.history, sensitive)

Sensitive operations require the explicit capability even when a connection's
capabilities list is empty, so we gate strictly on the sensitive capability.
"""

import base64
import logging
from dataclasses import dataclass, field
from typing import Any, Protocol

from app.schemas import ImportDocument

log = logging.getLogger(__name__)

PROVIDER_GITHUB = "github"
PROVIDER_SLACK = "slack"

CAP_GITHUB_CONTENTS = "repo.contents.read"
CAP_SLACK_HISTORY = "channels.history"


class Gateway(Protocol):
    async def list_connections(self, org_id: str, provider_key: str) -> list[dict[str, Any]]: ...
    async def execute_action(
        self,
        connection_id: str,
        operation: str,
        params: dict[str, Any] | None = None,
        body: dict[str, Any] | None = None,
        org_id: str | None = None,
    ) -> Any: ...


class AuditSink(Protocol):
    async def publish_sync(self, audit: dict[str, Any]) -> None: ...


@dataclass
class SyncResult:
    documents: list[ImportDocument] = field(default_factory=list)
    connections: int = 0
    skipped: list[str] = field(default_factory=list)
    outcome: str = "ok"  # "ok" | "skipped" | "failed"


def has_capability(capabilities: Any, want: str) -> bool:
    """Case-insensitive capability check. An empty/absent capabilities list does
    NOT satisfy a sensitive capability (matches integration-corev2's rule that
    sensitive operations always require the explicit capability string)."""
    if not isinstance(capabilities, list):
        return False
    want_lower = want.strip().lower()
    return any(isinstance(c, str) and c.strip().lower() == want_lower for c in capabilities)


def decode_github_content(payload: Any) -> str:
    """Decode a GitHub contents/readme payload's base64 body to text."""
    if not isinstance(payload, dict):
        return ""
    content = payload.get("content")
    if not isinstance(content, str) or not content:
        return ""
    if payload.get("encoding", "base64") == "base64":
        try:
            raw = base64.b64decode(content)
        except (ValueError, TypeError):
            return ""
        return raw.decode("utf-8", "ignore")
    return content


def render_slack_messages(messages: Any) -> str:
    """Flatten Slack conversations.history messages into plain text (one per line)."""
    if not isinstance(messages, list):
        return ""
    lines: list[str] = []
    for msg in messages:
        if not isinstance(msg, dict):
            continue
        text = msg.get("text")
        if isinstance(text, str) and text.strip():
            lines.append(text.strip())
    return "\n".join(lines)


def _repo_identity(repo: Any) -> tuple[str, str, str]:
    """Return (owner, name, full_name) from a GitHub repo object, tolerantly."""
    if not isinstance(repo, dict):
        return "", "", ""
    name = repo.get("name") or ""
    owner = ""
    owner_obj = repo.get("owner")
    if isinstance(owner_obj, dict):
        owner = owner_obj.get("login") or ""
    full = repo.get("full_name") or (f"{owner}/{name}" if owner and name else "")
    if not owner and "/" in full:
        owner = full.split("/", 1)[0]
    return owner, name, full


class KnowledgeSyncer:
    def __init__(
        self,
        gateway: Gateway,
        audit: AuditSink | None = None,
        max_repos: int = 25,
        max_channels: int = 25,
        max_messages: int = 100,
    ) -> None:
        self._gateway = gateway
        self._audit = audit
        self._max_repos = max_repos
        self._max_channels = max_channels
        self._max_messages = max_messages

    async def sync(self, org_id: str) -> SyncResult:
        result = SyncResult()
        any_provider_failed = False

        for provider, capability, fetch in (
            (PROVIDER_GITHUB, CAP_GITHUB_CONTENTS, self._sync_github),
            (PROVIDER_SLACK, CAP_SLACK_HISTORY, self._sync_slack),
        ):
            try:
                connections = await self._gateway.list_connections(org_id, provider)
            except Exception as exc:  # noqa: BLE001 — provider-list failure must be audited, not swallowed
                # Whole-provider gateway failure. Previously (leads-core) this left
                # NO audit trail at all — emit one so the failure is visible.
                any_provider_failed = True
                reason = f"list {provider} connections: {exc}"
                result.skipped.append(reason)
                await self._emit_audit(org_id, provider, "failed", skipped=[reason])
                continue

            provider_connections = 0
            provider_documents = 0
            for conn in connections:
                conn_id = conn.get("id")
                if not conn_id:
                    continue
                if not has_capability(conn.get("capabilities"), capability):
                    result.skipped.append(
                        f"{provider} connection {conn_id}: missing {capability} capability"
                    )
                    continue
                try:
                    docs = await fetch(org_id, conn)
                except Exception as exc:  # noqa: BLE001 — per-connection failure is a skip, not a run abort
                    result.skipped.append(f"{provider} connection {conn_id}: {exc}")
                    continue
                provider_connections += 1
                provider_documents += len(docs)
                result.connections += 1
                result.documents.extend(docs)

            outcome = "ok" if provider_connections > 0 else "skipped"
            await self._emit_audit(
                org_id,
                provider,
                outcome,
                connections=provider_connections,
                documents=provider_documents,
            )

        if any_provider_failed:
            result.outcome = "failed"
        elif result.connections == 0:
            result.outcome = "skipped"
        else:
            result.outcome = "ok"
        return result

    async def _sync_github(self, org_id: str, conn: dict[str, Any]) -> list[ImportDocument]:
        conn_id = conn["id"]
        repos = await self._gateway.execute_action(
            conn_id, "github.repos", params={"perPage": self._max_repos}, org_id=org_id
        )
        if not isinstance(repos, list):
            return []
        documents: list[ImportDocument] = []
        for repo in repos[: self._max_repos]:
            owner, name, full = _repo_identity(repo)
            if not owner or not name:
                continue
            try:
                readme = await self._gateway.execute_action(
                    conn_id,
                    "github.readme.get",
                    params={"owner": owner, "repo": name},
                    org_id=org_id,
                )
            except Exception:  # noqa: BLE001 — a repo without a README is a normal skip
                continue
            text = decode_github_content(readme)
            if not text.strip():
                continue
            html_url = readme.get("html_url") if isinstance(readme, dict) else None
            documents.append(
                ImportDocument(
                    source_id=f"github:{full}:readme",
                    source_name=full,
                    title=f"{full} README",
                    text=text,
                    metadata={
                        "source": "github",
                        "connection_id": conn_id,
                        "repo": full,
                        "html_url": html_url,
                    },
                )
            )
        return documents

    async def _sync_slack(self, org_id: str, conn: dict[str, Any]) -> list[ImportDocument]:
        conn_id = conn["id"]
        channels_result = await self._gateway.execute_action(
            conn_id, "slack.channels.list", org_id=org_id
        )
        channels = _extract_list(channels_result, "channels")
        documents: list[ImportDocument] = []
        for channel in channels[: self._max_channels]:
            if not isinstance(channel, dict):
                continue
            channel_id = channel.get("id")
            channel_name = channel.get("name") or channel_id
            if not channel_id:
                continue
            try:
                history = await self._gateway.execute_action(
                    conn_id,
                    "slack.messages.list",
                    params={"channel": channel_id, "limit": self._max_messages},
                    org_id=org_id,
                )
            except Exception:  # noqa: BLE001 — a channel we can't read is a normal skip
                continue
            text = render_slack_messages(_extract_list(history, "messages"))
            if not text.strip():
                continue
            documents.append(
                ImportDocument(
                    source_id=f"slack:{channel_id}:history",
                    source_name=channel_name,
                    title=f"#{channel_name}",
                    text=text,
                    metadata={
                        "source": "slack",
                        "connection_id": conn_id,
                        "channel_id": channel_id,
                        "channel": channel_name,
                    },
                )
            )
        return documents

    async def _emit_audit(
        self,
        org_id: str,
        provider: str,
        outcome: str,
        connections: int = 0,
        documents: int = 0,
        skipped: list[str] | None = None,
    ) -> None:
        if self._audit is None:
            return
        try:
            await self._audit.publish_sync(
                {
                    "org_id": org_id,
                    "provider": provider,
                    "outcome": outcome,
                    "connections": connections,
                    "documents": documents,
                    "skipped": skipped or [],
                }
            )
        except Exception:  # noqa: BLE001 — audit is best-effort, never breaks the sync
            log.warning("knowledge-sync audit publish failed", exc_info=True)


def _extract_list(result: Any, key: str) -> list[Any]:
    """Slack results are objects ({channels:[...]}/{messages:[...]}); tolerate a
    bare list too."""
    if isinstance(result, dict):
        value = result.get(key)
        return value if isinstance(value, list) else []
    if isinstance(result, list):
        return result
    return []
