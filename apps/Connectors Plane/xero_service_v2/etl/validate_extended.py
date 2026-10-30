"""Offline contract check for a local ``xero sync-extended --dry-run`` tree."""

import argparse
import json
from pathlib import Path

from etl.xero.extended import EXTENDED_ENDPOINTS, extract_records


def _validate_pair(root: Path, sidecar: Path, endpoint: str) -> int:
    parts = sidecar.relative_to(root).parts
    raw_index = next(
        (index for index in range(len(parts) - 1) if parts[index:index + 2] == ("raw", "xero")),
        None,
    )
    if raw_index is None:
        raise ValueError("invalid raw object path")
    raw_parts = parts[raw_index:]
    if len(raw_parts) != 7:
        raise ValueError("invalid raw object path")
    _, _, tenant, version, _, _, filename = raw_parts
    if not filename.endswith(".json.meta.json"):
        raise ValueError("invalid sidecar filename")
    expected_version = "identity" if endpoint.startswith("identity_") else (
        "2.0" if endpoint.startswith("projects_") else "1.0"
    )
    if version != expected_version:
        raise ValueError("API version does not match endpoint family")

    meta = json.loads(sidecar.read_text(encoding="utf-8"))
    if not isinstance(meta, dict):
        raise ValueError("sidecar is not a JSON object")
    if meta.get("x-tenant-id") != tenant:
        raise ValueError("sidecar tenant does not match object path")
    if meta.get("x-endpoint") != endpoint:
        raise ValueError("sidecar endpoint does not match object path")
    if meta.get("x-api-version") != version:
        raise ValueError("sidecar API version does not match object path")
    if meta.get("x-api-family") not in (None, endpoint.split("_", 1)[0]):
        raise ValueError("sidecar API family does not match endpoint")
    if str(meta.get("x-http-status")) != "200":
        raise ValueError("sidecar HTTP status is not 200")
    try:
        expected_count = int(meta["x-record-count"])
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError("invalid sidecar record count") from exc
    if expected_count < 0:
        raise ValueError("invalid sidecar record count")

    data_path = sidecar.with_name(filename.removesuffix(".meta.json"))
    if not data_path.is_file():
        raise ValueError("missing data file")
    data = json.loads(data_path.read_text(encoding="utf-8"))
    actual_count = len(extract_records(data, endpoint, meta))
    if actual_count != expected_count:
        raise ValueError("record count does not match response")
    return actual_count


def validate_tree(root: Path) -> dict:
    """Return a payload-free summary; errors never include source values."""
    root = Path(root)
    if not root.is_dir():
        return {"files": 0, "records": 0, "endpoints": {},
                "errors": ["local output directory does not exist"]}
    counts: dict[str, int] = {}
    errors: list[str] = []
    files = 0
    for sidecar in sorted(root.rglob("*.json.meta.json")):
        endpoint = sidecar.parent.parent.name
        if endpoint not in EXTENDED_ENDPOINTS:
            continue
        files += 1
        try:
            count = _validate_pair(root, sidecar, endpoint)
        except (OSError, ValueError, KeyError, TypeError) as exc:
            reason = str(exc) if isinstance(exc, ValueError) else "unreadable JSON or file"
            if isinstance(exc, json.JSONDecodeError):
                reason = "invalid JSON"
            errors.append(f"{endpoint}: {reason}")
            continue
        counts[endpoint] = counts.get(endpoint, 0) + count
    if not files:
        errors.append("no extended sidecars found")
    return {"files": files, "records": sum(counts.values()),
            "endpoints": counts, "errors": errors}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("local_dir", type=Path, help="--local-dir used by sync-extended")
    args = parser.parse_args()
    report = validate_tree(args.local_dir)
    print(json.dumps(report, indent=2, sort_keys=True))
    return 1 if report["errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
