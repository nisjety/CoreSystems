"""Tests for the GitHub/Slack knowledge content-sync worker."""

import asyncio
import base64

import pytest

from app import knowledge_sync as ks


# ---------------------------------------------------------------------------
# pure helpers
# ---------------------------------------------------------------------------

def test_has_capability_case_insensitive_and_strict_on_empty():
    assert ks.has_capability(["Repo.Contents.Read"], "repo.contents.read") is True
    assert ks.has_capability(["channels.history"], "channels.history") is True
    # Empty/absent capabilities do NOT satisfy a (sensitive) capability.
    assert ks.has_capability([], "repo.contents.read") is False
    assert ks.has_capability(None, "channels.history") is False
    assert ks.has_capability(["other"], "repo.contents.read") is False


def test_decode_github_content_base64():
    payload = {"content": base64.b64encode(b"# Title\nbody").decode(), "encoding": "base64"}
    assert ks.decode_github_content(payload) == "# Title\nbody"
    assert ks.decode_github_content({"content": "plain", "encoding": "utf-8"}) == "plain"
    assert ks.decode_github_content({}) == ""
    assert ks.decode_github_content(None) == ""


def test_render_slack_messages():
    msgs = [{"text": "hello"}, {"no": "text"}, {"text": "  world  "}, {"text": ""}]
    assert ks.render_slack_messages(msgs) == "hello\nworld"
    assert ks.render_slack_messages(None) == ""


# ---------------------------------------------------------------------------
# fakes
# ---------------------------------------------------------------------------

class FakeGateway:
    def __init__(self, connections=None, actions=None, list_errors=None):
        # connections: {provider_key: [conn, ...]}
        self._connections = connections or {}
        # actions: {(connection_id, operation): result}
        self._actions = actions or {}
        # list_errors: {provider_key: Exception}
        self._list_errors = list_errors or {}

    async def list_connections(self, org_id, provider_key):
        if provider_key in self._list_errors:
            raise self._list_errors[provider_key]
        return list(self._connections.get(provider_key, []))

    async def execute_action(self, connection_id, operation, params=None, body=None, org_id=None):
        key = (connection_id, operation)
        if key not in self._actions:
            raise RuntimeError(f"no fake action for {key}")
        val = self._actions[key]
        if isinstance(val, Exception):
            raise val
        return val


class FakeAudit:
    def __init__(self):
        self.events = []

    async def publish_sync(self, audit):
        self.events.append(audit)


def _b64(s: str) -> str:
    return base64.b64encode(s.encode()).decode()


# ---------------------------------------------------------------------------
# github arm
# ---------------------------------------------------------------------------

def test_github_readme_becomes_document():
    conn = {"id": "gh-1", "capabilities": ["repo.contents.read"]}
    gw = FakeGateway(
        connections={"github": [conn], "slack": []},
        actions={
            ("gh-1", "github.repos"): [
                {"name": "core", "full_name": "acme/core", "owner": {"login": "acme"}},
            ],
            ("gh-1", "github.readme.get"): {
                "content": _b64("# Core\nThe core service."),
                "encoding": "base64",
                "html_url": "https://github.com/acme/core/blob/main/README.md",
            },
        },
    )
    audit = FakeAudit()
    result = asyncio.run(ks.KnowledgeSyncer(gw, audit=audit).sync("org-1"))

    assert result.outcome == "ok"
    assert len(result.documents) == 1
    doc = result.documents[0]
    assert "The core service." in doc.text
    assert doc.source_id == "github:acme/core:readme"
    assert doc.metadata["repo"] == "acme/core"
    # per-provider audit fired for github (ok) and slack (skipped, no connections)
    outcomes = {(e["provider"], e["outcome"]) for e in audit.events}
    assert ("github", "ok") in outcomes


def test_github_connection_without_capability_is_skipped():
    conn = {"id": "gh-2", "capabilities": []}  # no repo.contents.read
    gw = FakeGateway(connections={"github": [conn], "slack": []}, actions={})
    result = asyncio.run(ks.KnowledgeSyncer(gw).sync("org-1"))
    assert result.documents == []
    assert any("missing repo.contents.read" in s for s in result.skipped)
    assert result.outcome == "skipped"


# ---------------------------------------------------------------------------
# slack arm
# ---------------------------------------------------------------------------

def test_slack_channel_history_becomes_document():
    conn = {"id": "sl-1", "capabilities": ["channels.history"]}
    gw = FakeGateway(
        connections={"github": [], "slack": [conn]},
        actions={
            ("sl-1", "slack.channels.list"): {"channels": [{"id": "C1", "name": "general"}]},
            ("sl-1", "slack.messages.list"): {"messages": [{"text": "deploy done"}, {"text": "ok"}]},
        },
    )
    result = asyncio.run(ks.KnowledgeSyncer(gw).sync("org-1"))
    assert len(result.documents) == 1
    doc = result.documents[0]
    assert doc.text == "deploy done\nok"
    assert doc.source_id == "slack:C1:history"
    assert doc.title == "#general"


# ---------------------------------------------------------------------------
# failure paths — the leads-core audit bug must NOT recur
# ---------------------------------------------------------------------------

def test_whole_provider_list_failure_emits_audit_and_marks_failed():
    # A gateway list_connections failure previously produced NO audit event.
    gw = FakeGateway(
        connections={"slack": []},
        list_errors={"github": RuntimeError("gateway 503")},
    )
    audit = FakeAudit()
    result = asyncio.run(ks.KnowledgeSyncer(gw, audit=audit).sync("org-1"))

    assert result.outcome == "failed"
    assert any("list github connections" in s for s in result.skipped)
    # THE FIX: an audit event with outcome=failed for the failing provider.
    failed_events = [e for e in audit.events if e["outcome"] == "failed" and e["provider"] == "github"]
    assert len(failed_events) == 1
    assert failed_events[0]["skipped"]  # carries the reason


def test_per_connection_action_failure_is_a_skip_not_a_run_abort():
    conn = {"id": "gh-3", "capabilities": ["repo.contents.read"]}
    gw = FakeGateway(
        connections={"github": [conn], "slack": []},
        actions={("gh-3", "github.repos"): RuntimeError("token expired")},
    )
    result = asyncio.run(ks.KnowledgeSyncer(gw).sync("org-1"))
    # No crash; recorded as a skip; outcome skipped (no successful connections).
    assert result.documents == []
    assert any("gh-3" in s and "token expired" in s for s in result.skipped)
    assert result.outcome == "skipped"


def test_audit_failure_does_not_break_sync():
    class BoomAudit:
        async def publish_sync(self, audit):
            raise RuntimeError("nats down")

    conn = {"id": "sl-2", "capabilities": ["channels.history"]}
    gw = FakeGateway(
        connections={"github": [], "slack": [conn]},
        actions={
            ("sl-2", "slack.channels.list"): {"channels": [{"id": "C9", "name": "ops"}]},
            ("sl-2", "slack.messages.list"): {"messages": [{"text": "hi"}]},
        },
    )
    # Audit raising must not break the sync (best-effort).
    result = asyncio.run(ks.KnowledgeSyncer(gw, audit=BoomAudit()).sync("org-1"))
    assert len(result.documents) == 1
