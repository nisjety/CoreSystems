from __future__ import annotations

import base64
import binascii
import hashlib
import os
from typing import Any, Optional, Tuple

import cv2
import numpy as np
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field


MAX_IMAGE_BYTES = int(os.getenv("QUARRY_VISION_MAX_IMAGE_BYTES", str(25 * 1024 * 1024)))
DEFAULT_MAX_LONG_EDGE = int(os.getenv("QUARRY_VISION_MAX_LONG_EDGE", "2048"))
DEFAULT_THUMBNAIL_EDGE = int(os.getenv("QUARRY_VISION_THUMBNAIL_EDGE", "512"))
DEFAULT_TILE_SIZE = int(os.getenv("QUARRY_VISION_TILE_SIZE", "512"))
CV_THREADS = int(os.getenv("QUARRY_VISION_CV_THREADS", "1"))

cv2.setNumThreads(max(1, CV_THREADS))
cv2.setRNGSeed(0)

app = FastAPI(title="Quarry Vision Sidecar", version="0.1.0")


class VisualOperations(BaseModel):
    diff: bool = True
    screenshot_preprocessing: bool = True
    thumbnail: bool = True
    tiles: bool = True
    ocr_preconditioning: bool = False
    rendered_branding: bool = False


class VisualRegion(BaseModel):
    x: int
    y: int
    width: int
    height: int
    score: Optional[float] = None
    label: Optional[str] = None


class ObserveRequest(BaseModel):
    run_id: str
    page_hash: str
    step: int = Field(ge=0)
    previous_png_b64: Optional[str] = None
    current_png_b64: str
    max_regions: int = Field(default=32, ge=1, le=128)
    operations: VisualOperations = Field(default_factory=VisualOperations)


class ObserveResponse(BaseModel):
    version: int = 1
    backend: str
    step: int
    previous_available: bool
    changed: bool
    change_ratio: float
    regions: list[VisualRegion] = Field(default_factory=list)
    metrics: dict[str, Any] = Field(default_factory=dict)
    annotated_png_b64: Optional[str] = None
    clean_png_b64: Optional[str] = None
    thumbnail_png_b64: Optional[str] = None
    tiles: Optional[dict[str, Any]] = None
    ocr_preprocessed_png_b64: Optional[str] = None
    logo_candidate_png_b64: Optional[str] = None
    rendered_palette: Optional[dict[str, Any]] = None


class PreprocessRequest(BaseModel):
    document_id: str
    image_png_b64: str
    operations: VisualOperations = Field(default_factory=VisualOperations)


class PreprocessResponse(BaseModel):
    version: int = 1
    backend: str
    clean_png_b64: str
    metrics: dict[str, Any] = Field(default_factory=dict)
    thumbnail_png_b64: Optional[str] = None
    tiles: Optional[dict[str, Any]] = None
    ocr_preprocessed_png_b64: Optional[str] = None
    logo_candidate_png_b64: Optional[str] = None
    rendered_palette: Optional[dict[str, Any]] = None


def backend_name() -> str:
    major = cv2.__version__.split(".", 1)[0]
    return f"opencv{major}-sidecar:{cv2.__version__}"


@app.get("/health")
def health() -> dict[str, Any]:
    return {
        "ok": True,
        "backend": backend_name(),
        "opencv_version": cv2.__version__,
        "opencv_major": int(cv2.__version__.split(".", 1)[0]),
    }


@app.post("/v1/visual/observe", response_model=ObserveResponse)
def observe(req: ObserveRequest) -> ObserveResponse:
    current = _decode_png(req.current_png_b64, "current_png_b64")
    previous = (
        _decode_png(req.previous_png_b64, "previous_png_b64")
        if req.previous_png_b64
        else None
    )

    clean = _preprocess_screenshot(current)
    metrics = _image_metrics(current, clean)
    regions: list[VisualRegion] = []
    changed = False
    change_ratio = 0.0
    annotated: Optional[np.ndarray] = None

    if req.operations.diff and previous is not None:
        diff = _visual_diff(previous, clean, req.max_regions)
        regions = diff["regions"]
        changed = diff["changed"]
        change_ratio = diff["change_ratio"]
        annotated = diff["annotated"]
        metrics["diff"] = diff["metrics"]

    response = ObserveResponse(
        backend=backend_name(),
        step=req.step,
        previous_available=previous is not None,
        changed=changed,
        change_ratio=change_ratio,
        regions=regions,
        metrics=metrics,
        annotated_png_b64=_encode_png_b64(annotated) if annotated is not None else None,
        clean_png_b64=_encode_png_b64(clean) if req.operations.screenshot_preprocessing else None,
    )
    _attach_optional_derivatives(response, clean, req.operations)
    return response


