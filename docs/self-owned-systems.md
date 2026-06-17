# Self-Owned Open-Source Systems

CoreSystem uses these systems where owning data locally is more valuable than
calling a hosted provider API.

| Need | System | Owner | Integration |
| --- | --- | --- | --- |
| API/catalog discovery | [APIs.guru OpenAPI Directory](https://github.com/APIs-guru/openapi-directory) | Model Plane `capability-core` | Seeded in capability registry; mirror path `APIS_GURU_OPENAPI_DIRECTORY_PATH`; sync with `apps/Model Plane/scripts/sync-apis-guru-openapi-directory.sh`. |
| Document parsing | [Apache Tika](https://tika.apache.org/) | Ingestion/Data | Optional `self-owned-docs` service; imports-core uses `TIKA_URL` when `TIKA_ENABLED=true` and `DOCUMENT_PROCESSOR_ORDER=tika,local`. |
| OCR | [Tesseract OCR](https://github.com/tesseract-ocr/tesseract) | Ingestion/Data | Provided through the Tika full image in the `self-owned-docs` profile for scanned documents. |
| PDF/document conversion | [Gotenberg](https://github.com/gotenberg/gotenberg), [Stirling PDF](https://github.com/Stirling-Tools/stirling-pdf) | Ingestion | Optional `self-owned-docs` services with `GOTENBERG_URL` and `STIRLING_PDF_URL`. |
| Threat intel store | [MISP](https://www.misp-project.org/), [OpenCTI](https://github.com/OpenCTI-Platform/opencti) | Control/Ingestion security | Capability registry endpoint contract via `MISP_BASE_URL`/`OPENCTI_URL`; keep upstream stacks self-hosted outside the default plane compose. |
| URL/phishing feeds | [URLhaus](https://urlhaus.abuse.ch/api/), [PhishTank](https://www.phishtank.net/developer_info.php) | Quarry security | `scripts/sync-security-feeds.py` writes `config/security-feeds/quarry-security.json`; `quarry-edge` loads it at boot. |
| Vulnerability data | [NVD feeds](https://nvd.nist.gov/vuln/data-feeds) | Control security tooling | `apps/Control Plane/scripts/sync-nvd-feeds.py` mirrors recent/modified CVE 2.0 JSON to `config/security/nvd`. |
| Sanctions/company risk | [OpenSanctions yente](https://www.opensanctions.org/docs/on-premise/) | Data/Control | Optional Data Plane overlay `docker-compose.self-owned-data.yml` profile `self-owned-risk`; endpoint `OPENSANCTIONS_YENTE_URL`. |
| Knowledge graph enrichment | [Wikidata dumps](https://www.wikidata.org/wiki/Wikidata:Database_download), [Wikimedia dumps](https://dumps.wikimedia.org/) | Data | `scripts/write-open-data-manifest.py` records explicit dump sources before large ingestion jobs run. |
| Web-scale public corpus | [Common Crawl](https://commoncrawl.org/about) | Data/Ingestion lab | Included in the open-data manifest as lab-first corpus source using the Common Crawl collection index. |
| Geocoding/address data | [OpenStreetMap planet](https://planet.openstreetmap.org/) + [Nominatim](https://nominatim.org/) | Data/Application | Optional Data Plane overlay profile `self-owned-geo`; endpoint `NOMINATIM_URL`. |
| Disposable email blocking | [disposable-email-domains](https://github.com/disposable-email-domains/disposable-email-domains) | Control | `auth-core` blocks disposable signups using `DISPOSABLE_EMAIL_DOMAINS_FILE`; sync with `scripts/sync-disposable-email-domains.py`. |

## Common Commands

```bash
# Document parsing/OCR/conversion processors
cd "apps/Ingestion Plane"
docker compose --profile self-owned-docs up -d tika gotenberg stirling-pdf

# URLhaus/PhishTank host snapshot for Quarry
python3 scripts/sync-security-feeds.py --force

# APIs.guru local OpenAPI catalog mirror
cd "../Model Plane"
APIS_GURU_OPENAPI_DIRECTORY_PATH=/var/lib/capability-core/openapi-directory \
  scripts/sync-apis-guru-openapi-directory.sh

# Control Plane security feeds
cd "../Control Plane"
python3 scripts/sync-disposable-email-domains.py
python3 scripts/sync-nvd-feeds.py

# Data Plane self-owned risk/geocoding overlays
cd "../Data Plane v2"
docker compose -f docker-compose.self-owned-data.yml --profile self-owned-risk up -d
docker compose -f docker-compose.self-owned-data.yml --profile self-owned-geo up -d
python3 scripts/write-open-data-manifest.py
```

Heavy datasets are not downloaded by default. Treat Wikidata/Wikimedia,
Common Crawl, full OSM planet imports, and OpenSanctions reindexing as planned
data jobs with storage, retention, and ZDR boundary review.
