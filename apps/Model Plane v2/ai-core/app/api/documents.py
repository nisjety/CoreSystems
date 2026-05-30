"""Document Intelligence API — Hybrid two-tier architecture.

Tier 1 — Azure Document Intelligence (structured documents):
  Best for: invoices, receipts, forms, IDs, standard templates
  Cost: ~$0.001-0.01/page | Latency: ~2-5 s | Compliance: ISO 27001, GDPR, HIPAA

Tier 2 — Mistral Document AI via Azure AI Foundry Serverless:
  Best for: legal contracts, research papers, complex reports, clause analysis
  Model: mistral-document-ai-2505 | Context: 128K tokens (~100 pages)
  Cost: $3.00 input / $15.00 output per 1M tokens | Latency: ~5-15 s

Use ``POST /api/v1/documents/analyze/mistral`` for Tier 2 reasoning.
All other endpoints use Tier 1 Azure Document Intelligence.
"""

from __future__ import annotations

import uuid
from typing import Any

import httpx
from fastapi import APIRouter, HTTPException, Response
from pydantic import BaseModel, Field

from app.config import get_settings

logger = __import__("logging").getLogger(__name__)

router = APIRouter(prefix="/api/v1/documents", tags=["documents"])


# ── Request / response models ───────────────────────────────────────────────


class DocumentRequest(BaseModel):
    """Either a public URL or base64-encoded file content."""

    url: str | None = Field(None, description="Publicly reachable document URL")
    content_base64: str | None = Field(None, description="Base64-encoded file bytes")
    model: str = "prebuilt-document"
    org_id: str = ""


class DocumentAnalysisResult(BaseModel):
    request_id: str
    model: str
    status: str  # succeeded | running | failed
    fields: dict[str, Any] = {}
    tables: list[dict[str, Any]] = []
    paragraphs: list[str] = []
    raw: dict[str, Any] | None = None


class LayoutResult(BaseModel):
    request_id: str
    tables: list[dict[str, Any]] = []
    paragraphs: list[str] = []
    raw: dict[str, Any] | None = None


class FormResult(BaseModel):
    request_id: str
    fields: dict[str, Any] = {}
    raw: dict[str, Any] | None = None


class ReceiptResult(BaseModel):
    request_id: str
    merchant_name: str | None = None
    transaction_date: str | None = None
    total: float | None = None
    items: list[dict[str, Any]] = []
    raw: dict[str, Any] | None = None


class InvoiceResult(BaseModel):
    request_id: str
    vendor_name: str | None = None
    invoice_id: str | None = None
    invoice_date: str | None = None
    due_date: str | None = None
    total_amount: float | None = None
    line_items: list[dict[str, Any]] = []
    raw: dict[str, Any] | None = None


class BusinessCardResult(BaseModel):
    request_id: str
    name: str | None = None
    email: str | None = None
    phone: str | None = None
    company: str | None = None
    raw: dict[str, Any] | None = None


# ── Helper ──────────────────────────────────────────────────────────────────


def _require_client():
    """Return an azure-ai-documentintelligence client or raise 501."""
    settings = get_settings()
    endpoint = settings.azure_document_intelligence_endpoint
    key = settings.azure_document_intelligence_key

    if not endpoint or not key:
        raise HTTPException(
            status_code=501,
            detail="azure_document_intelligence_endpoint and azure_document_intelligence_key not configured",
        )

    try:
        from azure.ai.documentintelligence import DocumentIntelligenceClient
        from azure.core.credentials import AzureKeyCredential
    except ImportError:
        raise HTTPException(
            status_code=501,
            detail="azure-ai-documentintelligence package not installed",
        )

    return DocumentIntelligenceClient(
        endpoint=endpoint,
        credential=AzureKeyCredential(key),
    )


async def _analyze(model_id: str, req: DocumentRequest) -> dict[str, Any]:
    """Common analysis helper; returns raw SDK result dict."""
    client = _require_client()
    from azure.ai.documentintelligence.models import AnalyzeDocumentRequest, DocumentContentFormat

    try:
        if req.url:
            poller = await client.begin_analyze_document(
                model_id,
                AnalyzeDocumentRequest(url_source=req.url),
                content_type="application/json",
                output_content_format=DocumentContentFormat.TEXT,
            )
        elif req.content_base64:
            import base64
            raw_bytes = base64.b64decode(req.content_base64)
            poller = await client.begin_analyze_document(
                model_id,
                raw_bytes,
                content_type="application/octet-stream",
                output_content_format=DocumentContentFormat.TEXT,
            )
        else:
            raise HTTPException(status_code=422, detail="Provide url or content_base64")

        result = await poller.result()
        return result.as_dict()
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("document_intelligence_error model=%s", model_id)
        raise HTTPException(status_code=503, detail=str(exc))


