#!/usr/bin/env python3
"""Write an explicit manifest for self-owned public datasets.

This does not download the datasets. It records the source URLs and intended
plane ownership so ingestion jobs can be reviewed before large transfers start.
"""

from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path

DEFAULT_OUTPUT = "manifests/open-data/self-owned-open-data.json"

DATASETS = [
    {
        "id": "wikidata-json-dump",
        "owner_plane": "data",
        "use": "knowledge_graph_enrichment",
        "source_url": "https://dumps.wikimedia.org/wikidatawiki/entities/latest-all.json.gz",
        "landing_path_env": "WIKIDATA_DUMP_PATH",
    },
    {
        "id": "wikimedia-pages-articles",
        "owner_plane": "data",
        "use": "knowledge_graph_enrichment",
        "source_url": "https://dumps.wikimedia.org/",
        "landing_path_env": "WIKIMEDIA_DUMP_PATH",
    },
    {
        "id": "common-crawl-wet-paths",
        "owner_plane": "data",
        "consumer_plane": "ingestion",
        "use": "web_corpus_lab",
        "source_url": "https://index.commoncrawl.org/collinfo.json",
        "landing_path_env": "COMMON_CRAWL_MANIFEST_PATH",
    },
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default=os.getenv("OPEN_DATA_MANIFEST_PATH", DEFAULT_OUTPUT))
    args = parser.parse_args()

    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "schema_version": 1,
        "generated_at_unix": time.time(),
        "download_by_default": False,
        "datasets": DATASETS,
    }
    output.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote open data manifest to {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