@app.post("/v1/visual/preprocess", response_model=PreprocessResponse)
def preprocess(req: PreprocessRequest) -> PreprocessResponse:
    image = _decode_png(req.image_png_b64, "image_png_b64")
    clean = _preprocess_screenshot(image)
    response = PreprocessResponse(
        backend=backend_name(),
        clean_png_b64=_encode_png_b64(clean),
        metrics=_image_metrics(image, clean),
    )
    _attach_optional_derivatives(response, clean, req.operations)
    return response


def _decode_png(encoded: str, field: str) -> np.ndarray:
    try:
        raw = base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as exc:
        raise HTTPException(status_code=400, detail=f"{field} is not valid base64") from exc
    if len(raw) > MAX_IMAGE_BYTES:
        raise HTTPException(status_code=413, detail=f"{field} exceeds max image bytes")
    arr = np.frombuffer(raw, dtype=np.uint8)
    image = cv2.imdecode(arr, cv2.IMREAD_UNCHANGED)
    if image is None:
        raise HTTPException(status_code=400, detail=f"{field} is not a decodable image")
    return image


def _normalize_to_bgr(image: np.ndarray) -> np.ndarray:
    if image.ndim == 2:
        return cv2.cvtColor(image, cv2.COLOR_GRAY2BGR)
    if image.shape[2] == 3:
        return image.copy()
    if image.shape[2] == 4:
        bgr = image[:, :, :3].astype(np.float32)
        alpha = image[:, :, 3:4].astype(np.float32) / 255.0
        white = np.full_like(bgr, 255.0)
        composed = (bgr * alpha) + (white * (1.0 - alpha))
        return composed.astype(np.uint8)
    raise HTTPException(status_code=400, detail="unsupported image channel count")


def _resize_long_edge(image: np.ndarray, max_edge: int) -> np.ndarray:
    height, width = image.shape[:2]
    long_edge = max(height, width)
    if long_edge <= max_edge:
        return image
    scale = max_edge / float(long_edge)
    size = (max(1, int(round(width * scale))), max(1, int(round(height * scale))))
    return cv2.resize(image, size, interpolation=cv2.INTER_AREA)


def _preprocess_screenshot(image: np.ndarray) -> np.ndarray:
    bgr = _normalize_to_bgr(image)
    bgr = _resize_long_edge(bgr, DEFAULT_MAX_LONG_EDGE)
    return np.ascontiguousarray(bgr)


