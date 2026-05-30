"""Analysis router — handles PDF conversion, text extraction, image analysis."""
import json
import logging

from fastapi import APIRouter, HTTPException
from pydantic import BaseModel

from app.config import settings
from app.events.publisher import get_shared_nats
from app.storage.minio_client import get_object_bytes, put_object_bytes

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/analyze", tags=["analyze"])


class PdfConversionRequest(BaseModel):
    org_id: str
    document_id: str
    minio_key: str


class TextExtractionRequest(BaseModel):
    org_id: str
    document_id: str
    minio_key: str


class ImageAnalysisRequest(BaseModel):
    org_id: str
    document_id: str
    minio_key: str


@router.post("/pdf-conversion")
async def pdf_conversion(body: PdfConversionRequest):
    try:
        data = get_object_bytes(settings.minio_bucket, body.minio_key)
        # TODO: actual PDF page-count / conversion logic
        page_count = 1
        out_key = f"{body.org_id}/{body.document_id}/converted.pdf"
        put_object_bytes(settings.minio_bucket, out_key, data, "application/pdf")
        shared = get_shared_nats()
        await shared.publish_pdf_conversion_completed(
            org_id=body.org_id,
            document_id=body.document_id,
            page_count=page_count,
            minio_key=out_key,
        )
        return {"status": "ok", "page_count": page_count, "minio_key": out_key}
    except Exception as e:
        logger.error("PDF conversion failed: %s", e)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/text-extraction")
async def text_extraction(body: TextExtractionRequest):
    try:
        data = get_object_bytes(settings.minio_bucket, body.minio_key)
        # TODO: actual text extraction logic
        text = data.decode("utf-8", errors="ignore")
        char_count = len(text)
        out_key = f"{body.org_id}/{body.document_id}/extracted.txt"
        put_object_bytes(settings.minio_bucket, out_key, text.encode(), "text/plain")
        shared = get_shared_nats()
        await shared.publish_text_extracted(
            org_id=body.org_id,
            document_id=body.document_id,
            char_count=char_count,
            minio_key=out_key,
        )
        return {"status": "ok", "char_count": char_count, "minio_key": out_key}
    except Exception as e:
        logger.error("Text extraction failed: %s", e)
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/image-analysis")
async def image_analysis(body: ImageAnalysisRequest):
    try:
        data = get_object_bytes(settings.minio_bucket, body.minio_key)
        # TODO: actual image analysis logic
        image_count = 1
        out_key = f"{body.org_id}/{body.document_id}/images_analyzed.json"
        result = json.dumps({"image_count": image_count}).encode()
        put_object_bytes(settings.minio_bucket, out_key, result, "application/json")
        shared = get_shared_nats()
        await shared.publish_image_analyzed(
            org_id=body.org_id,
            document_id=body.document_id,
            image_count=image_count,
            minio_key=out_key,
        )
        return {"status": "ok", "image_count": image_count, "minio_key": out_key}
    except Exception as e:
        logger.error("Image analysis failed: %s", e)
        raise HTTPException(status_code=500, detail=str(e))
