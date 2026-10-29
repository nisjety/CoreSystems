import base64
import os
import sys

import cv2
import numpy as np
from fastapi.testclient import TestClient

sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from app.main import app  # noqa: E402


client = TestClient(app)


def png_b64(image: np.ndarray) -> str:
    ok, raw = cv2.imencode(".png", image)
    assert ok
    return base64.b64encode(raw.tobytes()).decode("ascii")


def test_health_reports_opencv5_backend():
    res = client.get("/health")
    assert res.status_code == 200
    body = res.json()
    assert body["ok"] is True
    assert body["opencv_major"] >= 5
    assert body["backend"].startswith("opencv")


def test_observe_detects_region_and_returns_derivatives():
    prev = np.full((160, 240, 3), 255, dtype=np.uint8)
    curr = prev.copy()
    cv2.rectangle(curr, (40, 32), (110, 90), (30, 90, 220), -1)
    cv2.putText(curr, "ACME", (45, 70), cv2.FONT_HERSHEY_SIMPLEX, 0.7, (255, 255, 255), 2)

    res = client.post(
        "/v1/visual/observe",
        json={
            "run_id": "run_test",
            "page_hash": "blake3:test",
            "step": 2,
            "previous_png_b64": png_b64(prev),
            "current_png_b64": png_b64(curr),
            "max_regions": 8,
            "operations": {
                "diff": True,
                "screenshot_preprocessing": True,
                "thumbnail": True,
                "tiles": True,
                "ocr_preconditioning": True,
                "rendered_branding": True,
            },
        },
    )
    assert res.status_code == 200
    body = res.json()
    assert body["previous_available"] is True
    assert body["changed"] is True
    assert body["change_ratio"] > 0
    assert body["regions"]
    assert body["annotated_png_b64"]
    assert body["clean_png_b64"]
    assert body["thumbnail_png_b64"]
    assert body["tiles"]["tiles"]
    assert body["ocr_preprocessed_png_b64"]
    assert body["rendered_palette"]["colors"]


def test_observe_without_previous_is_not_a_change_but_preprocesses():
    image = np.full((96, 128, 3), 255, dtype=np.uint8)
    cv2.circle(image, (40, 40), 18, (0, 120, 255), -1)

    res = client.post(
        "/v1/visual/observe",
        json={
            "run_id": "run_test",
            "page_hash": "blake3:test",
            "step": 0,
            "current_png_b64": png_b64(image),
            "operations": {"diff": True, "screenshot_preprocessing": True},
        },
    )
    assert res.status_code == 200
    body = res.json()
    assert body["previous_available"] is False
    assert body["changed"] is False
    assert body["change_ratio"] == 0
    assert body["clean_png_b64"]


def test_preprocess_returns_clean_page_image():
    image = np.zeros((180, 260, 4), dtype=np.uint8)
    image[:, :, 3] = 0
    cv2.rectangle(image, (20, 20), (180, 120), (30, 200, 120, 255), -1)

    res = client.post(
        "/v1/visual/preprocess",
        json={
            "document_id": "doc_test",
            "image_png_b64": png_b64(image),
            "operations": {"thumbnail": True, "tiles": True, "rendered_branding": True},
        },
    )
    assert res.status_code == 200
    body = res.json()
    assert body["clean_png_b64"]
    assert body["thumbnail_png_b64"]
    assert body["tiles"]["image_width"] > 0
    assert body["rendered_palette"]["colors"]


def test_invalid_base64_is_400():
    res = client.post(
        "/v1/visual/preprocess",
        json={"document_id": "doc_test", "image_png_b64": "not-base64"},
    )
    assert res.status_code == 400