# ── Endpoints ───────────────────────────────────────────────────────────────


@router.post("/analyze", response_model=DocumentAnalysisResult)
async def analyze(body: DocumentRequest):
    """General document analysis with configurable prebuilt model."""
    request_id = str(uuid.uuid4())
    raw = await _analyze(body.model, body)

    fields: dict[str, Any] = {}
    if docs := raw.get("documents"):
        for doc in docs:
            for k, v in (doc.get("fields") or {}).items():
                fields[k] = v.get("content") or v.get("value")

    paragraphs = [p.get("content", "") for p in (raw.get("paragraphs") or [])]
    tables: list[dict[str, Any]] = raw.get("tables") or []

    return DocumentAnalysisResult(
        request_id=request_id,
        model=body.model,
        status="succeeded",
        fields=fields,
        tables=tables,
        paragraphs=paragraphs,
        raw=raw,
    )


@router.post("/layout", response_model=LayoutResult)
async def extract_layout(body: DocumentRequest):
    """Extract layout information (tables, paragraphs, reading order)."""
    request_id = str(uuid.uuid4())
    raw = await _analyze("prebuilt-layout", body)
    paragraphs = [p.get("content", "") for p in (raw.get("paragraphs") or [])]
    return LayoutResult(request_id=request_id, tables=raw.get("tables") or [], paragraphs=paragraphs, raw=raw)


@router.post("/forms", response_model=FormResult)
async def extract_form(body: DocumentRequest):
    """Extract form fields (key-value pairs) from a document."""
    request_id = str(uuid.uuid4())
    raw = await _analyze("prebuilt-document", body)

    fields: dict[str, Any] = {}
    for kv in raw.get("keyValuePairs") or []:
        key = (kv.get("key") or {}).get("content", "")
        val = (kv.get("value") or {}).get("content", "")
        if key:
            fields[key] = val

    return FormResult(request_id=request_id, fields=fields, raw=raw)


@router.post("/receipts", response_model=ReceiptResult)
async def parse_receipt(body: DocumentRequest):
    """Extract structured data from a receipt."""
    request_id = str(uuid.uuid4())
    raw = await _analyze("prebuilt-receipt", body)

    fields: dict[str, Any] = {}
    if docs := raw.get("documents"):
        for doc in docs:
            for k, v in (doc.get("fields") or {}).items():
                fields[k] = v.get("content") or v.get("value")

    items: list[dict[str, Any]] = []
    if raw_items := (fields.pop("Items", None)):
        if isinstance(raw_items, list):
            items = raw_items

    return ReceiptResult(
        request_id=request_id,
        merchant_name=fields.get("MerchantName"),
        transaction_date=fields.get("TransactionDate"),
        total=fields.get("Total"),
        items=items,
        raw=raw,
    )


@router.post("/invoices", response_model=InvoiceResult)
async def parse_invoice(body: DocumentRequest):
    """Extract structured data from an invoice."""
    request_id = str(uuid.uuid4())
    raw = await _analyze("prebuilt-invoice", body)

    fields: dict[str, Any] = {}
    if docs := raw.get("documents"):
        for doc in docs:
            for k, v in (doc.get("fields") or {}).items():
                fields[k] = v.get("content") or v.get("value")

    items: list[dict[str, Any]] = []
    if raw_items := fields.pop("Items", None):
        if isinstance(raw_items, list):
            items = raw_items

    return InvoiceResult(
        request_id=request_id,
        vendor_name=fields.get("VendorName"),
        invoice_id=fields.get("InvoiceId"),
        invoice_date=fields.get("InvoiceDate"),
        due_date=fields.get("DueDate"),
        total_amount=fields.get("InvoiceTotal"),
        line_items=items,
        raw=raw,
    )


@router.post("/business-cards", response_model=BusinessCardResult)
async def parse_business_card(body: DocumentRequest):
    """Extract contact information from a business card."""
    request_id = str(uuid.uuid4())
    raw = await _analyze("prebuilt-businessCard", body)

    fields: dict[str, Any] = {}
    if docs := raw.get("documents"):
        for doc in docs:
            for k, v in (doc.get("fields") or {}).items():
                fields[k] = v.get("content") or v.get("value")

    return BusinessCardResult(
        request_id=request_id,
        name=fields.get("ContactNames"),
        email=fields.get("Emails"),
        phone=fields.get("PhoneNumbers"),
        company=fields.get("CompanyNames"),
        raw=raw,
    )


