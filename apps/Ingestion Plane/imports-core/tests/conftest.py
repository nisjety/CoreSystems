"""Pytest bootstrap for the imports-core suite (M0-CI).

app.config.Settings is read when app.db and app.main are imported, and four of its fields have no default.
A test module that imports the app before tests/space_upload_support.py (which sets the same four keys;
its INTERNAL_API_KEY value differs, and no test depends on either value) failed at collection in a clean
checkout, so the Space gate could not run the suite without secrets. Note: commit f0a19e6cb says 18 modules
failed; with env unset under `python -m pytest` it was 5. The 18 was the plain-pytest ModuleNotFoundError
count (no pythonpath), which pytest.ini fixes.

These values only fill variables that are not already set. They point at a closed port, so a test that
tries to reach a real service fails instead of reaching one.
"""

import os

os.environ.setdefault("DATABASE_URL", "postgresql+asyncpg://unused:unused@127.0.0.1:1/unused")
os.environ.setdefault("DOCUMENT_SERVICE_URL", "http://127.0.0.1:1")
os.environ.setdefault("ORG_SERVICE_URL", "http://127.0.0.1:1")
os.environ.setdefault("INTERNAL_API_KEY", "imports-core-test-internal-key")