def _visual_diff(
    previous: np.ndarray, current_clean: np.ndarray, max_regions: int
) -> dict[str, Any]:
    previous_clean = _preprocess_screenshot(previous)
    height, width = current_clean.shape[:2]
    if previous_clean.shape[:2] != (height, width):
        previous_clean = cv2.resize(previous_clean, (width, height), interpolation=cv2.INTER_AREA)

    prev_gray = cv2.cvtColor(previous_clean, cv2.COLOR_BGR2GRAY)
    curr_gray = cv2.cvtColor(current_clean, cv2.COLOR_BGR2GRAY)
    diff = cv2.absdiff(prev_gray, curr_gray)
    diff = cv2.GaussianBlur(diff, (3, 3), 0)
    threshold = 18
    _, mask = cv2.threshold(diff, threshold, 255, cv2.THRESH_BINARY)
    kernel = np.ones((3, 3), np.uint8)
    mask = cv2.morphologyEx(mask, cv2.MORPH_OPEN, kernel)
    mask = cv2.dilate(mask, kernel, iterations=1)

    changed_pixels = int(cv2.countNonZero(mask))
    total_pixels = int(mask.shape[0] * mask.shape[1])
    change_ratio = changed_pixels / float(total_pixels) if total_pixels else 0.0

    contours, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    min_area = max(12, int(total_pixels * 0.00005))
    boxes: list[tuple[int, int, int, int, int]] = []
    for contour in contours:
        area = int(cv2.contourArea(contour))
        if area < min_area:
            continue
        x, y, w, h = cv2.boundingRect(contour)
        boxes.append((area, x, y, w, h))
    boxes.sort(key=lambda item: (-item[0], item[1], item[2], item[3], item[4]))

    regions = [
        VisualRegion(
            x=x,
            y=y,
            width=w,
            height=h,
            score=round(area / float(total_pixels), 6) if total_pixels else 0.0,
            label="changed",
        )
        for area, x, y, w, h in boxes[:max_regions]
    ]

    annotated = current_clean.copy()
    if changed_pixels:
        red = np.zeros_like(annotated)
        red[:, :] = (0, 0, 255)
        overlay_mask = mask > 0
        annotated[overlay_mask] = cv2.addWeighted(
            annotated[overlay_mask], 0.65, red[overlay_mask], 0.35, 0
        )
    for region in regions:
        cv2.rectangle(
            annotated,
            (region.x, region.y),
            (region.x + region.width, region.y + region.height),
            (0, 0, 255),
            2,
        )

    return {
        "changed": bool(change_ratio >= 0.0005 and len(regions) > 0),
        "change_ratio": round(change_ratio, 6),
        "regions": regions,
        "annotated": annotated,
        "metrics": {
            "threshold": threshold,
            "changed_pixels": changed_pixels,
            "total_pixels": total_pixels,
            "min_region_area": min_area,
            "region_count": len(regions),
        },
    }


def _make_thumbnail(image: np.ndarray) -> np.ndarray:
    return _resize_long_edge(image, DEFAULT_THUMBNAIL_EDGE)


def _make_tiles(image: np.ndarray) -> dict[str, Any]:
    height, width = image.shape[:2]
    tile_size = max(64, DEFAULT_TILE_SIZE)
    tiles: list[dict[str, Any]] = []
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    for y in range(0, height, tile_size):
        for x in range(0, width, tile_size):
            tile = gray[y : min(y + tile_size, height), x : min(x + tile_size, width)]
            non_white = np.count_nonzero(tile < 248)
            pixels = int(tile.size)
            tile_hash = hashlib.sha256(tile.tobytes()).hexdigest()
            tiles.append(
                {
                    "row": y // tile_size,
                    "col": x // tile_size,
                    "x": x,
                    "y": y,
                    "width": int(tile.shape[1]),
                    "height": int(tile.shape[0]),
                    "non_empty_ratio": round(non_white / float(pixels), 6) if pixels else 0.0,
                    "sha256": tile_hash,
                }
            )
    return {
        "version": 1,
        "tile_size": tile_size,
        "image_width": width,
        "image_height": height,
        "tiles": tiles,
    }


def _ocr_precondition(image: np.ndarray) -> np.ndarray:
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    clahe = cv2.createCLAHE(clipLimit=2.0, tileGridSize=(8, 8))
    gray = clahe.apply(gray)
    return cv2.adaptiveThreshold(
        gray,
        255,
        cv2.ADAPTIVE_THRESH_GAUSSIAN_C,
        cv2.THRESH_BINARY,
        35,
        11,
    )


