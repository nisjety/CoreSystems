"""Azure AI Language API.

Wraps the Azure AI Language service (formerly Cognitive Services Text Analytics)
for text analytics operations.

Capabilities exposed:
  POST /api/v1/language/sentiment       — sentiment analysis + opinion mining
  POST /api/v1/language/entities        — named entity recognition (NER)
  POST /api/v1/language/key-phrases     — key phrase extraction
  POST /api/v1/language/pii             — PII detection and redaction
  POST /api/v1/language/summary/text    — extractive + abstractive text summarisation
  POST /api/v1/language/summary/conversation  — conversation summarisation
  POST /api/v1/language/detect          — language detection

All operations use the synchronous Azure AI Language REST API
(api-version ``2024-11-15-preview`` by default).

Returns 501 if ``AZURE_AI_LANGUAGE_ENDPOINT`` or ``AZURE_AI_LANGUAGE_KEY``
are not set.
"""

from __future__ import annotations

import logging
import uuid
from typing import Any

import httpx
from fastapi import APIRouter, HTTPException
from pydantic import BaseModel, Field

from app.config import get_settings

logger = logging.getLogger(__name__)

router = APIRouter(prefix="/api/v1/language", tags=["language"])

# ── HTTP client ───────────────────────────────────────────────────────────────

_http: httpx.AsyncClient | None = None


def _client() -> httpx.AsyncClient:
    global _http
    if _http is None:
        _http = httpx.AsyncClient(timeout=30.0)
    return _http


def _require_language() -> tuple[str, str, str]:
    """Return (endpoint, api_key, api_version) or raise 501."""
    s = get_settings()
    if not s.azure_ai_language_endpoint or not s.azure_ai_language_key:
        raise HTTPException(
            status_code=501,
            detail="Azure AI Language (AZURE_AI_LANGUAGE_ENDPOINT / AZURE_AI_LANGUAGE_KEY) not configured",
        )
    return (
        s.azure_ai_language_endpoint.rstrip("/"),
        s.azure_ai_language_key,
        s.azure_ai_language_api_version,
    )


def _headers(api_key: str) -> dict[str, str]:
    return {"Ocp-Apim-Subscription-Key": api_key, "Content-Type": "application/json"}


async def _analyze(endpoint: str, api_key: str, api_version: str, body: dict) -> dict:
    """Call the synchronous Azure AI Language :analyze-text endpoint."""
    url = f"{endpoint}/language/:analyze-text?api-version={api_version}"
    resp = await _client().post(url, headers=_headers(api_key), json=body)
    if resp.status_code == 200:
        return resp.json()
    logger.error("azure_language_error status=%d body=%s", resp.status_code, resp.text[:300])
    raise HTTPException(status_code=resp.status_code, detail=resp.text[:400])


def _docs(texts: list[str], language: str | None) -> list[dict]:
    return [
        {"id": str(i + 1), "text": t, **({"language": language} if language else {})}
        for i, t in enumerate(texts)
    ]


# ── Shared request shape ──────────────────────────────────────────────────────


class LanguageRequest(BaseModel):
    texts: list[str] = Field(..., min_length=1, max_length=25)
    language: str | None = Field(None, description="ISO 639-1 code, e.g. 'en'. None = auto-detect.")
    org_id: str = ""


# ── Sentiment ────────────────────────────────────────────────────────────────


class SentimentResult(BaseModel):
    id: str
    sentiment: str   # positive | negative | neutral | mixed
    confidence_scores: dict[str, float]
    sentences: list[dict[str, Any]] | None = None


@router.post("/sentiment", response_model=list[SentimentResult])
async def analyze_sentiment(body: LanguageRequest):
    """Sentence-level and document-level sentiment analysis with opinion mining."""
    endpoint, key, version = _require_language()
    data = await _analyze(endpoint, key, version, {
        "kind": "SentimentAnalysis",
        "analysisInput": {"documents": _docs(body.texts, body.language)},
        "parameters": {"opinionMining": True},
    })
    results = data.get("results", {}).get("documents", [])
    return [
        SentimentResult(
            id=r["id"],
            sentiment=r["sentiment"],
            confidence_scores=r.get("confidenceScores", {}),
            sentences=r.get("sentences"),
        )
        for r in results
    ]


# ── NER ──────────────────────────────────────────────────────────────────────


class EntityRecognitionResult(BaseModel):
    id: str
    entities: list[dict[str, Any]]


@router.post("/entities", response_model=list[EntityRecognitionResult])
async def recognize_entities(body: LanguageRequest):
    """Named Entity Recognition — persons, locations, organisations, dates, etc."""
    endpoint, key, version = _require_language()
    data = await _analyze(endpoint, key, version, {
        "kind": "EntityRecognition",
        "analysisInput": {"documents": _docs(body.texts, body.language)},
    })
    results = data.get("results", {}).get("documents", [])
    return [EntityRecognitionResult(id=r["id"], entities=r.get("entities", [])) for r in results]


# ── Key phrases ──────────────────────────────────────────────────────────────


