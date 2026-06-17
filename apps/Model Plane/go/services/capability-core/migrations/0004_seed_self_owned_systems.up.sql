-- 0004_seed_self_owned_systems.up.sql
--
-- Self-owned open-source systems that replace hosted provider APIs where data
-- ownership matters. capability-core owns discovery/policy metadata even when
-- the runtime service is deployed in another plane.

WITH seed (
    id,
    name,
    description,
    risk_level,
    rollout_state,
    schema_input,
    schema_output,
    config_json,
    tags
) AS (
    VALUES
    (
        'cap.self_owned.apis_guru_openapi_directory',
        'apis-guru-openapi-directory',
        'Self-owned OpenAPI catalog mirror for API and capability discovery.',
        'low',
        'stable',
        '{"type":"object","properties":{"query":{"type":"string"},"provider":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"specs":{"type":"array"}}}'::jsonb,
        '{"owner_plane":"model","owner_service":"capability-core","runtime_state":"registry-ready","source_url":"https://github.com/APIs-guru/openapi-directory","mirror_path_env":"APIS_GURU_OPENAPI_DIRECTORY_PATH"}'::jsonb,
        ARRAY['self-owned','openapi','catalog','apis-guru']
    ),
    (
        'cap.self_owned.apache_tika',
        'apache-tika-document-parser',
        'Self-hosted document parsing and text extraction through Ingestion Plane imports-core.',
        'medium',
        'stable',
        '{"type":"object","properties":{"filename":{"type":"string"},"contentType":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"text":{"type":"string"},"metadata":{"type":"object"}}}'::jsonb,
        '{"owner_plane":"ingestion","consumer_plane":"data","owner_service":"imports-core","runtime_state":"implemented","endpoint_env":"TIKA_URL","default_endpoint":"http://tika:9998","compose_profile":"self-owned-docs","source_url":"https://tika.apache.org/"}'::jsonb,
        ARRAY['self-owned','document-parsing','tika','ingestion']
    ),
    (
        'cap.self_owned.tesseract_ocr',
        'tesseract-ocr',
        'Self-owned OCR runtime used by the Tika full image for scanned documents.',
        'medium',
        'stable',
        '{"type":"object","properties":{"imageOrPdf":{"type":"string"},"language":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"text":{"type":"string"}}}'::jsonb,
        '{"owner_plane":"ingestion","consumer_plane":"data","owner_service":"imports-core","runtime_state":"implemented-via-tika-full","compose_profile":"self-owned-docs","source_url":"https://github.com/tesseract-ocr/tesseract"}'::jsonb,
        ARRAY['self-owned','ocr','tesseract','ingestion']
    ),
    (
        'cap.self_owned.gotenberg',
        'gotenberg-document-conversion',
        'Self-hosted document and HTML to PDF conversion service.',
        'medium',
        'stable',
        '{"type":"object","properties":{"sourceFormat":{"type":"string"},"targetFormat":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"artifactRef":{"type":"string"},"contentType":{"type":"string"}}}'::jsonb,
        '{"owner_plane":"ingestion","owner_service":"imports-core","runtime_state":"service-ready","endpoint_env":"GOTENBERG_URL","default_endpoint":"http://gotenberg:3000","compose_profile":"self-owned-docs","source_url":"https://github.com/gotenberg/gotenberg"}'::jsonb,
        ARRAY['self-owned','document-conversion','pdf','gotenberg']
    ),
    (
        'cap.self_owned.stirling_pdf',
        'stirling-pdf',
        'Self-hosted PDF operations and conversion helper for Ingestion Plane.',
        'medium',
        'stable',
        '{"type":"object","properties":{"operation":{"type":"string"},"artifactRef":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"artifactRef":{"type":"string"},"contentType":{"type":"string"}}}'::jsonb,
        '{"owner_plane":"ingestion","owner_service":"imports-core","runtime_state":"service-ready","endpoint_env":"STIRLING_PDF_URL","default_endpoint":"http://stirling-pdf:8080","compose_profile":"self-owned-docs","source_url":"https://github.com/Stirling-Tools/stirling-pdf"}'::jsonb,
        ARRAY['self-owned','document-conversion','pdf','stirling-pdf']
    ),
    (
        'cap.self_owned.misp_opencti',
        'misp-opencti-threat-intel-store',
        'Self-owned threat intelligence store endpoint contract for MISP or OpenCTI.',
        'high',
        'canary',
        '{"type":"object","properties":{"indicator":{"type":"string"},"type":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"matches":{"type":"array"},"source":{"type":"string"}}}'::jsonb,
        '{"owner_plane":"control","consumer_plane":"ingestion","runtime_state":"external-self-host-contract","misp_url_env":"MISP_BASE_URL","opencti_url_env":"OPENCTI_URL","source_urls":["https://www.misp-project.org/","https://github.com/OpenCTI-Platform/opencti"]}'::jsonb,
        ARRAY['self-owned','threat-intel','misp','opencti','security']
    ),
    (
        'cap.self_owned.quarry_url_reputation_feeds',
        'quarry-url-reputation-feeds',
        'Local URLhaus and PhishTank feed snapshot consumed by quarry-security preflight.',
        'high',
        'stable',
        '{"type":"object","properties":{"url":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"decision":{"type":"string"},"reason":{"type":"string"}}}'::jsonb,
        '{"owner_plane":"ingestion","owner_service":"quarry-edge","runtime_state":"implemented","snapshot_env":"QUARRY_SECURITY_SNAPSHOT_PATH","default_snapshot":"/app/data/security-feeds/quarry-security.json","sync_script":"apps/Ingestion Plane/scripts/sync-security-feeds.py","source_urls":["https://urlhaus.abuse.ch/api/","https://www.phishtank.net/developer_info.php"]}'::jsonb,
        ARRAY['self-owned','url-reputation','urlhaus','phishtank','quarry']
    ),
    (
        'cap.self_owned.nvd_vulnerability_data',
        'nvd-vulnerability-data',
        'Self-owned NVD CVE/CPE feed mirror for Control Plane security tooling.',
        'medium',
        'canary',
        '{"type":"object","properties":{"cve":{"type":"string"},"cpe":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"vulnerabilities":{"type":"array"}}}'::jsonb,
        '{"owner_plane":"control","runtime_state":"feed-contract","mirror_path_env":"NVD_FEED_CACHE_PATH","source_url":"https://nvd.nist.gov/vuln/data-feeds"}'::jsonb,
        ARRAY['self-owned','vulnerability','nvd','cve','security']
    ),
    (
        'cap.self_owned.opensanctions_yente',
        'opensanctions-yente',
        'Self-hosted sanctions and company risk screening API for Data/Control Plane.',
        'high',
        'canary',
        '{"type":"object","properties":{"query":{"type":"string"},"dataset":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"results":{"type":"array"}}}'::jsonb,
        '{"owner_plane":"data","consumer_plane":"control","runtime_state":"compose-profile-ready","endpoint_env":"OPENSANCTIONS_YENTE_URL","default_endpoint":"http://opensanctions-yente:8000","compose_file":"apps/Data Plane v2/docker-compose.self-owned-data.yml","compose_profile":"self-owned-risk","source_url":"https://www.opensanctions.org/docs/on-premise/"}'::jsonb,
        ARRAY['self-owned','sanctions','company-risk','opensanctions','yente']
    ),
    (
        'cap.self_owned.wikimedia_wikidata_dumps',
        'wikimedia-wikidata-dumps',
        'Local Wikimedia/Wikidata dump manifests for Data Plane graph enrichment.',
        'medium',
        'canary',
        '{"type":"object","properties":{"dump":{"type":"string"},"entity":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"manifest":{"type":"object"},"entities":{"type":"array"}}}'::jsonb,
        '{"owner_plane":"data","runtime_state":"manifest-ready","manifest_path_env":"OPEN_DATA_MANIFEST_PATH","source_urls":["https://www.wikidata.org/wiki/Wikidata:Database_download","https://dumps.wikimedia.org/"]}'::jsonb,
        ARRAY['self-owned','knowledge-graph','wikidata','wikimedia','data-plane']
    ),
    (
        'cap.self_owned.common_crawl_lab',
        'common-crawl-lab',
        'Lab-first Common Crawl corpus manifests for Data/Ingestion experimentation.',
        'medium',
        'canary',
        '{"type":"object","properties":{"crawlId":{"type":"string"},"prefix":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"records":{"type":"array"},"manifest":{"type":"object"}}}'::jsonb,
        '{"owner_plane":"data","consumer_plane":"ingestion","runtime_state":"lab-first-manifest","manifest_path_env":"OPEN_DATA_MANIFEST_PATH","source_url":"https://commoncrawl.org/about"}'::jsonb,
        ARRAY['self-owned','common-crawl','corpus','lab']
    ),
    (
        'cap.self_owned.nominatim_osm',
        'nominatim-openstreetmap',
        'Self-hosted OSM/Nominatim geocoding for Data/Application Plane.',
        'medium',
        'canary',
        '{"type":"object","properties":{"query":{"type":"string"},"lat":{"type":"number"},"lon":{"type":"number"}}}'::jsonb,
        '{"type":"object","properties":{"places":{"type":"array"}}}'::jsonb,
        '{"owner_plane":"data","consumer_plane":"application","runtime_state":"compose-profile-ready","endpoint_env":"NOMINATIM_URL","default_endpoint":"http://nominatim:8080","compose_file":"apps/Data Plane v2/docker-compose.self-owned-data.yml","compose_profile":"self-owned-geo","source_urls":["https://planet.openstreetmap.org/","https://nominatim.org/"]}'::jsonb,
        ARRAY['self-owned','geocoding','openstreetmap','nominatim']
    ),
    (
        'cap.self_owned.disposable_email_domains',
        'disposable-email-domains',
        'Local disposable email domain blocklist enforced by auth-core signup.',
        'medium',
        'stable',
        '{"type":"object","properties":{"email":{"type":"string"}}}'::jsonb,
        '{"type":"object","properties":{"blocked":{"type":"boolean"},"domain":{"type":"string"}}}'::jsonb,
        '{"owner_plane":"control","owner_service":"auth-core","runtime_state":"implemented","file_env":"DISPOSABLE_EMAIL_DOMAINS_FILE","default_file":"/app/config/security/disposable-email-domains.txt","sync_script":"apps/Control Plane/scripts/sync-disposable-email-domains.py","source_url":"https://github.com/disposable-email-domains/disposable-email-domains"}'::jsonb,
        ARRAY['self-owned','auth','email-risk','disposable-email']
    )
)
INSERT INTO capabilities (
    id,
    org_id,
    kind,
    name,
    version,
    description,
    risk_level,
    scope,
    lazy_load,
    enabled,
    idempotency_key,
    schema_input,
    schema_output,
    config_json,
    tags,
    enabled_for_scopes,
    rollout_state,
    created_by
)
SELECT
    id,
    'global',
    'connector',
    name,
    '1.0.0',
    description,
    risk_level,
    'global',
    true,
    true,
    'self-owned:' || name || ':v1',
    schema_input,
    schema_output,
    config_json,
    tags,
    ARRAY['global'],
    rollout_state,
    'migration:0004_seed_self_owned_systems'
FROM seed
ON CONFLICT (id) DO UPDATE SET
    description = EXCLUDED.description,
    risk_level = EXCLUDED.risk_level,
    scope = EXCLUDED.scope,
    lazy_load = EXCLUDED.lazy_load,
    enabled = EXCLUDED.enabled,
    idempotency_key = EXCLUDED.idempotency_key,
    schema_input = EXCLUDED.schema_input,
    schema_output = EXCLUDED.schema_output,
    config_json = EXCLUDED.config_json,
    tags = EXCLUDED.tags,
    enabled_for_scopes = EXCLUDED.enabled_for_scopes,
    rollout_state = EXCLUDED.rollout_state,
    updated_at = now();
