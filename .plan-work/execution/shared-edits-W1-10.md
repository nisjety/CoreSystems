## WS06-S8
shared-edit apps/Control Plane/docker-compose.yml: add this service after `audit-core` (4-space indent as in the file; <<: *core-security is the file's existing anchor):
    # WS06-S8: SIEM exporter. Absent unless the compose profile `siem` is selected;
    # AUDIT_SIEM_EXPORT_ENABLED=0 by default, in which case it idles and exports nothing.
    audit-siem-exporter:
        <<: *core-security
        profiles: ["siem"]
        build:
            context: ./audit-core
            dockerfile: Dockerfile
            args: *build-metadata
        container_name: audit-siem-exporter
        init: true
        restart: unless-stopped
        entrypoint: ["/usr/local/bin/siem-exporter"]
        environment:
            DATABASE_URL: postgres://${DB_USER}:${DB_PASSWORD:?DB_PASSWORD must be set in Control Plane .env}@controlplane-postgres:5432/${DB_NAME:-controlplane}?sslmode=disable
            ORG_CORE_URL: http://org-core:8080
            ORG_CORE_SERVICE_TOKEN: ${AUDIT_ORG_CORE_SERVICE_TOKEN:?AUDIT_ORG_CORE_SERVICE_TOKEN is required}
            AUDIT_SIEM_EXPORT_ENABLED: ${AUDIT_SIEM_EXPORT_ENABLED:-0}
            AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS: ${AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS:-0}
            ENVIRONMENT: ${ENVIRONMENT:-development}
        networks:
            - controlplane-net
        depends_on:
            controlplane-postgres:
                condition: service_healthy
        deploy:
            resources:
                limits:
                    memory: 64M
                    cpus: "0.25"
                reservations:
                    memory: 16M
        logging:
            driver: json-file
            options:
                max-size: "10m"
                max-file: "3"
(AUDIT_ORG_CORE_SERVICE_TOKEN and the org-core audit-core principal with scope org:siem:read:any already exist from S7. Apply this together with the production override already added to docker-compose.production.yml, and bring the exporter up only through scripts/run-control-plane.sh with --profile siem, never a bare docker compose build.)