# ── Tier 2: Mistral Document AI ───────────────────────────────────────────────


class MistralDocumentRequest(BaseModel):
    """Tier 2: complex document reasoning via Mistral Document AI.

    Supply either a publicly reachable URL *or* base64-encoded bytes.
    The document is embedded as a file part in the multimodal chat request
    (Mistral Document AI accepts PDF/DOCX/PPTX/XLSX as first-class inputs).
    """

    url: str | None = Field(None, description="Publicly reachable document URL (PDF recommended)")
    content_base64: str | None = Field(None, description="Base64-encoded document bytes")
    mimetype: str = Field("application/pdf", description="MIME type of the document bytes")
    question: str = Field(
        "Summarise this document and extract the key findings.",
        description="The question or task to perform over the document.",
    )
    max_tokens: int = Field(2048, ge=64, le=8192)
    org_id: str = ""


class MistralDocumentResult(BaseModel):
    request_id: str
    answer: str
    model: str
    input_tokens: int | None = None
    output_tokens: int | None = None


def _require_mistral_doc() -> tuple[str, str]:
    """Return (endpoint, api_key) for Mistral Document AI or raise 501."""
    s = get_settings()
    if not s.mistral_document_ai_endpoint or not s.mistral_document_ai_key:
        raise HTTPException(
            status_code=501,
            detail=(
                "Mistral Document AI not configured. "
                "Set MISTRAL_DOCUMENT_AI_ENDPOINT and MISTRAL_DOCUMENT_AI_KEY "
                "(Azure AI Foundry → Model Catalog → mistral-document-ai-2505 → Serverless API)."
            ),
        )
    return s.mistral_document_ai_endpoint.rstrip("/"), s.mistral_document_ai_key


_mistral_http: httpx.AsyncClient | None = None


def _mistral_client() -> httpx.AsyncClient:
    global _mistral_http
    if _mistral_http is None:
        import httpx as _httpx
        _mistral_http = _httpx.AsyncClient(timeout=120.0)
    return _mistral_http


@router.post("/analyze/mistral", response_model=MistralDocumentResult)
async def analyze_with_mistral(body: MistralDocumentRequest):
    """Tier 2: Deep semantic reasoning over complex documents using Mistral Document AI.

    Ideal for legal contracts, research papers, financial reports, and any
    document requiring multi-page reasoning beyond what structured OCR can provide.

    The ``question`` field drives the analysis — ask for summaries, clause extraction,
    compliance checks, comparative analysis, etc.

    Backed by ``mistral-document-ai-2505`` deployed as a Serverless API on
    Azure AI Foundry (Sweden Central).
    """
    import base64 as _b64
    import httpx as _httpx

    request_id = str(uuid.uuid4())
    endpoint, api_key = _require_mistral_doc()

    # Build the document_url content block
    if body.url:
        doc_block: dict[str, Any] = {
            "type": "document_url",
            "document_url": body.url,
        }
    elif body.content_base64:
        doc_block = {
            "type": "document_url",
            "document_url": f"data:{body.mimetype};base64,{body.content_base64}",
        }
    else:
        raise HTTPException(status_code=422, detail="Provide url or content_base64")

    # Mistral Document AI chat completion format
    chat_payload = {
        "model":      "mistral-document-ai-2505",
        "max_tokens": body.max_tokens,
        "messages": [
            {
                "role": "user",
                "content": [
                    doc_block,
                    {"type": "text", "text": body.question},
                ],
            }
        ],
    }

    try:
        resp = await _mistral_client().post(
            f"{endpoint}/v1/chat/completions",
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type":  "application/json",
            },
            json=chat_payload,
        )
        if resp.status_code == 200:
            data = resp.json()
            choice = (data.get("choices") or [{}])[0]
            answer = (choice.get("message") or {}).get("content") or ""
            usage = data.get("usage") or {}
            return MistralDocumentResult(
                request_id=request_id,
                answer=answer,
                model=data.get("model", "mistral-document-ai-2505"),
                input_tokens=usage.get("prompt_tokens"),
                output_tokens=usage.get("completion_tokens"),
            )
        logger.error("mistral_doc_error status=%d body=%s", resp.status_code, resp.text[:300])
        raise HTTPException(status_code=resp.status_code, detail=resp.text[:400])
    except HTTPException:
        raise
    except Exception as exc:
        logger.exception("mistral_document_exception request_id=%s", request_id)
        raise HTTPException(status_code=503, detail=str(exc))