class KeyPhrasesResult(BaseModel):
    id: str
    key_phrases: list[str]


@router.post("/key-phrases", response_model=list[KeyPhrasesResult])
async def extract_key_phrases(body: LanguageRequest):
    """Extract the main topics (key phrases) from text."""
    endpoint, key, version = _require_language()
    data = await _analyze(endpoint, key, version, {
        "kind": "KeyPhraseExtraction",
        "analysisInput": {"documents": _docs(body.texts, body.language)},
    })
    results = data.get("results", {}).get("documents", [])
    return [KeyPhrasesResult(id=r["id"], key_phrases=r.get("keyPhrases", [])) for r in results]


# ── PII detection ─────────────────────────────────────────────────────────────


class PIIResult(BaseModel):
    id: str
    redacted_text: str
    entities: list[dict[str, Any]]


@router.post("/pii", response_model=list[PIIResult])
async def detect_pii(body: LanguageRequest):
    """Detect and redact Personally Identifiable Information (PII)."""
    endpoint, key, version = _require_language()
    data = await _analyze(endpoint, key, version, {
        "kind": "PiiEntityRecognition",
        "analysisInput": {"documents": _docs(body.texts, body.language)},
        "parameters": {"piiCategories": ["All"], "redactionCharacter": "*"},
    })
    results = data.get("results", {}).get("documents", [])
    return [
        PIIResult(
            id=r["id"],
            redacted_text=r.get("redactedText", ""),
            entities=r.get("entities", []),
        )
        for r in results
    ]


# ── Language detection ────────────────────────────────────────────────────────


class DetectedLanguage(BaseModel):
    id: str
    language_name: str
    iso_code: str
    confidence: float


@router.post("/detect", response_model=list[DetectedLanguage])
async def detect_language(body: LanguageRequest):
    """Detect the language of each text snippet."""
    endpoint, key, version = _require_language()
    data = await _analyze(endpoint, key, version, {
        "kind": "LanguageDetection",
        "analysisInput": {"documents": [{"id": str(i + 1), "text": t} for i, t in enumerate(body.texts)]},
    })
    results = data.get("results", {}).get("documents", [])
    return [
        DetectedLanguage(
            id=r["id"],
            language_name=r.get("detectedLanguage", {}).get("name", ""),
            iso_code=r.get("detectedLanguage", {}).get("iso6391Name", ""),
            confidence=r.get("detectedLanguage", {}).get("confidenceScore", 0.0),
        )
        for r in results
    ]


# ── Summarisation ─────────────────────────────────────────────────────────────


class SummarizationRequest(LanguageRequest):
    sentence_count: int = Field(3, ge=1, le=20)
    kind: str = Field("AbstractiveSummarization", description="AbstractiveSummarization | ExtractiveSummarization")


class SummarizationResult(BaseModel):
    id: str
    summary: str


@router.post("/summary/text", response_model=list[SummarizationResult])
async def summarize_text(body: SummarizationRequest):
    """Abstractive or extractive text summarisation.

    Set ``kind`` to ``"AbstractiveSummarization"`` (default) for LLM-generated
    summaries or ``"ExtractiveSummarization"`` for sentence-extraction.
    """
    endpoint, key, version = _require_language()

    # Summarisation is a long-running job — use the /analyze-text/jobs endpoint
    submit_url = f"{endpoint}/language/analyze-text/jobs?api-version={version}"
    job_payload = {
        "displayName":   f"summary-{uuid.uuid4()}",
        "analysisInput": {"documents": _docs(body.texts, body.language)},
        "tasks": [
            {
                "kind": body.kind,
                "parameters": {
                    "summaryCount":  body.sentence_count,
                    "modelVersion":  "latest",
                },
            }
        ],
    }

    try:
        submit_resp = await _client().post(
            submit_url, headers=_headers(key), json=job_payload
        )
        if submit_resp.status_code not in (200, 202):
            raise HTTPException(status_code=submit_resp.status_code, detail=submit_resp.text[:400])

        # Poll for results (Operation-Location header)
        op_url = submit_resp.headers.get("operation-location")
        if not op_url:
            # Some API versions return inline results at 200
            result_data = submit_resp.json()
        else:
            import asyncio
            for _ in range(12):  # max 60 s
                await asyncio.sleep(5)
                poll_resp = await _client().get(op_url, headers=_headers(key))
                poll_data = poll_resp.json()
                if poll_data.get("status") in ("succeeded", "failed"):
                    result_data = poll_data
                    break
            else:
                raise HTTPException(status_code=504, detail="Summarisation job timed out")

        # Extract results
        tasks = ((result_data.get("tasks") or {}).get("items") or [])
        docs_out: list[SummarizationResult] = []
        for task in tasks:
            for doc in (task.get("results") or {}).get("documents") or []:
                summaries = doc.get("summaries") or doc.get("sentences") or []
                text = " ".join(s.get("text", "") for s in summaries)
                docs_out.append(SummarizationResult(id=doc["id"], summary=text))
        return docs_out
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("summarize_text_exception")
        raise HTTPException(status_code=503, detail=str(exc))