def _extract_palette(image: np.ndarray) -> dict[str, Any]:
    sample = _resize_long_edge(image, 256)
    hsv = cv2.cvtColor(sample, cv2.COLOR_BGR2HSV)
    rgb = cv2.cvtColor(sample, cv2.COLOR_BGR2RGB)
    saturation = hsv[:, :, 1]
    value = hsv[:, :, 2]
    mask = (value > 24) & (value < 248) & ((saturation > 28) | (value < 220))
    pixels = rgb[mask]
    if pixels.size == 0:
        pixels = rgb.reshape(-1, 3)

    quantized = ((pixels.astype(np.uint16) // 32) * 32 + 16).astype(np.uint8)
    packed = (
        quantized[:, 0].astype(np.uint32) << 16
        | quantized[:, 1].astype(np.uint32) << 8
        | quantized[:, 2].astype(np.uint32)
    )
    values, counts = np.unique(packed, return_counts=True)
    ranked = sorted(zip(values.tolist(), counts.tolist()), key=lambda item: (-item[1], item[0]))
    total = int(sum(counts.tolist())) or 1
    colors = []
    for value_int, count in ranked[:8]:
        r = (value_int >> 16) & 0xFF
        g = (value_int >> 8) & 0xFF
        b = value_int & 0xFF
        colors.append(
            {
                "hex": f"#{r:02x}{g:02x}{b:02x}",
                "coverage": round(count / float(total), 6),
            }
        )
    return {
        "version": 1,
        "method": "opencv_quantized_histogram",
        "sampled_pixels": total,
        "colors": colors,
    }


def _extract_logo_candidate(
    image: np.ndarray,
) -> Tuple[Optional[np.ndarray], Optional[dict[str, Any]]]:
    height, width = image.shape[:2]
    top = image[: max(1, int(height * 0.45)), :]
    hsv = cv2.cvtColor(top, cv2.COLOR_BGR2HSV)
    saturation = hsv[:, :, 1]
    value = hsv[:, :, 2]
    mask = ((saturation > 36) & (value > 30) & (value < 248)).astype(np.uint8) * 255
    kernel = np.ones((5, 5), np.uint8)
    mask = cv2.morphologyEx(mask, cv2.MORPH_CLOSE, kernel)
    contours, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    min_area = max(80, int(width * height * 0.00025))
    best: Optional[Tuple[float, int, int, int, int]] = None
    for contour in contours:
        area = float(cv2.contourArea(contour))
        if area < min_area:
            continue
        x, y, w, h = cv2.boundingRect(contour)
        score = area / (1.0 + x * 0.002 + y * 0.004)
        candidate = (score, x, y, w, h)
        if best is None or candidate > best:
            best = candidate
    if best is None:
        return None, None
    _, x, y, w, h = best
    pad = 8
    x0 = max(0, x - pad)
    y0 = max(0, y - pad)
    x1 = min(width, x + w + pad)
    y1 = min(height, y + h + pad)
    crop = image[y0:y1, x0:x1].copy()
    region = {"x": x0, "y": y0, "width": x1 - x0, "height": y1 - y0}
    return crop, region


def _attach_optional_derivatives(response: Any, clean: np.ndarray, operations: VisualOperations) -> None:
    if operations.thumbnail:
        response.thumbnail_png_b64 = _encode_png_b64(_make_thumbnail(clean))
    if operations.tiles:
        response.tiles = _make_tiles(clean)
    if operations.ocr_preconditioning:
        response.ocr_preprocessed_png_b64 = _encode_png_b64(_ocr_precondition(clean))
    if operations.rendered_branding:
        palette = _extract_palette(clean)
        logo, region = _extract_logo_candidate(clean)
        if logo is not None:
            response.logo_candidate_png_b64 = _encode_png_b64(logo)
            palette["logo_candidate_region"] = region
        response.rendered_palette = palette


def _encode_png_b64(image: np.ndarray) -> str:
    ok, raw = cv2.imencode(".png", image, [cv2.IMWRITE_PNG_COMPRESSION, 3])
    if not ok:
        raise HTTPException(status_code=500, detail="failed to encode png")
    return base64.b64encode(raw.tobytes()).decode("ascii")


def _image_metrics(original: np.ndarray, clean: np.ndarray) -> dict[str, Any]:
    return {
        "original_width": int(original.shape[1]),
        "original_height": int(original.shape[0]),
        "clean_width": int(clean.shape[1]),
        "clean_height": int(clean.shape[0]),
        "max_long_edge": DEFAULT_MAX_LONG_EDGE,
        "thumbnail_edge": DEFAULT_THUMBNAIL_EDGE,
        "tile_size": DEFAULT_TILE_SIZE,
    }
