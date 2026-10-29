# Quarry Vision Sidecar

Isolated OpenCV 5 service for deterministic visual evidence processing.

The sidecar does not persist input images, does not call model providers, and
does not attempt anti-bot bypass. Quarry edge owns ZDR checks and artifact
persistence.

Endpoints:

- `GET /health`
- `POST /v1/visual/observe`
- `POST /v1/visual/preprocess`

Local test:

```bash
python -m venv .venv
. .venv/bin/activate
pip install -r requirements-dev.txt
pytest
```

