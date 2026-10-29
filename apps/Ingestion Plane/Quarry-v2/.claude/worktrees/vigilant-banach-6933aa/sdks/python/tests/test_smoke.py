"""Smoke tests for the generated Quarry Python SDK.

These verify that the generated models can be instantiated and serialized,
catching OpenAPI spec drift before it reaches consumers.
"""

import json
import pytest

from quarry_client.models import (
    BatchRequest,
    CachePolicy,
    ChangeInfo,
    CrawlRequest,
    DriverInfo,
    DriverSignals,
    EnvelopeError,
    EnvelopeErrorError,
    EnvelopeHandoffAck,
    EnvelopeNormalizedOutput,
    FormatRef,
    HandoffAck,
    InternalRunPage,
    InternalRunPageResult,
    Link,
    NormalizedOutput,
    OutputFormats,
    PageMetadata,
    ScrapeRequest,
    UrlTriple,
)


class TestScrapeRequest:
    def test_minimal(self):
        req = ScrapeRequest(url="https://example.com")
        assert req.url == "https://example.com"
        assert req.zdr is None or req.zdr is False

    def test_full(self):
        req = ScrapeRequest(
            url="https://example.com/page",
            prev_fingerprint="blake3:abc123",
            cache=CachePolicy(mode="bypass", max_age_s=0),
            zdr=True,
            signals=DriverSignals(
                screenshot=True,
                pdf=False,
                actions=["click:#btn"],
                prior_block_signals=2,
            ),
            ingest=True,
            org_id="org_123",
        )
        assert req.url == "https://example.com/page"
        assert req.zdr is True
        assert req.signals.screenshot is True
        assert req.cache.mode == "bypass"
        assert req.ingest is True
        assert req.org_id == "org_123"

    def test_roundtrip_json(self):
        req = ScrapeRequest(url="https://example.com")
        data = json.loads(req.to_json())
        assert data["url"] == "https://example.com"
        restored = ScrapeRequest.from_json(json.dumps(data))
        assert restored.url == req.url


class TestBatchRequest:
    def test_multiple_urls(self):
        req = BatchRequest(urls=["https://a.com", "https://b.com", "https://c.com"])
        assert len(req.urls) == 3


class TestCrawlRequest:
    def test_with_max_pages(self):
        req = CrawlRequest(url="https://example.com", max_pages=50)
        assert req.max_pages == 50

    def test_without_max_pages(self):
        req = CrawlRequest(url="https://example.com")
        assert req.max_pages is None


class TestCachePolicy:
    def test_defaults(self):
        policy = CachePolicy()
        assert policy.mode is None or policy.mode == "read_write"

    @pytest.mark.parametrize("mode", ["read_write", "read_only", "write_only", "bypass"])
    def test_all_modes(self, mode):
        policy = CachePolicy(mode=mode)
        assert policy.mode == mode


class TestDriverSignals:
    def test_defaults(self):
        signals = DriverSignals()
        assert signals.screenshot is None or signals.screenshot is False

    def test_browser_escalation(self):
        signals = DriverSignals(prior_block_signals=3, profile_required=True)
        assert signals.prior_block_signals == 3
        assert signals.profile_required is True


class TestNormalizedOutput:
    def test_full_output(self):
        output = NormalizedOutput(
            run_id="run_01ABC",
            url=UrlTriple(
                requested="https://example.com",
                final_url="https://example.com/",
                canonical="https://example.com/",
            ),
            status=200,
            fetched_at="2026-05-07T12:00:00Z",
            fingerprint="blake3:deadbeef",
            formats=OutputFormats(
                html=FormatRef(artifact_id="art_html", bytes=5000),
                markdown=FormatRef(artifact_id="art_md", bytes=3000),
                links=[
                    Link(href="https://example.com/page1", text="Page 1"),
                    Link(href="https://example.com/page2"),
                ],
            ),
            change=ChangeInfo(status="new"),
            metadata=PageMetadata(title="Example", lang="en", content_type="text/html"),
            driver=DriverInfo(kind="static", duration_ms=150),
        )
        assert output.run_id == "run_01ABC"
        assert output.status == 200
        assert output.url.final_url == "https://example.com/"
        assert output.change.status == "new"
        assert output.driver.kind == "static"


class TestChangeInfo:
    @pytest.mark.parametrize("status", ["new", "changed", "unchanged"])
    def test_all_statuses(self, status):
        info = ChangeInfo(status=status)
        assert info.status == status


class TestDriverInfo:
    @pytest.mark.parametrize("kind", ["static", "browser", "tls"])
    def test_all_kinds(self, kind):
        info = DriverInfo(kind=kind, duration_ms=100)
        assert info.kind == kind

    def test_browser_metadata(self):
        info = DriverInfo(
            kind="browser",
            duration_ms=2500,
            profile="default",
            session_id="sess_abc",
            live_view_url="https://live.example.com/sess_abc",
            recording_id="rec_123",
        )
        assert info.session_id == "sess_abc"
        assert info.live_view_url == "https://live.example.com/sess_abc"


class TestEnvelopes:
    def test_error_envelope(self):
        env = EnvelopeError(
            request_id="req_001",
            success=False,
            error=EnvelopeErrorError(code="BAD_REQUEST", message="missing url"),
        )
        assert env.success is False
        assert env.error.code == "BAD_REQUEST"

    def test_handoff_envelope(self):
        env = EnvelopeHandoffAck(
            request_id="req_002",
            success=True,
            data=HandoffAck(job_id="job_abc", accepted_at="2026-05-07T12:00:00Z"),
        )
        assert env.success is True
        assert env.data.job_id == "job_abc"


class TestInternalRunPage:
    def test_minimal(self):
        req = InternalRunPage(url="https://example.com")
        assert req.url == "https://example.com"

    def test_with_run_id(self):
        req = InternalRunPage(url="https://example.com", run_id="run_xyz", zdr=True)
        assert req.run_id == "run_xyz"
        assert req.zdr is True


class TestInternalRunPageResult:
    def test_basic(self):
        result = InternalRunPageResult(
            run_id="run_xyz",
            status=200,
            fingerprint="blake3:aabbcc",
            links=["https://example.com/a", "https://example.com/b"],
        )
        assert len(result.links) == 2
        assert result.fingerprint == "blake3:aabbcc"
