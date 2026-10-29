from __future__ import annotations

import base64
import json
import time

import cv2
import numpy as np
from fastapi.testclient import TestClient

from app.main import app


def png_b64(image: np.ndarray) -> str:
    ok, raw = cv2.imencode(".png", image)
    if not ok:
        raise RuntimeError("png encode failed")
    return base64.b64encode(raw.tobytes()).decode("ascii")


def fixture_pair() -> tuple[np.ndarray, np.ndarray]:
    prev = np.full((900, 1440, 3), 255, dtype=np.uint8)
    cv2.rectangle(prev, (80, 80), (360, 180), (20, 90, 200), -1)
    cv2.putText(prev, "ACME", (115, 145), cv2.FONT_HERSHEY_SIMPLEX, 2.0, (255, 255, 255), 4)
    cv2.rectangle(prev, (120, 300), (1260, 740), (245, 245, 245), -1)
    cv2.putText(prev, "Pricing", (160, 390), cv2.FONT_HERSHEY_SIMPLEX, 1.8, (60, 60, 60), 3)

    curr = prev.copy()
    cv2.rectangle(curr, (900, 330), (1210, 430), (30, 160, 100), -1)
    cv2.putText(curr, "New plan", (925, 395), cv2.FONT_HERSHEY_SIMPLEX, 1.2, (255, 255, 255), 3)
    return prev, curr


def main(iterations: int = 25) -> None:
    client = TestClient(app)
    prev, curr = fixture_pair()
    payload = {
        "run_id": "run_bench",
        "page_hash": "blake3:visual-bench",
        "step": 1,
        "previous_png_b64": png_b64(prev),
        "current_png_b64": png_b64(curr),
        "operations": {
            "diff": True,
            "screenshot_preprocessing": True,
            "thumbnail": True,
            "tiles": True,
            "ocr_preconditioning": True,
            "rendered_branding": True,
        },
    }
    started = time.perf_counter()
    last = None
    for _ in range(iterations):
        res = client.post("/v1/visual/observe", json=payload)
        res.raise_for_status()
        last = res.json()
    elapsed = time.perf_counter() - started
    print(
        json.dumps(
            {
                "fixture": "browser_before_after_pricing_card",
                "iterations": iterations,
                "mean_ms": round((elapsed / iterations) * 1000, 3),
                "changed": last["changed"] if last else None,
                "change_ratio": last["change_ratio"] if last else None,
                "regions": len(last["regions"]) if last else None,
            },
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()

